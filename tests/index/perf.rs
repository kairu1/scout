//! The performance gate: 100k paths in under 30 s wall with under 100 MB
//! resident. Ignored by default; CI runs it as its own job under the
//! release profile: `cargo test --release --test index -- --ignored`.

use std::fs;

use scout::index::walk::{walk, WalkConfig};
use scout::index::write::{batched_insert, batched_insert_with, WriteOptions};
use scout::platform::signals;

use crate::walk::open_db;
use crate::{serial, temp_dir};

fn plant_100k(tree: &std::path::Path) {
    fs::create_dir(tree).unwrap();
    // 1000 dirs x 100 files = 100k files (+1001 dirs).
    for d in 0..1000 {
        let sub = tree.join(format!("dir-{d:04}"));
        fs::create_dir(&sub).unwrap();
        for f in 0..100 {
            fs::write(sub.join(format!("f{f:03}")), b"").unwrap();
        }
    }
}

/// The same walk with the stat-only recon checks running per path. The
/// hard budget is the walk's own 30 s; the ratio to the plain walk is
/// printed and recorded, not asserted: an `lstat` per path is a real cost
/// against a walk that finishes 100k paths in half a second, which is why
/// `--recon` at index time is opt-in.
#[test]
#[ignore]
fn one_hundred_thousand_paths_index_with_recon_under_budget() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("smoke-recon");
    let tree = dir.join("tree");
    plant_100k(&tree);

    let mut conn = open_db(&dir);
    let plain = std::time::Instant::now();
    let stats = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let plain = plain.elapsed();
    assert!(stats.completed);

    let with_recon = std::time::Instant::now();
    let options = WriteOptions { batch_size: 1000, root: Some(&tree), recon: true };
    let stats =
        batched_insert_with(&mut conn, walk(&WalkConfig::new(tree.clone())), &options).unwrap();
    let with_recon = with_recon.elapsed();
    assert!(stats.completed);
    assert!(with_recon.as_secs() < 30, "walk with recon took {with_recon:?}");
    println!(
        "100k paths: plain {plain:?}, with recon {with_recon:?} ({:.1}x)",
        with_recon.as_secs_f64() / plain.as_secs_f64().max(0.001)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[ignore]
fn one_hundred_thousand_paths_index_under_budget() {
    let _serial = serial();
    signals::reset_interrupt();
    let dir = temp_dir("smoke");
    let tree = dir.join("tree");
    plant_100k(&tree);

    let mut conn = open_db(&dir);
    let started = std::time::Instant::now();
    let stats = batched_insert(&mut conn, walk(&WalkConfig::new(tree.clone())), 1000).unwrap();
    let elapsed = started.elapsed();

    assert!(stats.completed);
    assert!(stats.inserted >= 100_000, "inserted {}", stats.inserted);
    assert!(elapsed.as_secs() < 30, "walk took {elapsed:?}");

    let rss_kb = fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmRSS:"))
                .and_then(|l| l.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()))
        })
        .unwrap_or(0);
    assert!(rss_kb < 100 * 1024, "RSS {rss_kb} kB exceeds 100 MB");
    println!("100k paths: {elapsed:?}, RSS {rss_kb} kB");

    fs::remove_dir_all(&dir).unwrap();
}
