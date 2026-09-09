//! Findings, exceptions and the baseline in the index, and the
//! `worst_finding` summary column the picker reads.
//!
//! Two writers change findings: a recon run and `accept`/`revoke`. Both
//! recompute `worst_finding` for exactly the rows they touched, so the
//! column always equals "max severity among findings with no matching
//! exception" for every row; a test asserts that equality.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

use super::checks::{Check, Finding, Severity, ENTRY_POINTS};
use crate::platform::hash::hex_digest;
use crate::Result;

/// A stored finding, as read back for reports and the picker footer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFinding {
    pub path_id: i64,
    pub path: String,
    pub check: Check,
    pub severity: Severity,
    pub detail: String,
    pub fact: String,
    pub first_seen: i64,
    pub last_seen: i64,
    /// `Some(reason)` when an exception with the same fact covers it.
    pub accepted: Option<String>,
}

/// Replace the findings for one path row with `found`, keeping
/// `first_seen` for findings that persist, and recompute the row's
/// summary. Checks listed in `evaluated` that produced nothing are
/// cleared; checks not evaluated (ACL, baseline, on an index-time run)
/// are left alone.
pub fn store_for_path(
    conn: &Connection,
    path_id: i64,
    found: &[Finding],
    evaluated: &[Check],
    now: i64,
) -> Result<()> {
    // The common case by far: a clean path with no stored findings. One
    // indexed point lookup, then nothing; without this, every clean path
    // paid one DELETE per evaluated check plus a summary UPDATE, which
    // multiplied a 100k walk's time by five.
    if found.is_empty() {
        let mut probe =
            conn.prepare_cached("SELECT 1 FROM findings WHERE path_id = :id LIMIT 1")?;
        if !probe.exists(rusqlite::named_params! { ":id": path_id })? {
            return Ok(());
        }
    }
    let names: HashSet<&str> = found.iter().map(|f| f.check.name()).collect();
    for check in evaluated {
        if !names.contains(check.name()) {
            let mut clear = conn.prepare_cached(
                "DELETE FROM findings WHERE path_id = :id AND check_name = :check",
            )?;
            clear.execute(rusqlite::named_params! { ":id": path_id, ":check": check.name() })?;
        }
    }
    for f in found {
        let mut upsert = conn.prepare_cached(
            "INSERT INTO findings (path_id, check_name, severity, detail, fact, first_seen, last_seen)
             VALUES (:id, :check, :sev, :detail, :fact, :now, :now)
             ON CONFLICT(path_id, check_name) DO UPDATE SET
                 severity = excluded.severity,
                 detail = excluded.detail,
                 fact = excluded.fact,
                 last_seen = excluded.last_seen",
        )?;
        upsert.execute(rusqlite::named_params! {
            ":id": path_id,
            ":check": f.check.name(),
            ":sev": f.severity as i64,
            ":detail": f.detail,
            ":fact": f.fact,
            ":now": now,
        })?;
    }
    recompute_worst(conn, path_id)
}

/// Recompute `paths.worst_finding` for one row from its unaccepted
/// findings.
pub fn recompute_worst(conn: &Connection, path_id: i64) -> Result<()> {
    let mut update = conn.prepare_cached(
        "UPDATE paths SET worst_finding = COALESCE((
             SELECT max(f.severity) FROM findings f
              WHERE f.path_id = paths.rowid
                AND NOT EXISTS (SELECT 1 FROM exceptions e
                                 WHERE e.path = paths.path
                                   AND e.check_name = f.check_name
                                   AND e.fact = f.fact)
         ), 0)
         WHERE rowid = :id",
    )?;
    update.execute(rusqlite::named_params! { ":id": path_id })?;
    Ok(())
}

/// The value `worst_finding` should hold for a row, computed the long
/// way. For tests that check the summary never drifts.
pub fn aggregate_worst(conn: &Connection, path_id: i64) -> Result<i64> {
    let worst: i64 = conn.query_row(
        "SELECT COALESCE((
             SELECT max(f.severity) FROM findings f
              WHERE f.path_id = p.rowid
                AND NOT EXISTS (SELECT 1 FROM exceptions e
                                 WHERE e.path = p.path AND e.check_name = f.check_name AND e.fact = f.fact)
         ), 0) FROM paths p WHERE p.rowid = :id",
        rusqlite::named_params! { ":id": path_id },
        |r| r.get(0),
    )?;
    Ok(worst)
}

