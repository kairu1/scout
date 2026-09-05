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
