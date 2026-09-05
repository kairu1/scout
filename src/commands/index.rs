//! `scout index <root>`: walk a tree into the index.

use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;

use rusqlite::Connection;

use super::{logging, open_default_db};
use crate::index::walk::{walk, WalkConfig};
use crate::index::write::{batched_insert_with, InsertStats, WriteOptions, DEFAULT_BATCH_SIZE};
use crate::platform::signals;
use crate::{index, Error};

/// How a walk is run, shared by the CLI and a session's re-index.
pub struct WalkRequest {
    pub root: PathBuf,
    pub hidden: bool,
    pub follow: bool,
    pub recon: bool,
    pub progress: Option<Arc<AtomicU64>>,
}

/// Walk `request.root` into `conn`, close the connection cleanly, and
/// report. The root is canonicalised first so `last_root` records exactly
/// the tree walked.
pub fn run_walk(mut conn: Connection, request: &WalkRequest) -> crate::Result<InsertStats> {
    let root = std::fs::canonicalize(&request.root).unwrap_or_else(|_| request.root.clone());
    let config =
        WalkConfig { root: root.clone(), follow_symlinks: request.follow, hidden: request.hidden };
    let options = WriteOptions {
        batch_size: DEFAULT_BATCH_SIZE,
        root: Some(root.as_path()),
        recon: request.recon,
        progress: request.progress.clone(),
    };
    let stats = batched_insert_with(&mut conn, walk(&config), &options)
        .map_err(|err| Error::io("index run failed", std::io::Error::other(err.to_string())))?;
    index::recovery::shutdown(conn)
        .map_err(|err| Error::io("shutdown", std::io::Error::other(err.to_string())))?;
    Ok(stats)
}

/// The tree the last completed `scout index` walked, if any.
pub fn last_root(conn: &Connection) -> Option<PathBuf> {
    conn.query_row("SELECT last_root FROM run_state WHERE id = 1", [], |r| {
        r.get::<_, Option<String>>(0)
    })
    .ok()
    .flatten()
    .map(PathBuf::from)
}

pub fn index(root: PathBuf, hidden: bool, follow: bool, recon: bool) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    signals::install().map_err(Error::Signals)?;
    let conn = open_default_db()?;
    let request = WalkRequest { root, hidden, follow, recon, progress: None };
    let stats = run_walk(conn, &request)?;
    println!(
        "indexed {} paths in {} batches (skipped {}, errors {}); generation {}{}{}",
        stats.inserted,
        stats.batches,
        stats.skipped,
        stats.errors,
        stats.generation,
        if recon { format!("; {} recon finding(s)", stats.findings) } else { String::new() },
        if stats.completed { "" } else { " INCOMPLETE - prior generation still serves" }
    );
    Ok(if stats.completed { 0 } else { 1 })
}
