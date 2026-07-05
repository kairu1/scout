//! Engagement 3 — streaming walker + batched insert.
//! The interrupt flag is process-global; tests that touch the insert
//! path serialize on one mutex so a tripped flag cannot leak into a
//! concurrently running test.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

use rusqlite::Connection;
use scout::index::insert::batched_insert;
use scout::index::walk::{walk, WalkConfig};
use scout::index::{pragma, signals};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-walk-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn open_db(dir: &std::path::Path) -> Connection {
    pragma::open(&dir.join("index.db")).unwrap()
}

fn make_files(root: &std::path::Path, count: usize) {
    for i in 0..count {
        fs::write(root.join(format!("file-{i:05}.txt")), b"x").unwrap();
    }
}

fn row_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap()
}

fn current_generation(conn: &Connection) -> i64 {
    conn.query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn walk_and_insert_100_files() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("100");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 100);

    let mut conn = open_db(&dir);
    let stats =
        batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();

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

    // 1001 entries at batch size 100 → 11 transactions.
    assert_eq!(stats.inserted, 1001);
    assert_eq!(stats.batches, 11);
    assert!(stats.completed);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn interrupt_leaves_generation_unadvanced() {
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
fn rescan_advances_generation_without_duplicates() {
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
fn refused_boundary_paths() {
    use scout::index::walk::refused_at_boundary;
    use std::path::Path;
    assert!(refused_at_boundary(Path::new("/proc/self/status")));
    assert!(refused_at_boundary(Path::new("/sys/kernel")));
    assert!(refused_at_boundary(Path::new("/dev/null")));
    assert!(refused_at_boundary(Path::new("/tmp/evil\nname")));
    assert!(!refused_at_boundary(Path::new("/home/user/project")));
    let long = format!("/tmp/{}", "a".repeat(5000));
    assert!(refused_at_boundary(Path::new(&long)));
}

/// Phase 2 smoke gate: 100k paths < 30 s wall, RSS < 100 MB. Run
/// explicitly at check-in: cargo test --release --test index_walk -- --ignored
#[test]
#[ignore]
fn smoke_100k_paths_under_budget() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("smoke");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    // 1000 dirs x 100 files = 100k files (+1001 dirs).
    for d in 0..1000 {
        let sub = tree.join(format!("dir-{d:04}"));
        fs::create_dir(&sub).unwrap();
        for f in 0..100 {
            fs::write(sub.join(format!("f{f:03}")), b"").unwrap();
        }
    }

    let mut conn = open_db(&dir);
    let started = std::time::Instant::now();
    let stats =
        batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let elapsed = started.elapsed();

    assert!(stats.completed);
    assert!(stats.inserted >= 100_000, "inserted {}", stats.inserted);
    assert!(elapsed.as_secs() < 30, "walk took {elapsed:?}");

    let rss_kb = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines().find(|l| l.starts_with("VmRSS:")).and_then(|l| {
                l.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok())
            })
        })
        .unwrap_or(0);
    assert!(rss_kb < 100 * 1024, "RSS {rss_kb} kB exceeds 100 MB");

    fs::remove_dir_all(&dir).unwrap();
}
