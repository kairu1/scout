//! Opening the index: refuse a symlinked path, create the file 0600 under
//! a 0700 directory, check the owner, pre-create the WAL siblings with
//! the same mode, configure WAL with explicit checkpointing only, and run
//! migrations. This is the path that creates, migrates and recovers;
//! `doctor` deliberately does not use it.

use std::path::Path;

use rusqlite::Connection;

use crate::platform::fs as pfs;
use crate::{Error, Result};

/// Open (creating if absent) the index database at `path` and return a
/// configured connection.
pub fn open(path: &Path) -> Result<Connection> {
    let parent = path.parent().ok_or_else(|| {
        Error::IndexRefused(format!("db path has no parent directory: {}", path.display()))
    })?;

    // The parent is created 0700 only when scout creates it; a
    // pre-existing directory's mode is the user's call.
    pfs::create_private_dir(parent)?;

    // Refuse a symlinked final component before recovery ever touches it.
    if pfs::is_symlink(path) {
        return Err(Error::IndexRefused(format!(
            "db path is a symlink (O_NOFOLLOW): {}",
            path.display()
        )));
    }

    // Crash recovery: consume the clean-shutdown sentinel, integrity-check
    // a suspicious open, rename aside and rebuild on corruption.
    super::recovery::startup_check(path)?;

    match pfs::open_or_create_private(path) {
        Ok(_) => {}
        Err(err) => {
            if pfs::is_symlink(path) {
                return Err(Error::IndexRefused(format!(
                    "db path is a symlink (O_NOFOLLOW): {}",
                    path.display()
                )));
            }
            return Err(err.into());
        }
    }

    // The file must belong to the invoking user.
    let db_uid = pfs::owner_uid(path)?;
    let our_uid = pfs::euid();
    if db_uid != our_uid {
        return Err(Error::IndexRefused(format!(
            "db owner uid {db_uid} != invoking uid {our_uid} at {}",
            path.display()
        )));
    }
    let mode = pfs::mode_bits(path)?;
    if mode & 0o077 != 0 {
        tracing::warn!(
            mode = format!("{:o}", mode & 0o777),
            path = %path.display(),
            "index db mode grants group/other access; recommend chmod 600"
        );
        eprintln!(
            "scout: warning: {} mode {:o} grants group/other access; recommend chmod 600",
            path.display(),
            mode & 0o777
        );
    }

    // WAL/SHM siblings: create them 0600 before SQLite does, so their
    // mode never depends on the umask.
    for suffix in ["-wal", "-shm"] {
        pfs::ensure_private_file(&sibling_path(path, suffix))?;
    }

    let conn = Connection::open(path)?;
    ensure_exp_function(&conn)?;

    // WAL, synchronous=NORMAL, explicit checkpointing only. journal_mode
    // and wal_autocheckpoint return a row; read it back.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(Error::IndexRefused(format!("journal_mode = WAL not honoured (got {mode})")));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let _autocheckpoint: i64 = conn.query_row("PRAGMA wal_autocheckpoint = 0", [], |r| r.get(0))?;

    super::schema::apply_migrations(&conn)?;
    Ok(conn)
}

/// Read back the three configured PRAGMAs: (journal_mode, synchronous,
/// wal_autocheckpoint).
pub fn pragma_state(conn: &Connection) -> Result<(String, i64, i64)> {
    let journal: String = conn.query_row("PRAGMA journal_mode", [], |r| r.get(0))?;
    let synchronous: i64 = conn.query_row("PRAGMA synchronous", [], |r| r.get(0))?;
    let autocheckpoint: i64 = conn.query_row("PRAGMA wal_autocheckpoint", [], |r| r.get(0))?;
    Ok((journal, synchronous, autocheckpoint))
}

/// `exp` for the frecency update. The bundled SQLite may or may not carry
/// math built-ins; register ours only when absent.
fn ensure_exp_function(conn: &Connection) -> Result<()> {
    if conn.query_row("SELECT exp(1.0)", [], |r| r.get::<_, f64>(0)).is_ok() {
        return Ok(());
    }
    use rusqlite::functions::FunctionFlags;
    conn.create_scalar_function(
        "exp",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let x: f64 = ctx.get(0)?;
            Ok(x.exp())
        },
    )?;
    Ok(())
}

fn sibling_path(path: &Path, suffix: &str) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    std::path::PathBuf::from(s)
}
