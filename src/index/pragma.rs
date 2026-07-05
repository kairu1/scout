//! DB open path: O_NOFOLLOW refusal, permission discipline (ADR-003 §4),
//! WAL + PRAGMA configuration (ADR-001 §Index-writer pacing).

use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Path;

use rusqlite::Connection;

use super::schema::apply_migrations;
use super::{IndexError, Result};

// Not on the ADR-002 roster: `libc`. The single flag we need is stable
// kernel ABI per OS/arch; hand-pinning it is the ~200-line-rule answer.
// Linux x86 family defines its own value; every other Linux arch uses
// the asm-generic one.
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "x86")))]
const O_NOFOLLOW: i32 = 0o400000;
#[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "x86"))))]
const O_NOFOLLOW: i32 = 0o100000;
#[cfg(target_os = "macos")]
const O_NOFOLLOW: i32 = 0x0100;

/// Open (creating if absent) the index database at `path`, enforce
/// ADR-003 §4 file discipline, configure ADR-001 PRAGMAs, and run
/// migrations. Returns the configured connection.
pub fn open(path: &Path) -> Result<Connection> {
    let parent = path.parent().ok_or_else(|| {
        IndexError::Refused(format!("db path has no parent directory: {}", path.display()))
    })?;

    // Parent created 0700 iff SCOUT creates it; pre-existing dirs are the
    // user's call (ADR-003 §4).
    if !parent.exists() {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
    }

    // Refuse a symlinked final component before recovery ever touches it.
    if path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        return Err(IndexError::Refused(format!(
            "db path is a symlink (O_NOFOLLOW): {}",
            path.display()
        )));
    }

    // Crash recovery: sentinel consumption, integrity check on a
    // suspicious open, rename-and-rebuild on corruption (Surgeon §4).
    super::recovery::startup_check(path)?;

    // O_NOFOLLOW pre-check on the final component; explicit 0600 at first
    // creation, umask-agnostic via set_permissions.
    let existed = path.symlink_metadata().is_ok();
    let open_result = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(O_NOFOLLOW)
        .open(path);
    match open_result {
        Ok(_file) => {
            if !existed {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
        }
        Err(err) => {
            if path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                return Err(IndexError::Refused(format!(
                    "db path is a symlink (O_NOFOLLOW): {}",
                    path.display()
                )));
            }
            return Err(err.into());
        }
    }

    // Owner check: the file must belong to the invoking UID. No `libc`
    // geteuid on the roster — a probe file we just created carries our
    // euid by definition.
    let db_meta = fs::metadata(path)?;
    let our_uid = probe_uid(parent)?;
    if db_meta.uid() != our_uid {
        return Err(IndexError::Refused(format!(
            "db owner uid {} != invoking uid {} at {}",
            db_meta.uid(),
            our_uid,
            path.display()
        )));
    }
    if db_meta.mode() & 0o077 != 0 {
        tracing::warn!(
            mode = format!("{:o}", db_meta.mode() & 0o777),
            path = %path.display(),
            "index db mode grants group/other access; recommend chmod 600"
        );
        eprintln!(
            "scout: warning: {} mode {:o} grants group/other access; recommend chmod 600",
            path.display(),
            db_meta.mode() & 0o777
        );
    }

    // WAL/SHM siblings: guarantee 0600 initial state regardless of umask
    // by creating them before SQLite does (ADR-003 §4).
    for suffix in ["-wal", "-shm"] {
        let sibling = sibling_path(path, suffix);
        if sibling.symlink_metadata().is_err() {
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&sibling)?;
            fs::set_permissions(&sibling, fs::Permissions::from_mode(0o600))?;
        }
    }

    let conn = Connection::open(path)?;
    ensure_exp_function(&conn)?;

    // ADR-001 §Index-writer pacing: WAL, synchronous=NORMAL, explicit
    // checkpointing only. journal_mode and wal_autocheckpoint return a
    // row; read it back rather than fighting the statement API.
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(IndexError::Refused(format!("journal_mode = WAL not honoured (got {mode})")));
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    let _autocheckpoint: i64 =
        conn.query_row("PRAGMA wal_autocheckpoint = 0", [], |r| r.get(0))?;

    apply_migrations(&conn)?;
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

/// `exp` scalar for the frecency update (ADR-001). The bundled SQLite may
/// or may not carry math built-ins; register ours only when absent.
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

fn probe_uid(dir: &Path) -> Result<u32> {
    let probe = dir.join(format!(".scout-uid-probe-{}", std::process::id()));
    fs::OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).open(&probe)?;
    let uid = fs::metadata(&probe)?.uid();
    let _ = fs::remove_file(&probe);
    Ok(uid)
}