/// Every stored finding, optionally under a path prefix, with its
/// exception (if any) joined in. Ordered by severity desc, then path.
pub fn load(conn: &Connection, under: Option<&str>) -> Result<Vec<StoredFinding>> {
    let mut stmt = conn.prepare(
        "SELECT f.path_id, p.path, f.check_name, f.severity, f.detail, f.fact, f.first_seen, f.last_seen,
                (SELECT e.reason FROM exceptions e
                  WHERE e.path = p.path AND e.check_name = f.check_name AND e.fact = f.fact)
           FROM findings f JOIN paths p ON p.rowid = f.path_id
          WHERE p.tombstoned_at IS NULL
            AND (:under IS NULL OR p.path = :under OR p.path LIKE :prefix ESCAPE '\\')
          ORDER BY f.severity DESC, p.path ASC, f.check_name ASC",
    )?;
    let prefix = under.map(|u| format!("{}/%", like_escape(u)));
    let rows = stmt
        .query_map(rusqlite::named_params! { ":under": under, ":prefix": prefix }, |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, Option<String>>(8)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(path_id, path, check, sev, detail, fact, first, last, accepted)| {
            Some(StoredFinding {
                path_id,
                path,
                check: Check::parse(&check)?,
                severity: Severity::from_i64(sev)?,
                detail,
                fact,
                first_seen: first,
                last_seen: last,
                accepted,
            })
        })
        .collect())
}

fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Unaccepted finding names on one row, for `when = { finding = ... }`
/// and the footer.
pub fn unaccepted_for_row(conn: &Connection, path_id: i64) -> Result<Vec<StoredFinding>> {
    let mut stmt = conn.prepare_cached(
        "SELECT f.path_id, p.path, f.check_name, f.severity, f.detail, f.fact, f.first_seen, f.last_seen
           FROM findings f JOIN paths p ON p.rowid = f.path_id
          WHERE f.path_id = :id
            AND NOT EXISTS (SELECT 1 FROM exceptions e
                             WHERE e.path = p.path AND e.check_name = f.check_name AND e.fact = f.fact)
          ORDER BY f.severity DESC, f.check_name ASC",
    )?;
    let rows = stmt
        .query_map(rusqlite::named_params! { ":id": path_id }, |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(path_id, path, check, sev, detail, fact, first, last)| {
            Some(StoredFinding {
                path_id,
                path,
                check: Check::parse(&check)?,
                severity: Severity::from_i64(sev)?,
                detail,
                fact,
                first_seen: first,
                last_seen: last,
                accepted: None,
            })
        })
        .collect())
}

/// Outcome of `accept`.
#[derive(Debug, PartialEq, Eq)]
pub enum Accepted {
    /// The exception was recorded against the finding's current fact.
    Recorded,
    /// There is no such finding on that path; accepting a hypothetical is
    /// not a decision.
    NoSuchFinding,
}

/// Accept a finding: record an exception for its current fact and
/// recompute the row's summary.
pub fn accept(
    conn: &Connection,
    path: &str,
    check: Check,
    reason: &str,
    now: i64,
) -> Result<Accepted> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT f.path_id, f.fact FROM findings f JOIN paths p ON p.rowid = f.path_id
              WHERE p.path = :path AND f.check_name = :check",
            rusqlite::named_params! { ":path": path, ":check": check.name() },
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    let Some((path_id, fact)) = row else { return Ok(Accepted::NoSuchFinding) };
    conn.execute(
        "INSERT INTO exceptions (path, check_name, fact, reason, accepted_at)
         VALUES (:path, :check, :fact, :reason, :now)
         ON CONFLICT(path, check_name) DO UPDATE SET
             fact = excluded.fact, reason = excluded.reason, accepted_at = excluded.accepted_at",
        rusqlite::named_params! {
            ":path": path, ":check": check.name(), ":fact": fact, ":reason": reason, ":now": now
        },
    )?;
    recompute_worst(conn, path_id)?;
    Ok(Accepted::Recorded)
}

