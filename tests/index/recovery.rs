//! Crash recovery: the sentinel, rename-aside on corruption, and a
//! healthy database surviving a missing sentinel.

use std::fs;
use std::io::{Seek, SeekFrom, Write};

use scout::index;
use scout::index::recovery::{sentinel_path, shutdown};

use crate::{seed_paths, temp_dir};

#[test]
fn the_sentinel_exists_only_between_a_clean_shutdown_and_the_next_open() {
    let dir = temp_dir("sentinel");
    let db = dir.join("index.db");

    let conn = index::open(&db).unwrap();
    assert!(!sentinel_path(&db).exists(), "sentinel must be absent while running");
    shutdown(conn).unwrap();
    assert!(sentinel_path(&db).exists(), "sentinel must exist after clean shutdown");

    let conn = index::open(&db).unwrap();
    assert!(!sentinel_path(&db).exists(), "sentinel must be consumed on open");
    shutdown(conn).unwrap();

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_corrupt_db_is_renamed_aside_and_rebuilt() {
    let dir = temp_dir("corrupt");
    let db = dir.join("index.db");

    // Build a valid DB with content, then crash (drop without shutdown,
    // so the sentinel stays absent) and corrupt page 2.
    {
        let conn = index::open(&db).unwrap();
        seed_paths(&conn, 500);
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
    }
    {
        let mut f = fs::OpenOptions::new().write(true).open(&db).unwrap();
        f.seek(SeekFrom::Start(4096)).unwrap();
        f.write_all(&[0xFF; 2048]).unwrap();
    }

    let conn = index::open(&db).unwrap();
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
fn a_healthy_db_survives_a_missing_sentinel() {
    let dir = temp_dir("healthy");
    let db = dir.join("index.db");

    {
        let conn = index::open(&db).unwrap();
        seed_paths(&conn, 100);
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
        // drop without shutdown: simulated crash, data intact
    }

    let conn = index::open(&db).unwrap();
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap();
    assert_eq!(rows, 100, "healthy data must survive the integrity check");
    shutdown(conn).unwrap();

    fs::remove_dir_all(&dir).unwrap();
}
