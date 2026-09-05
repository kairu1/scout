//! The streaming, gitignore-aware, parallel walker. Every path is
//! canonicalised here and passed through the boundary refusals before it
//! enters a bounded channel; the tree is never held in memory.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::platform::signals;

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
pub fn walk(config: &WalkConfig) -> impl Iterator<Item = PathBuf> {
    // A bounded channel is what makes this streaming: the walker blocks
    // when the writer falls behind instead of buffering the tree.
    let (tx, rx) = mpsc::sync_channel::<PathBuf>(1024);

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
                match tx.send(canonical) {
                    Ok(()) => ignore::WalkState::Continue,
                    // Receiver dropped: the consumer is gone, stop walking.
                    Err(_) => ignore::WalkState::Quit,
                }
            })
        });
        tracing::info!("index.walk.complete");
    });

    rx.into_iter()
}
