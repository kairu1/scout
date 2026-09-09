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
    let stats =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();

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
    let stats =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 100).unwrap();

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
    let stats = batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 10).unwrap();
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
    let first =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(first.generation, 1);
    let rows_after_first = row_count(&conn);

    let second =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
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
    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let id: i64 = conn
        .query_row(
            "SELECT rowid FROM paths WHERE path = :p",
            rusqlite::named_params! { ":p": victim.to_str().unwrap() },
            |r| r.get(0),
        )
        .unwrap();
    assert!(scout::index::frecency::record_visit(&conn, id, 1_800_000_000).unwrap());

    fs::remove_file(&victim).unwrap();
    let second =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
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
    let third =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
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
    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let second =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
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
    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
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

    let stats =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
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

/// Walking a second tree keeps the first: each root has its own current
/// generation, and a reader serves each row against its own root's.
/// Before roots existed, the second walk silently retired the first tree.
#[test]
fn two_roots_are_both_served_and_walking_one_leaves_the_other_alone() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("two-roots");
    let (a, b) = (dir.join("a"), dir.join("b"));
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    make_files(&a, 3);
    make_files(&b, 2);

    let mut conn = open_db(&dir);
    let first = batched_insert(&mut conn, &a, walk(&WalkConfig::new(a.clone())), 1000).unwrap();
    let second = batched_insert(&mut conn, &b, walk(&WalkConfig::new(b.clone())), 1000).unwrap();
    assert_eq!((first.generation, second.generation), (1, 2), "one counter across all walks");
    assert_eq!(second.tombstoned, 0, "walking b must not tombstone a's rows");

    let served = |conn: &Connection| -> Vec<String> {
        let mut v: Vec<String> =
            scout::search::load_candidates(conn).unwrap().into_iter().map(|c| c.path).collect();
        v.sort();
        v
    };
    let live = served(&conn);
    assert_eq!(live.len(), 4 + 3, "a: root + 3 files, b: root + 2 files");
    assert!(live.iter().any(|p| p.ends_with("/a/file-00000.txt")));
    assert!(live.iter().any(|p| p.ends_with("/b/file-00001.txt")));

    // Re-walk a after a file vanished: only a's row is tombstoned, b is untouched.
    fs::remove_file(a.join("file-00002.txt")).unwrap();
    let third = batched_insert(&mut conn, &a, walk(&WalkConfig::new(a.clone())), 1000).unwrap();
    assert_eq!(third.generation, 3);
    assert_eq!(third.tombstoned, 1);
    let live = served(&conn);
    assert_eq!(live.len(), 6);
    assert!(
        live.iter().any(|p| p.ends_with("/b/file-00001.txt")),
        "b still serves at generation 2"
    );
    let roots = scout::index::roots::list(&conn).unwrap();
    let gens: Vec<i64> = roots.iter().map(|r| r.current_generation).collect();
    assert_eq!(gens, vec![3, 2]);

    fs::remove_dir_all(&dir).unwrap();
}

/// A path inside a root means that root; a path that would swallow a
/// root is refused with the root named; a sibling is a new root.
#[test]
fn a_path_resolves_to_its_root_and_nesting_is_refused() {
    use scout::index::roots::{ensure, resolve, Resolved, WalkFlags};
    let _serial = serial();
    let dir = temp_dir("resolve");
    let work = dir.join("work");
    fs::create_dir_all(work.join("api/src")).unwrap();
    fs::create_dir_all(dir.join("other")).unwrap();
    let conn = open_db(&dir);
    let root = ensure(&conn, &work, WalkFlags::default()).unwrap();
    assert_eq!(root.path, work);

    match resolve(&conn, &work.join("api/src")).unwrap() {
        Resolved::Existing(r) => assert_eq!(r.id, root.id),
        other => panic!("inside a root must resolve to it: {other:?}"),
    }
    match resolve(&conn, &dir.join("other")).unwrap() {
        Resolved::New(p) => assert_eq!(p, dir.join("other")),
        other => panic!("a sibling is a new root: {other:?}"),
    }
    match resolve(&conn, &dir) {
        Err(scout::Error::NestedRoot { given, existing }) => {
            assert_eq!((given, existing), (dir.clone(), work.clone()));
        }
        other => panic!("a parent of a root must be refused: {other:?}"),
    }
    fs::remove_dir_all(&dir).unwrap();
}

/// Flags given are stored on the root; flags not given are read back
/// from it, so "index again" means "the same way".
#[test]
fn walk_flags_are_stored_on_the_root_and_reused_when_absent() {
    use scout::index::roots::{ensure, WalkFlags};
    let _serial = serial();
    let dir = temp_dir("flags");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    let conn = open_db(&dir);

    let fresh = ensure(&conn, &tree, WalkFlags::default()).unwrap();
    assert_eq!((fresh.hidden, fresh.follow, fresh.recon), (false, false, true), "defaults");

    let set =
        ensure(&conn, &tree, WalkFlags { hidden: Some(true), follow: None, recon: Some(false) })
            .unwrap();
    assert_eq!((set.hidden, set.follow, set.recon), (true, false, false));

    let again = ensure(&conn, &tree, WalkFlags::default()).unwrap();
    assert_eq!((again.hidden, again.follow, again.recon), (true, false, false), "reused");

    let off =
        ensure(&conn, &tree, WalkFlags { hidden: Some(false), ..WalkFlags::default() }).unwrap();
    assert!(!off.hidden, "an explicit flag turns a stored one off");
    fs::remove_dir_all(&dir).unwrap();
}

