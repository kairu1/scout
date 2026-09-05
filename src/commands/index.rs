//! `scout index <root>`: walk a tree into the index.

use std::path::PathBuf;

use super::{logging, open_default_db};
use crate::index::walk::{walk, WalkConfig};
use crate::index::write::{batched_insert_with, WriteOptions, DEFAULT_BATCH_SIZE};
use crate::platform::signals;
use crate::{index, Error};

pub fn index(root: PathBuf, hidden: bool, follow: bool, recon: bool) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    signals::install().map_err(Error::Signals)?;
    let mut conn = open_default_db()?;
    // Record the tree as given after canonicalising, so a session can
    // re-run exactly this walk.
    let root = std::fs::canonicalize(&root).unwrap_or(root);
    let config = WalkConfig { root: root.clone(), follow_symlinks: follow, hidden };
    let options = WriteOptions { batch_size: DEFAULT_BATCH_SIZE, root: Some(&root), recon };
    let stats = batched_insert_with(&mut conn, walk(&config), &options)
        .map_err(|err| Error::io("index run failed", std::io::Error::other(err.to_string())))?;
    index::recovery::shutdown(conn)
        .map_err(|err| Error::io("shutdown", std::io::Error::other(err.to_string())))?;
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
