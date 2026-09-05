//! The streaming walker and the batched writer.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use scout::index;
use scout::index::walk::{refused_at_boundary, walk, WalkConfig};
use scout::index::write::batched_insert;
use scout::platform::signals;

use crate::{serial, temp_dir};

pub fn open_db(dir: &Path) -> Connection {
    index::open(&dir.join("index.db")).unwrap()
}

pub fn make_files(root: &Path, count: usize) {
    for i in 0..count {
        fs::write(root.join(format!("file-{i:05}.txt")), b"x").unwrap();
    }
}

pub fn row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap()
}

pub fn current_generation(conn: &Connection) -> i64 {
    conn.query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn a_walk_indexes_every_file_and_the_root() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("100");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 100);

    let mut conn = open_db(&dir);
    let stats = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();

    // 100 files + the root dir itself.
    assert_eq!(stats.inserted, 101);
    assert!(stats.completed);
    assert_eq!(row_count(&conn), 101);
    assert_eq!(current_generation(&conn), 1);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn batches_commit_in_batch_size_chunks() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("batch");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 1000);

    let mut conn = open_db(&dir);
    let stats = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 100).unwrap();

    // 1001 entries at batch size 100 = 11 transactions.
    assert_eq!(stats.inserted, 1001);
    assert_eq!(stats.batches, 11);
    assert!(stats.completed);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_interrupted_walk_leaves_the_generation_unadvanced() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("interrupt");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 50);

    let mut conn = open_db(&dir);
    signals::request_interrupt();
    let stats = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 10).unwrap();
    signals::reset_interrupt();

    assert!(!stats.completed);
    assert_eq!(current_generation(&conn), 0, "partial generation must not become current");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rescan_advances_the_generation_without_duplicating_rows() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("rescan");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 25);

    let mut conn = open_db(&dir);
    let first = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(first.generation, 1);
    let rows_after_first = row_count(&conn);

    let second = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(current_generation(&conn), 2);
    assert_eq!(row_count(&conn), rows_after_first, "UNIQUE path upsert duplicated rows");

    let stale: i64 = conn
        .query_row("SELECT count(*) FROM paths WHERE scan_generation != 2", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stale, 0, "all rows must carry the new generation");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn system_paths_hazardous_bytes_and_long_paths_are_refused_at_the_boundary() {
    assert!(refused_at_boundary(Path::new("/proc/self/status")));
    assert!(refused_at_boundary(Path::new("/sys/kernel")));
    assert!(refused_at_boundary(Path::new("/dev/null")));
    assert!(refused_at_boundary(Path::new("/tmp/evil\nname")));
    assert!(!refused_at_boundary(Path::new("/home/user/project")));
    let long = format!("/tmp/{}", "a".repeat(5000));
    assert!(refused_at_boundary(Path::new(&long)));
}

/// A path that disappears between walks is tombstoned when the next walk
/// completes, leaves the candidate set, and comes back with its frecency
/// intact when it reappears. Tombstoning rather than deleting is what
/// keeps the history across an unmount, a re-clone or a branch switch.
#[test]
fn a_completed_walk_tombstones_paths_that_vanished_and_revives_them_with_history() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("tombstone");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 3);
    let victim = tree.join("file-00001.txt");

    let mut conn = open_db(&dir);
    batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let id: i64 = conn
        .query_row(
            "SELECT rowid FROM paths WHERE path = :p",
            rusqlite::named_params! { ":p": victim.to_str().unwrap() },
            |r| r.get(0),
        )
        .unwrap();
    assert!(scout::index::frecency::record_visit(&conn, id, 1_800_000_000).unwrap());

    fs::remove_file(&victim).unwrap();
    let second = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(second.tombstoned, 1, "exactly the vanished row is tombstoned");
    let tombstoned_at: Option<i64> = conn
        .query_row(
            "SELECT tombstoned_at FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id },
            |r| r.get(0),
        )
        .unwrap();
    assert!(tombstoned_at.is_some(), "tombstoned_at must be SET, not merely implied by generation");
    let live: Vec<String> =
        scout::search::load_candidates(&conn).unwrap().into_iter().map(|c| c.path).collect();
    assert!(
        !live.iter().any(|p| p == victim.to_str().unwrap()),
        "a tombstoned row is not a candidate"
    );

    fs::write(&victim, b"back").unwrap();
    let third = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(third.tombstoned, 0);
    let (tomb, visits): (Option<i64>, i64) = conn
        .query_row(
            "SELECT tombstoned_at, visits_total FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id },
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(tomb.is_none(), "a reappearing path is revived");
    assert_eq!(visits, 1, "and keeps its history");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rescan_of_an_unchanged_tree_tombstones_nothing() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("no-tombstone");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 10);

    let mut conn = open_db(&dir);
    batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let second = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(second.tombstoned, 0);
    assert_eq!(second.purged, 0);
    let tombstoned: i64 = conn
        .query_row("SELECT count(*) FROM paths WHERE tombstoned_at IS NOT NULL", [], |r| r.get(0))
        .unwrap();
    assert_eq!(tombstoned, 0);

    fs::remove_dir_all(&dir).unwrap();
}

/// Tombstones older than the purge window are deleted at the next
/// completed walk; younger ones stay so a returning path keeps its score.
#[test]
fn old_tombstones_are_purged_and_recent_ones_kept() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("purge");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 2);

    let mut conn = open_db(&dir);
    batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    // Plant two tombstones by hand: one ancient, one from yesterday.
    let now = scout::platform::time::unix_now();
    conn.execute(
        "INSERT INTO paths (path, scan_generation, tombstoned_at) VALUES ('/gone/ancient', 0, :t)",
        rusqlite::named_params! { ":t": now - scout::index::write::PURGE_AFTER_SECS - 1 },
    )
    .unwrap();
    conn.execute(
        "INSERT INTO paths (path, scan_generation, tombstoned_at) VALUES ('/gone/recent', 0, :t)",
        rusqlite::named_params! { ":t": now - 86_400 },
    )
    .unwrap();

    let stats = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(stats.purged, 1);
    let remaining: Vec<String> = conn
        .prepare("SELECT path FROM paths WHERE path LIKE '/gone/%' ORDER BY path")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(remaining, vec!["/gone/recent".to_string()]);

    fs::remove_dir_all(&dir).unwrap();
}
