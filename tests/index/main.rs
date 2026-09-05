//! Tests for the index module: open discipline, schema, frecency,
//! recovery, the walker and writer, pacing, and the 100k performance gate
//! (ignored; CI runs it under the release profile).
//!
//! The interrupt flag is process-global. Every test that walks or
//! inserts holds `SERIAL` so a tripped flag cannot leak into a
//! concurrently running test; no other test in this binary touches the
//! flag.

mod frecency;
mod open;
mod pacing;
mod perf;
mod recovery;
mod schema;
mod walk;

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

pub static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-index-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn seed_paths(conn: &rusqlite::Connection, count: usize) {
    let tx = conn.unchecked_transaction().unwrap();
    {
        let mut stmt =
            tx.prepare("INSERT INTO paths (path, scan_generation) VALUES (:path, 1)").unwrap();
        for i in 0..count {
            stmt.execute(rusqlite::named_params! { ":path": format!("/fixture/p{i:06}") }).unwrap();
        }
    }
    tx.commit().unwrap();
}
