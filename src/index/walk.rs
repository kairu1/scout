//! The streaming, gitignore-aware, parallel walker. Every path is
//! canonicalised here and passed through the boundary refusals before it
//! enters a bounded channel; the tree is never held in memory.
//!
//! Besides the entries the ignore rules admit, the walker lists every
//! directory it descends for credential-bearing names (`.env`, `id_rsa`,
//! the `.ssh` directory and one level inside it) and sends those too,
//! marked as not candidates: a `.env` that gitignore hides, or that a
//! walk without hidden entries never sees, is exactly the file whose
//! mode recon must look at. The picker never shows such a row.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::platform::signals;
use crate::recon::checks::is_secret_name;

/// One path from the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkItem {
    pub path: PathBuf,
    /// False for a row recorded only so recon can examine it.
    pub candidate: bool,
}

impl WalkItem {
    pub fn candidate(path: PathBuf) -> Self {
        WalkItem { path, candidate: true }
    }
}

/// Trees that must never enter the index.
const SYSTEM_DENYLIST: &[&str] = &["/proc", "/sys", "/dev"];
const MAX_PATH_BYTES: usize = 4096;

pub struct WalkConfig {
    pub root: PathBuf,
    pub follow_symlinks: bool,
    /// Include hidden entries (dotfiles). Excluded by default.
    pub hidden: bool,
}

impl WalkConfig {
    pub fn new(root: PathBuf) -> Self {
        WalkConfig { root, follow_symlinks: false, hidden: false }
    }
}

/// True when `path` must not enter the index: too long, carrying a NUL
/// or newline (the two bytes the print seam cannot quote), or under a
/// system pseudo-filesystem.
pub fn refused_at_boundary(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) || bytes.contains(&b'\n') {
        return true;
    }
    SYSTEM_DENYLIST.iter().any(|deny| path.starts_with(deny))
}

/// Walk `config.root` in parallel, canonicalising every yielded path.
/// Canonicalisation failure skips the path (debug log, never fatal).
/// The iterator ends early if an interrupt is requested.
pub fn walk(config: &WalkConfig) -> impl Iterator<Item = WalkItem> {
    // A bounded channel is what makes this streaming: the walker blocks
    // when the writer falls behind instead of buffering the tree.
    let (tx, rx) = mpsc::sync_channel::<WalkItem>(1024);

    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or_else(|err| {
        tracing::warn!(%err, "available_parallelism failed; walking single-threaded");
        1
    });

    let mut builder = ignore::WalkBuilder::new(&config.root);
    builder.follow_links(config.follow_symlinks).hidden(!config.hidden).threads(threads);
    let walker = builder.build_parallel();

    std::thread::spawn(move || {
        let span = tracing::info_span!("index.walk.start");
        let _guard = span.enter();
        walker.run(|| {
            let tx = tx.clone();
            Box::new(move |entry| {
                if signals::interrupt_requested() {
                    return ignore::WalkState::Quit;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(err) => {
                        tracing::debug!(%err, "walk entry error; skipping");
                        return ignore::WalkState::Continue;
                    }
                };
                let canonical = match std::fs::canonicalize(entry.path()) {
                    Ok(path) => path,
                    Err(err) => {
                        tracing::debug!(path = %entry.path().display(), %err,
                            "canonicalisation failed; skipping");
                        return ignore::WalkState::Continue;
                    }
                };
                if refused_at_boundary(&canonical) {
                    tracing::debug!(path = %canonical.display(), "refused at boundary");
                    return ignore::WalkState::Continue;
                }
                let is_dir = entry.file_type().is_some_and(|t| t.is_dir());
                if tx.send(WalkItem::candidate(canonical)).is_err() {
                    // Receiver dropped: the consumer is gone, stop walking.
                    return ignore::WalkState::Quit;
                }
                if is_dir {
                    for secret in secret_entries(entry.path()) {
                        if tx.send(WalkItem { path: secret, candidate: false }).is_err() {
                            return ignore::WalkState::Quit;
                        }
                    }
                }
                ignore::WalkState::Continue
            })
        });
        tracing::info!("index.walk.complete");
    });

    rx.into_iter()
}

/// Credential-bearing entries directly inside `dir`, canonicalised and
/// boundary-checked, plus one level inside a `.ssh` directory. Symlinks
/// are skipped: a link named `.env` is the link's business, and the
/// symlink check reports where it leads.
fn secret_entries(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name == ".ssh" && kind.is_dir() {
            if let Some(canonical) = admitted(&entry.path()) {
                out.push(canonical);
            }
            if let Ok(inner) = std::fs::read_dir(entry.path()) {
                for child in inner.flatten() {
                    let is_link = child.file_type().is_ok_and(|t| t.is_symlink());
                    let child_name = child.file_name();
                    let Some(child_name) = child_name.to_str() else { continue };
                    if !is_link && is_secret_name(child_name) {
                        if let Some(canonical) = admitted(&child.path()) {
                            out.push(canonical);
                        }
                    }
                }
            }
        } else if is_secret_name(name) {
            if let Some(canonical) = admitted(&entry.path()) {
                out.push(canonical);
            }
        }
    }
    out
}

fn admitted(path: &Path) -> Option<PathBuf> {
    let canonical = std::fs::canonicalize(path).ok()?;
    if refused_at_boundary(&canonical) {
        return None;
    }
    Some(canonical)
}
