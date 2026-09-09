//! `scout index [PATH]`: walk a tree into the index, or every known tree
//! again. A path inside a known root re-walks that root the way it was
//! walked before unless a flag says otherwise; a new path is a new root;
//! a path that would contain a root is refused. `--forget PATH` drops a
//! root, keeping its rows' history for a while.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};

use rusqlite::Connection;

use super::{logging, open_default_db};
use crate::index::roots::{self, Root, WalkFlags};
use crate::index::walk::{walk, WalkConfig};
use crate::index::write::{
    batched_insert_with, InsertStats, MultiWalk, WriteOptions, DEFAULT_BATCH_SIZE,
};
use crate::platform::signals;
use crate::platform::time::unix_now;
use crate::{index, Error};

/// What `scout index` was asked to do with one path.
pub struct WalkRequest {
    pub path: PathBuf,
    pub flags: WalkFlags,
    pub progress: Option<Arc<AtomicU64>>,
}

/// Walk one root on `conn`: resolve the path to its root (creating one),
/// store the flags, walk with them. The path is canonicalised first so
/// the root records exactly the tree walked.
pub fn walk_root(
    conn: &mut Connection,
    request: &WalkRequest,
) -> crate::Result<(Root, InsertStats)> {
    let canonical = std::fs::canonicalize(&request.path).unwrap_or_else(|_| request.path.clone());
    let root = roots::ensure(conn, &canonical, request.flags)?;
    let stats = walk_known_root(conn, &root, request.progress.clone())?;
    Ok((root, stats))
}

/// Walk a root exactly as its stored flags say.
pub fn walk_known_root(
    conn: &mut Connection,
    root: &Root,
    progress: Option<Arc<AtomicU64>>,
) -> crate::Result<InsertStats> {
    let config =
        WalkConfig { root: root.path.clone(), follow_symlinks: root.follow, hidden: root.hidden };
    let options =
        WriteOptions { batch_size: DEFAULT_BATCH_SIZE, root, recon: root.recon, progress };
    batched_insert_with(conn, walk(&config), &options)
        .map_err(|err| Error::io("index run failed", std::io::Error::other(err.to_string())))
}

/// Walk every root again, each the way it was walked before, in the
/// order they were added. Stops after a cancelled walk; roots already
/// completed stay completed. `label` receives "root (i of n)" as each
/// walk starts, for a live display.
pub fn walk_all(
    conn: &mut Connection,
    progress: Option<Arc<AtomicU64>>,
    label: Option<Arc<RwLock<String>>>,
) -> crate::Result<MultiWalk> {
    let roots = roots::list(conn)?;
    if roots.is_empty() {
        return Err(Error::NoRoots);
    }
    let mut report = MultiWalk { walks: Vec::new(), roots_total: roots.len() };
    for (i, root) in roots.iter().enumerate() {
        if let Some(label) = &label {
            if let Ok(mut l) = label.write() {
                *l = format!("{} ({} of {})", root.path.display(), i + 1, roots.len());
            }
        }
        if let Some(progress) = &progress {
            progress.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        let stats = walk_known_root(conn, root, progress.clone())?;
        let completed = stats.completed;
        report.walks.push((root.path.clone(), stats));
        if !completed || signals::interrupt_requested() {
            break;
        }
    }
    Ok(report)
}

fn report_line(root: &Path, stats: &InsertStats, recon: bool) -> String {
    format!(
        "indexed {}: {} paths in {} batches (skipped {}, errors {}); generation {}{}{}",
        root.display(),
        stats.inserted,
        stats.batches,
        stats.skipped,
        stats.errors,
        stats.generation,
        if recon { format!("; {} recon finding(s)", stats.findings) } else { String::new() },
        if stats.completed { "" } else { " INCOMPLETE - prior generation still serves" }
    )
}

/// `scout index` with its arguments already reduced to intent.
pub fn index(path: Option<PathBuf>, flags: WalkFlags, forget: bool) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    signals::install().map_err(Error::Signals)?;
    let mut conn = open_default_db()?;
    let code = match (path, forget) {
        (Some(path), true) => {
            let canonical = std::fs::canonicalize(&path).unwrap_or(path);
            match roots::forget(&conn, &canonical, unix_now())? {
                Some(rows) => {
                    println!("forgot {}: {rows} paths tombstoned", canonical.display());
                    0
                }
                None => {
                    eprintln!(
                        "scout: {} is not a root; `scout doctor` lists them",
                        canonical.display()
                    );
                    1
                }
            }
        }
        (None, true) => {
            return Err(Error::UnknownFormat {
                given: "--forget".into(),
                wanted: "a path: scout index --forget <path>",
            })
        }
        (Some(path), false) => {
            let request = WalkRequest { path, flags, progress: None };
            let (root, stats) = walk_root(&mut conn, &request)?;
            println!("{}", report_line(&root.path, &stats, root.recon));
            u8::from(!stats.completed)
        }
        (None, false) => {
            let roots = roots::list(&conn)?;
            let report = walk_all(&mut conn, None, None)?;
            for (path, stats) in &report.walks {
                let recon = roots.iter().find(|r| r.path == *path).is_none_or(|r| r.recon);
                println!("{}", report_line(path, stats, recon));
            }
            u8::from(!report.all_completed())
        }
    };
    index::recovery::shutdown(conn)
        .map_err(|err| Error::io("shutdown", std::io::Error::other(err.to_string())))?;
    Ok(code)
}
