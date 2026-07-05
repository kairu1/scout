//! Engagement 4 — visit path + crash recovery.

use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::Instant;

use scout::index::recovery::{sentinel_path, shutdown};
use scout::index::visit::record_visit;
use scout::index::{pragma, signals};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-rec-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn seed_paths(conn: &rusqlite::Connection, count: usize) {
    let tx = conn.unchecked_transaction().unwrap();
    {
        let mut stmt = tx
            .prepare("INSERT INTO paths (path, scan_generation) VALUES (:path, 1)")
            .unwrap();
        for i in 0..count {
            stmt.execute(rusqlite::named_params! { ":path": format!("/fixture/p{i:06}") })
                .unwrap();
        }
    }
    tx.commit().unwrap();
}

#[test]
fn visit_updates_frecency_and_total() {
    signals::reset_interrupt();
    let dir = temp_dir("visit");
    let db = dir.join("index.db");
    let conn = pragma::open(&db).unwrap();
    seed_paths(&conn, 3);

    let id: i64 = conn
        .query_row("SELECT rowid FROM paths LIMIT 1", [], |r| r.get(0))
        .unwrap();
    assert!(record_visit(&conn, id).unwrap());
    assert!(record_visit(&conn, id).unwrap());

    let (s, visits): (f64, i64) = conn
        .query_row("SELECT S, visits_total FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id }, |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(s > 1.9 && s <= 2.0, "S after two immediate visits: {s}");
    assert_eq!(visits, 2);

    // Tombstoned rows are never credited.
    conn.execute("UPDATE paths SET tombstoned_at = 1 WHERE rowid = :id",
        rusqlite::named_params! { ":id": id }).unwrap();
    assert!(!record_visit(&conn, id).unwrap());

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn visit_median_under_budget_on_100k_rows() {
    signals::reset_interrupt();
    let dir = temp_dir("bench");
    let db = dir.join("index.db");
    let conn = pragma::open(&db).unwrap();
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
    // ADR-001 budget is 5 ms; runbook fails the test above 10 ms.
    assert!(median_us <= 10_000, "record_visit median {median_us} µs");
    println!("record_visit median: {median_us} µs over 1000 calls on 100k rows");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn sentinel_lifecycle() {
    signals::reset_interrupt();
    let dir = temp_dir("sentinel");
    let db = dir.join("index.db");

    let conn = pragma::open(&db).unwrap();
    assert!(!sentinel_path(&db).exists(), "sentinel must be absent while running");
    shutdown(conn).unwrap();
    assert!(sentinel_path(&db).exists(), "sentinel must exist after clean shutdown");

    let conn = pragma::open(&db).unwrap();
    assert!(!sentinel_path(&db).exists(), "sentinel must be consumed on open");
    shutdown(conn).unwrap();

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn corrupt_db_renamed_and_rebuilt() {
    signals::reset_interrupt();
    let dir = temp_dir("corrupt");
    let db = dir.join("index.db");

    // Build a valid DB with content, then crash (drop without shutdown —
    // sentinel stays absent) and corrupt page 2.
    {
        let conn = pragma::open(&db).unwrap();
        seed_paths(&conn, 500);
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        // drop: no sentinel written
    }
    {
        let mut f = fs::OpenOptions::new().write(true).open(&db).unwrap();
        f.seek(SeekFrom::Start(4096)).unwrap();
        f.write_all(&[0xFF; 2048]).unwrap();
    }

    let conn = pragma::open(&db).unwrap();
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 0, "rebuilt index must be fresh");
    shutdown(conn).unwrap();

    let corpses: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
        .collect();
    assert!(!corpses.is_empty(), "corrupt DB was not renamed aside");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn healthy_db_survives_missing_sentinel() {
    signals::reset_interrupt();
    let dir = temp_dir("healthy");
    let db = dir.join("index.db");

    {
        let conn = pragma::open(&db).unwrap();
        seed_paths(&conn, 100);
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        // drop without shutdown: simulated crash, data intact
    }

    let conn = pragma::open(&db).unwrap();
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 100, "healthy data must survive the integrity check");
    shutdown(conn).unwrap();

    fs::remove_dir_all(&dir).unwrap();
}