/// Forgetting a root hides its rows at once and keeps their history; a
/// later walk of the same tree revives them.
#[test]
fn a_forgotten_root_stops_serving_and_a_later_walk_revives_its_history() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("forget");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 2);
    let mut conn = open_db(&dir);
    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let id: i64 = conn
        .query_row(
            "SELECT rowid FROM paths WHERE path = :p",
            rusqlite::named_params! { ":p": tree.join("file-00000.txt").to_str().unwrap() },
            |r| r.get(0),
        )
        .unwrap();
    assert!(scout::index::frecency::record_visit(&conn, id, 1_800_000_000).unwrap());

    let now = scout::platform::time::unix_now();
    assert_eq!(scout::index::roots::forget(&conn, &tree, now).unwrap(), Some(3));
    assert!(scout::search::load_candidates(&conn).unwrap().is_empty());
    assert!(scout::index::roots::list(&conn).unwrap().is_empty());
    assert_eq!(scout::index::roots::forget(&conn, &tree, now).unwrap(), None, "already gone");

    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let (tomb, visits): (Option<i64>, i64) = conn
        .query_row(
            "SELECT tombstoned_at, visits_total FROM paths WHERE rowid = :id",
            rusqlite::named_params! { ":id": id },
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((tomb, visits), (None, 1), "revived with history");
    assert_eq!(scout::search::load_candidates(&conn).unwrap().len(), 3);
    fs::remove_dir_all(&dir).unwrap();
}

/// An interrupted re-walk of a root leaves the previous index serving:
/// rows the partial walk rewrote are still there on disk and stay
/// served; a row only the partial walk saw is retired by the next
/// completed walk rather than lingering as a ghost.
#[test]
fn an_interrupted_rewalk_keeps_the_previous_rows_serving() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("interrupted-rewalk");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 50);
    let mut conn = open_db(&dir);
    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert_eq!(scout::search::load_candidates(&conn).unwrap().len(), 51);

    // A new file appears, then a re-walk is cut off after the first batch.
    fs::write(tree.join("ghost.txt"), b"x").unwrap();
    let interrupt_after_first_batch =
        walk(&WalkConfig::new(tree.clone())).enumerate().map(|(i, item)| {
            if i == 9 {
                signals::request_interrupt();
            }
            item
        });
    let partial = batched_insert(&mut conn, &tree, interrupt_after_first_batch, 10).unwrap();
    signals::reset_interrupt();
    assert!(!partial.completed);
    let served = scout::search::load_candidates(&conn).unwrap().len();
    assert!(
        (51..=52).contains(&served),
        "every previous row is still served (plus the ghost if the partial walk saw it): {served}"
    );

    // The ghost vanishes; the next completed walk retires it, and its
    // generation number is past the partial walk's, never reused.
    fs::remove_file(tree.join("ghost.txt")).unwrap();
    let third =
        batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert!(third.completed);
    assert!(third.generation > partial.generation, "a partial walk's number is not reused");
    let live: Vec<String> =
        scout::search::load_candidates(&conn).unwrap().into_iter().map(|c| c.path).collect();
    assert_eq!(live.len(), 51);
    assert!(!live.iter().any(|p| p.ends_with("ghost.txt")), "the ghost is retired");
    fs::remove_dir_all(&dir).unwrap();
}

/// `/a/b` is not inside `/a/bc`: the nesting rule works on path
/// components, not on string prefixes.
#[test]
fn a_sibling_whose_name_extends_the_root_is_not_inside_it() {
    use scout::index::roots::{ensure, resolve, Resolved, WalkFlags};
    let _serial = serial();
    let dir = temp_dir("boundary");
    fs::create_dir_all(dir.join("work")).unwrap();
    fs::create_dir_all(dir.join("workspace")).unwrap();
    fs::create_dir_all(dir.join("wo")).unwrap();
    let conn = open_db(&dir);
    ensure(&conn, &dir.join("work"), WalkFlags::default()).unwrap();
    assert!(matches!(resolve(&conn, &dir.join("workspace")).unwrap(), Resolved::New(_)));
    assert!(matches!(resolve(&conn, &dir.join("wo")).unwrap(), Resolved::New(_)));
    assert!(matches!(resolve(&conn, &dir.join("work/sub")).unwrap(), Resolved::Existing(_)));
    fs::remove_dir_all(&dir).unwrap();
}

/// A forgotten root's rows are gone from every reader, and an index with
/// no completed root reads as empty rather than as ready with nothing.
#[test]
fn an_index_without_a_completed_root_is_empty_not_ready() {
    use scout::search::{index_state, IndexState};
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("rootless");
    let tree = dir.join("tree");
    fs::create_dir(&tree).unwrap();
    make_files(&tree, 2);
    let mut conn = open_db(&dir);
    assert_eq!(index_state(&conn).unwrap(), IndexState::Empty);
    batched_insert(&mut conn, &tree, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    assert!(matches!(index_state(&conn).unwrap(), IndexState::Ready { candidates: 3, .. }));
    let now = scout::platform::time::unix_now();
    scout::index::roots::forget(&conn, &tree, now).unwrap();
    assert_eq!(index_state(&conn).unwrap(), IndexState::Empty, "rows without a root are not ready");
    fs::remove_dir_all(&dir).unwrap();
}

/// With no roots, walking everything again is refused by variant, so a
/// caller can act on it without reading prose.
#[test]
fn walking_every_root_with_none_is_refused_by_variant() {
    let _serial = serial();
    let dir = temp_dir("no-roots");
    let mut conn = open_db(&dir);
    assert!(matches!(
        scout::commands::index::walk_all(&mut conn, None, None),
        Err(scout::Error::NoRoots)
    ));
    fs::remove_dir_all(&dir).unwrap();
}
