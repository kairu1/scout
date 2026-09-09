//! Crash recovery: a clean-shutdown sentinel, an integrity check when the
//! sentinel is missing, and rename-aside-and-rebuild when the check fails.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::platform::time::unix_now;
use crate::{Error, Result};

/// Soft budget for the recovery-path integrity check; a miss is logged,
/// not fatal.
const INTEGRITY_BUDGET: Duration = Duration::from_secs(2);
/// Hard ceiling: the check is interrupted and the database treated as
/// corrupt.
const INTEGRITY_HARD_FAIL: Duration = Duration::from_secs(10);

pub fn sentinel_path(db_path: &Path) -> PathBuf {
    let mut s = db_path.as_os_str().to_os_string();
    s.push(".clean");
    PathBuf::from(s)
}

/// Called before the database is opened. Consumes the sentinel; a missing
/// sentinel over an existing database triggers the integrity check; a
/// failed check renames the database aside so a fresh one is built.
pub(crate) fn startup_check(db_path: &Path) -> Result<()> {
    let sentinel = sentinel_path(db_path);
    let db_exists = db_path.symlink_metadata().map(|m| m.is_file() && m.len() > 0).unwrap_or(false);

    if sentinel.exists() {
        // Clean shutdown last time; consume the sentinel so a crash
        // before the next shutdown is detectable.
        std::fs::remove_file(&sentinel)?;
        return Ok(());
    }
    if !db_exists {
        return Ok(());
    }

    let span = tracing::info_span!("index.recovery.integrity_check");
    let _guard = span.enter();
    let started = Instant::now();
    let healthy = integrity_check_with_watchdog(db_path);
    let elapsed = started.elapsed();
    tracing::info!(elapsed_ms = elapsed.as_millis() as u64, healthy, "integrity check done");
    if elapsed > INTEGRITY_BUDGET {
        tracing::warn!(
            elapsed_ms = elapsed.as_millis() as u64,
            "integrity check exceeded 2 s budget"
        );
    }

    if healthy {
        return Ok(());
    }

    let epoch = unix_now();
    let target = corrupt_target(db_path, epoch, "");
    std::fs::rename(db_path, &target)?;
    for suffix in ["-wal", "-shm"] {
        let mut sibling = db_path.as_os_str().to_os_string();
        sibling.push(suffix);
        let sibling = PathBuf::from(sibling);
        if sibling.exists() {
            let _ = std::fs::rename(&sibling, corrupt_target(db_path, epoch, suffix));
        }
    }
    tracing::error!(
        renamed_to = %target.display(),
        "index.recovery.corrupt_renamed - rebuilding fresh index"
    );
    eprintln!("scout: index failed integrity check; moved to {} and rebuilding", target.display());
    Ok(())
}

fn corrupt_target(db_path: &Path, epoch: i64, suffix: &str) -> PathBuf {
    let mut s = db_path.as_os_str().to_os_string();
    s.push(format!(".corrupt-{epoch}{suffix}"));
    PathBuf::from(s)
}

/// True iff `PRAGMA integrity_check` reports "ok" within the hard
/// ceiling. A hang is interrupted via SQLite's interrupt handle and
/// treated as failure.
fn integrity_check_with_watchdog(db_path: &Path) -> bool {
    let conn = match Connection::open(db_path) {
        Ok(conn) => conn,
        Err(err) => {
            tracing::error!(%err, "could not open db for integrity check");
            return false;
        }
    };
    let handle = conn.get_interrupt_handle();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let watchdog = std::thread::spawn(move || {
        if done_rx.recv_timeout(INTEGRITY_HARD_FAIL).is_err() {
            handle.interrupt();
        }
    });
    let verdict: std::result::Result<String, rusqlite::Error> =
        conn.query_row("PRAGMA integrity_check", [], |row| row.get(0));
    let _ = done_tx.send(());
    let _ = watchdog.join();
    match verdict {
        Ok(first_row) => first_row.eq_ignore_ascii_case("ok"),
        Err(err) => {
            tracing::error!(%err, "integrity check errored or was interrupted");
            false
        }
    }
}

/// Close a secondary connection: checkpoint and close, no sentinel. For
/// a writer opened beside a connection that is still open, whose own
/// shutdown will write the sentinel.
pub fn close_writer(conn: Connection) -> Result<()> {
    conn.query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |_| Ok(()))?;
    conn.close().map_err(|(_, err)| Error::Sqlite(err))?;
    Ok(())
}

/// Clean shutdown: TRUNCATE-checkpoint the WAL, close, then write the
/// sentinel. Callers own invoking this before process exit.
pub fn shutdown(conn: Connection) -> Result<()> {
    let db_path = conn.path().map(PathBuf::from).ok_or_else(|| {
        Error::IndexRefused("shutdown on a pathless (in-memory) connection".into())
    })?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    conn.close().map_err(|(_, err)| Error::Sqlite(err))?;
    std::fs::write(sentinel_path(&db_path), b"clean\n")?;
    Ok(())
}
