//! The visit credit.

use std::fs;
use std::time::Instant;

use scout::index;
use scout::index::frecency::{record_visit, CREDIT_WINDOW_SECS};

use crate::{seed_paths, temp_dir};

#[test]
fn a_visit_raises_the_score_and_the_total_and_skips_tombstoned_rows() {
    let dir = temp_dir("visit");
    let db = dir.join("index.db");
    let conn = index::open(&db).unwrap();
    seed_paths(&conn, 3);

    let now = 1_800_000_000;
    let id: i64 = conn.query_row("SELECT rowid FROM paths LIMIT 1", [], |r| r.get(0)).unwrap();
    assert!(record_visit(&conn, id, now).unwrap());
    assert!(record_visit(&conn, id, now + CREDIT_WINDOW_SECS).unwrap());

    let (s, visits): (f64, i64) = conn
        .query_row(
            "SELECT S, visits_total FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id },
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(s > 1.9 && s <= 2.0, "S after two visits ten seconds apart: {s}");
    assert_eq!(visits, 2);

    // Tombstoned rows are never credited.
    conn.execute(
        "UPDATE paths SET tombstoned_at = 1 WHERE rowid = :id",
        rusqlite::named_params! { ":id": id },
    )
    .unwrap();
    assert!(!record_visit(&conn, id, now + 2 * CREDIT_WINDOW_SECS).unwrap());

    fs::remove_dir_all(&dir).unwrap();
}

/// The rate limit is a property of the row, not of the process: a second
/// credit inside the window is refused by the statement itself, so it
/// holds across a long-lived session and across two processes.
#[test]
fn a_second_credit_inside_the_window_is_refused_by_the_row() {
    let dir = temp_dir("window");
    let db = dir.join("index.db");
    let conn = index::open(&db).unwrap();
    seed_paths(&conn, 1);
    let id: i64 = conn.query_row("SELECT rowid FROM paths LIMIT 1", [], |r| r.get(0)).unwrap();
    let now = 1_800_000_000;

    assert!(record_visit(&conn, id, now).unwrap(), "a never-visited row credits");
    assert!(!record_visit(&conn, id, now + 5).unwrap(), "five seconds later is inside the window");
    assert!(!record_visit(&conn, id, now + CREDIT_WINDOW_SECS - 1).unwrap(), "one second short");
    assert!(
        record_visit(&conn, id, now + CREDIT_WINDOW_SECS).unwrap(),
        "the window boundary credits"
    );

    // A second connection opened the way another scout process would open
    // it sees the same rule, because the rule is in the row.
    let other = index::open(&db).unwrap();
    assert!(!record_visit(&other, id, now + CREDIT_WINDOW_SECS + 1).unwrap());

    let visits: i64 = conn
        .query_row(
            "SELECT visits_total FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id },
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(visits, 2);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_visit_credit_stays_under_budget_on_100k_rows() {
    let dir = temp_dir("bench");
    let db = dir.join("index.db");
    let conn = index::open(&db).unwrap();
    seed_paths(&conn, 100_000);

    let mut latencies: Vec<u128> = Vec::with_capacity(1000);
    for i in 0..1000u64 {
        let id = (i * 97 % 100_000 + 1) as i64;
        let t = Instant::now();
        record_visit(&conn, id, 1_800_000_000 + i as i64 * 100).unwrap();
        latencies.push(t.elapsed().as_micros());
    }
    latencies.sort_unstable();
    let median_us = latencies[latencies.len() / 2];
    // The budget is 5 ms; the test fails above 10 ms.
    assert!(median_us <= 10_000, "record_visit median {median_us} µs");
    println!("record_visit median: {median_us} µs over 1000 calls on 100k rows");

    fs::remove_dir_all(&dir).unwrap();
}
