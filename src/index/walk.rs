//! Streaming, gitignore-aware parallel walker (`ignore` crate, ADR-002
//! slot 3; admitted for Phase 2 per OPORD §Doctrinal note). Paths are
//! canonicalised at this boundary (ADR-003 §1) and stream through a
//! bounded channel — no in-memory buffering of the tree (Surgeon §2).

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use super::signals;

/// ADR-003 §1 refusals at the indexer boundary.
const SYSTEM_DENYLIST: &[&str] = &["/proc", "/sys", "/dev"];
const MAX_PATH_BYTES: usize = 4096;

pub struct WalkConfig {
    pub root: PathBuf,
    pub follow_symlinks: bool,
    /// Include hidden entries (dotfiles). Default posture: excluded.
    pub hidden: bool,
}

impl WalkConfig {
    pub fn new(root: PathBuf) -> Self {
        WalkConfig { root, follow_symlinks: false, hidden: false }
    }
}

/// True when `path` must not enter the index (ADR-003 §1).
pub fn refused_at_boundary(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    if bytes.len() > MAX_PATH_BYTES || bytes.contains(&0) || bytes.contains(&b'\n') {
        return true;
    }
    SYSTEM_DENYLIST.iter().any(|deny| path.starts_with(deny))
}

/// Walk `config.root` in parallel, canonicalising every yielded path.
/// Canonicalisation failure skips the path (debug log, never fatal —
/// Surgeon §1a). The iterator ends early if an interrupt is requested.
pub fn walk(config: &WalkConfig) -> impl Iterator<Item = PathBuf> {
    // Bounded channel = backpressure = streaming memory profile.
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
                    // Receiver dropped: consumer is gone, stop walking.
                    Err(_) => ignore::WalkState::Quit,
                }
            })
        });
        tracing::info!("index.walk.complete");
    });

    rx.into_iter()
}
