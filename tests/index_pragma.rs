//! Engagement 2 — WAL + PRAGMA + O_NOFOLLOW (ADR-001 pacing, ADR-003 §4).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use scout::index::pragma;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-test-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn fresh_db_reports_wal_and_pragmas() {
    let dir = temp_dir("pragma");
    let db = dir.join("index.db");
    let conn = pragma::open(&db).unwrap();

    let (journal, synchronous, autocheckpoint) = pragma::pragma_state(&conn).unwrap();
    assert!(journal.eq_ignore_ascii_case("wal"), "journal_mode = {journal}");
    assert_eq!(synchronous, 1, "synchronous NORMAL");
    assert_eq!(autocheckpoint, 0, "wal_autocheckpoint");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn db_file_created_0600_parent_0700() {
    let dir = temp_dir("perm");
    let parent = dir.join("scout-data");
    let db = parent.join("index.db");
    let _conn = pragma::open(&db).unwrap();

    let dir_mode = fs::metadata(&parent).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700, "parent dir mode");
    let db_mode = fs::metadata(&db).unwrap().permissions().mode() & 0o777;
    assert_eq!(db_mode, 0o600, "db mode");
    for suffix in ["-wal", "-shm"] {
        let sibling = PathBuf::from(format!("{}{}", db.display(), suffix));
        let mode = fs::metadata(&sibling).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "sibling {suffix} mode");
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn symlinked_db_path_refuses() {
    let dir = temp_dir("symlink");
    let real = dir.join("real.db");
    fs::write(&real, b"").unwrap();
    let link = dir.join("link.db");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let err = pragma::open(&link).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("symlink"), "unexpected error: {msg}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn query_active_atomic_present() {
    use std::sync::atomic::Ordering;
    // Phase 3 search workers store; Phase 2 only asserts the spine exists
    // and starts at 0 (never-queried sentinel).
    assert_eq!(scout::ipc::QUERY_ACTIVE.load(Ordering::Relaxed), 0);
    assert!(scout::ipc::now_ms() > 0);
}
