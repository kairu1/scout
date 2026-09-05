//! Opening the index: WAL and PRAGMAs, private modes, symlink refusal.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use scout::index;

use crate::temp_dir;

#[test]
fn a_fresh_db_is_wal_with_normal_sync_and_no_autocheckpoint() {
    let dir = temp_dir("pragma");
    let db = dir.join("index.db");
    let conn = index::open(&db).unwrap();

    let (journal, synchronous, autocheckpoint) = index::pragma_state(&conn).unwrap();
    assert!(journal.eq_ignore_ascii_case("wal"), "journal_mode = {journal}");
    assert_eq!(synchronous, 1, "synchronous NORMAL");
    assert_eq!(autocheckpoint, 0, "wal_autocheckpoint");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_db_and_its_wal_siblings_are_0600_under_a_0700_parent() {
    let dir = temp_dir("perm");
    let parent = dir.join("scout-data");
    let db = parent.join("index.db");
    let _conn = index::open(&db).unwrap();

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
fn a_symlinked_db_path_is_refused() {
    let dir = temp_dir("symlink");
    let real = dir.join("real.db");
    fs::write(&real, b"").unwrap();
    let link = dir.join("link.db");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let err = index::open(&link).unwrap_err();
    assert!(
        matches!(err, scout::Error::IndexRefused(ref what) if what.contains("symlink")),
        "{err}"
    );

    fs::remove_dir_all(&dir).unwrap();
}
