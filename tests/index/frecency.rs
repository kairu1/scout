//! The visit credit.

use std::fs;
use std::time::Instant;

use scout::index;
use scout::index::frecency::record_visit;

use crate::{seed_paths, temp_dir};

#[test]
fn a_visit_raises_the_score_and_the_total_and_skips_tombstoned_rows() {
    let dir = temp_dir("visit");
    let db = dir.join("index.db");
    let conn = index::open(&db).unwrap();
    seed_paths(&conn, 3);

    let id: i64 = conn.query_row("SELECT rowid FROM paths LIMIT 1", [], |r| r.get(0)).unwrap();
    assert!(record_visit(&conn, id).unwrap());
    assert!(record_visit(&conn, id).unwrap());

    let (s, visits): (f64, i64) = conn
        .query_row(
            "SELECT S, visits_total FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id },
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(s > 1.9 && s <= 2.0, "S after two immediate visits: {s}");
    assert_eq!(visits, 2);

    // Tombstoned rows are never credited.
    conn.execute(
        "UPDATE paths SET tombstoned_at = 1 WHERE rowid = :id",
        rusqlite::named_params! { ":id": id },
    )
    .unwrap();
    assert!(!record_visit(&conn, id).unwrap());

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
        record_visit(&conn, id).unwrap();
        latencies.push(t.elapsed().as_micros());
    }
    latencies.sort_unstable();
    let median_us = latencies[latencies.len() / 2];
    // The budget is 5 ms; the test fails above 10 ms.
    assert!(median_us <= 10_000, "record_visit median {median_us} µs");
    println!("record_visit median: {median_us} µs over 1000 calls on 100k rows");

    fs::remove_dir_all(&dir).unwrap();
}