/// Remove an exception. Returns whether one existed.
pub fn revoke(conn: &Connection, path: &str, check: Check) -> Result<bool> {
    let removed = conn.execute(
        "DELETE FROM exceptions WHERE path = :path AND check_name = :check",
        rusqlite::named_params! { ":path": path, ":check": check.name() },
    )?;
    if let Ok(path_id) = conn.query_row(
        "SELECT rowid FROM paths WHERE path = :path",
        rusqlite::named_params! { ":path": path },
        |r| r.get::<_, i64>(0),
    ) {
        recompute_worst(conn, path_id)?;
    }
    Ok(removed == 1)
}

/// Largest file the baseline will hash. Entry points are hand-edited
/// files; anything bigger is recorded as too large rather than read.
pub const BASELINE_MAX_BYTES: u64 = 1024 * 1024;

/// Hash the project's entry points and record them, replacing any earlier
/// baseline for the row. Returns the number of files recorded.
pub fn record_baseline(conn: &Connection, path_id: i64, root: &Path, now: i64) -> Result<usize> {
    let entries = baseline_entries(root);
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM baseline WHERE path_id = :id",
        rusqlite::named_params! { ":id": path_id },
    )?;
    for (file, sha, size) in &entries {
        tx.execute(
            "INSERT INTO baseline (path_id, file, sha256, size, recorded_at)
             VALUES (:id, :file, :sha, :size, :now)",
            rusqlite::named_params! { ":id": path_id, ":file": file, ":sha": sha, ":size": *size as i64, ":now": now },
        )?;
    }
    tx.commit()?;
    Ok(entries.len())
}

/// (relative file, sha256 or "too-large", size) for every entry point
/// present under `root`: the fixed list, root-level `*.sh`, and non-sample
/// git hooks.
pub fn baseline_entries(root: &Path) -> Vec<(String, String, u64)> {
    let mut files: Vec<String> = ENTRY_POINTS.iter().map(|s| s.to_string()).collect();
    if let Ok(dir) = std::fs::read_dir(root) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".sh") && entry.path().is_file() {
                files.push(name);
            }
        }
    }
    if let Ok(dir) = std::fs::read_dir(root.join(".git/hooks")) {
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.ends_with(".sample") && entry.path().is_file() {
                files.push(format!(".git/hooks/{name}"));
            }
        }
    }
    files.sort();
    files.dedup();
    files
        .into_iter()
        .filter_map(|file| {
            let full = root.join(&file);
            let meta = std::fs::symlink_metadata(&full).ok()?;
            if !meta.is_file() {
                return None;
            }
            let size = meta.len();
            let sha = if size > BASELINE_MAX_BYTES {
                "too-large".to_string()
            } else {
                hex_digest(&std::fs::read(&full).ok()?)
            };
            Some((file, sha, size))
        })
        .collect()
}

/// Compare the current entry points of every baselined project against
/// its record and return one finding per changed project (or none).
pub fn baseline_findings(conn: &Connection) -> Result<Vec<(i64, Finding)>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT b.path_id, p.path FROM baseline b JOIN paths p ON p.rowid = b.path_id
          WHERE p.tombstoned_at IS NULL",
    )?;
    let roots = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (path_id, path) in roots {
        let mut recorded = conn.prepare("SELECT file, sha256 FROM baseline WHERE path_id = :id")?;
        let recorded: Vec<(String, String)> = recorded
            .query_map(rusqlite::named_params! { ":id": path_id }, |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let current = baseline_entries(Path::new(&path));
        let mut changed: Vec<String> = Vec::new();
        for (file, sha) in &recorded {
            match current.iter().find(|(f, _, _)| f == file) {
                Some((_, now_sha, _)) if now_sha == sha => {}
                Some(_) => changed.push(format!("{file} changed")),
                None => changed.push(format!("{file} removed")),
            }
        }
        for (file, _, _) in &current {
            if !recorded.iter().any(|(f, _)| f == file) {
                changed.push(format!("{file} added"));
            }
        }
        if !changed.is_empty() {
            let mut fact = current.iter().map(|(f, s, _)| format!("{f}={s}")).collect::<Vec<_>>();
            fact.sort();
            out.push((
                path_id,
                Finding {
                    check: Check::EntrypointChanged,
                    severity: Severity::High,
                    detail: changed.join(", ").chars().take(200).collect(),
                    fact: hex_digest(fact.join("\n").as_bytes()),
                },
            ));
        }
    }
    Ok(out)
}
