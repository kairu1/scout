//! Running the checks over the index, over one path, and over scout's own
//! files.

use std::io::Read;
use std::path::Path;

use rusqlite::Connection;

use super::checks::{self, Check, Context, Finding, ALL_CHECKS};
use super::report::OwnStateFinding;
use super::store;
use super::Severity;
use crate::platform::fs as pfs;
use crate::platform::hash::hex_digest;
use crate::platform::xattr;
use crate::Result;

/// The checks a stat-only pass evaluates (and therefore clears when they
/// no longer hold).
pub fn stat_checks() -> Vec<Check> {
    ALL_CHECKS.iter().copied().filter(|c| c.stat_only()).collect()
}

/// Evaluate the stat-only checks for one path.
pub fn stat_findings(path: &Path, ctx: &Context<'_>) -> Vec<Finding> {
    let Ok(facts) = pfs::facts(path) else { return Vec::new() };
    let basename = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let target = if facts.is_symlink { std::fs::read_link(path).ok() } else { None };
    checks::evaluate(path, &basename, &facts, target.as_deref(), ctx)
}

/// Re-evaluate one row's stat checks and store the result. Used after a
/// fix action succeeds so the marker clears without a full run.
pub fn rescan_path(conn: &Connection, path_id: i64, path: &Path, ctx: &Context<'_>) -> Result<()> {
    let found = stat_findings(path, ctx);
    store::store_for_path(conn, path_id, &found, &stat_checks(), ctx.now)
}

/// Symbolic links directly inside `dir` that lead into a system tree or
/// outside the user's home, folded into one finding on the directory.
/// The index stores canonical paths, so a link never appears as a row of
/// its own: the directory that holds it is where the finding belongs.
pub fn symlink_escapes(dir: &Path, ctx: &Context<'_>) -> Option<Finding> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut hits: Vec<(String, String, Severity)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(facts) = pfs::facts(&path) else { continue };
        if !facts.is_symlink {
            continue;
        }
        let Ok(target) = std::fs::read_link(&path) else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        for f in checks::evaluate(&path, &name, &facts, Some(&target), ctx) {
            if f.check == Check::SymlinkEscape {
                hits.push((name.clone(), target.display().to_string(), f.severity));
            }
        }
    }
    if hits.is_empty() {
        return None;
    }
    hits.sort();
    let severity = hits.iter().map(|h| h.2).max().unwrap_or(Severity::Low);
    let detail: Vec<String> = hits.iter().map(|(n, t, _)| format!("{n} -> {t}")).collect();
    let fact = hex_digest(detail.join("\n").as_bytes());
    Some(checks::Finding {
        check: Check::SymlinkEscape,
        severity,
        detail: detail.join(", ").chars().take(checks::DETAIL_CAP).collect(),
        fact,
    })
}

/// When the previous full run happened, if ever. Read before a run
/// updates it, so "new since last" has a last to be since.
pub fn last_recon_at(conn: &Connection) -> Result<Option<i64>> {
    Ok(conn.query_row("SELECT last_recon_at FROM run_state WHERE id = 1", [], |r| r.get(0))?)
}

/// Where the shell wrapper turned up: an rc file with the installer's
/// marker (or a `scout.bash` source line) at a 1-based line, or a
/// wrapper file itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrapperSite {
    pub path: std::path::PathBuf,
    /// The line the marker sits on; `None` for a wrapper file.
    pub line: Option<usize>,
}

/// Bytes of an rc file read while looking for the marker: the config
/// cap. A shell rc beyond it is not a file scout will scan.
const RC_SCAN_CAP: usize = 256 * 1024;

/// The shell rc files a wrapper is sourced from, relative to `home`.
pub fn rc_files(home: &Path) -> Vec<std::path::PathBuf> {
    [".bashrc", ".bash_profile", ".zshrc", ".profile", ".config/fish/config.fish"]
        .iter()
        .map(|name| home.join(name))
        .collect()
}

/// The line number (1-based) of the first line in `rc` that carries the
/// installer's marker or names `scout.bash`; `None` when neither is
/// there or the file cannot be read. Nothing else about the content is
/// kept.
pub fn wrapper_line(rc: &Path) -> Option<usize> {
    let mut file = std::fs::File::open(rc).ok()?;
    let mut buf = Vec::new();
    std::io::Read::take(&mut file, RC_SCAN_CAP as u64).read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    text.lines()
        .position(|l| l.contains(checks::WRAPPER_MARKER) || l.contains("scout.bash"))
        .map(|i| i + 1)
}

/// What a full run covered.
#[derive(Debug, Default)]
pub struct RunStats {
    pub paths: usize,
    pub findings: usize,
}

/// Run every check over the live candidates (optionally only those at or
/// under `under`), including the ACL probe and the baseline comparison,
/// and record when the run happened.
pub fn scan(conn: &Connection, under: Option<&Path>, ctx: &Context<'_>) -> Result<RunStats> {
    // Every live row at its root's current generation, candidate or not:
    // a credential file recorded only for recon is exactly what this
    // pass is for.
    let mut stmt = conn.prepare(
        "SELECT p.rowid, p.path FROM paths p JOIN roots r ON p.root_id = r.id
          WHERE p.scan_generation = r.current_generation AND p.tombstoned_at IS NULL",
    )?;
    let rows: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let under_str = under.map(|u| u.display().to_string());
    let mut evaluated = stat_checks();
    evaluated.push(Check::AclPresent);
    evaluated.push(Check::SymlinkEscape);
    let mut stats = RunStats::default();
    let tx = conn.unchecked_transaction()?;
    for (path_id, path_text) in rows {
        if let Some(u) = &under_str {
            if path_text != *u && !path_text.starts_with(&format!("{u}/")) {
                continue;
            }
        }
        let path = Path::new(&path_text);
        let mut found = stat_findings(path, ctx);
        if let Ok(has_acl) = xattr::has_posix_acl(path) {
            found.extend(checks::evaluate_acl(has_acl));
        }
        if path.is_dir() {
            found.extend(symlink_escapes(path, ctx));
        }
        stats.paths += 1;
        stats.findings += found.len();
        store::store_for_path(&tx, path_id, &found, &evaluated, ctx.now)?;
    }
    // Baseline comparison: one finding per changed project, cleared where
    // the entry points match again.
    let changed = store::baseline_findings(&tx)?;
    let baselined: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT DISTINCT path_id FROM baseline")?;
        let ids = stmt.query_map([], |r| r.get(0))?.collect::<std::result::Result<Vec<_>, _>>()?;
        ids
    };
    for path_id in baselined {
        let found: Vec<Finding> =
            changed.iter().filter(|(id, _)| *id == path_id).map(|(_, f)| f.clone()).collect();
        stats.findings += found.len();
        store::store_for_path(&tx, path_id, &found, &[Check::EntrypointChanged], ctx.now)?;
    }
    tx.execute(
        "UPDATE run_state SET last_recon_at = :now WHERE id = 1",
        rusqlite::named_params! { ":now": ctx.now },
    )?;
    tx.commit()?;
    Ok(stats)
}

/// Scout's own files: the config that won discovery, the trust store, the
/// index and its siblings, the shell wrapper where it can be found, and
/// the rc files a shell sources at every start. Not index rows, so
/// computed fresh on every run and never stored. Returns the findings
/// and where the wrapper was seen.
pub fn own_state(
    config: Option<&Path>,
    trust_store: &Path,
    index_db: &Path,
    wrapper_candidates: &[std::path::PathBuf],
    rc_files: &[std::path::PathBuf],
    euid: u32,
) -> (Vec<OwnStateFinding>, Vec<WrapperSite>) {
    let mut sites = Vec::new();
    let mut out = Vec::new();
    let mut push = |check: &'static str, severity: Severity, path: &Path, detail: String| {
        out.push(OwnStateFinding { check, severity, path: path.display().to_string(), detail });
    };
    if let Some(config) = config {
        if let Ok(f) = pfs::facts(config) {
            if f.uid != euid {
                push(
                    "own-config-exposed",
                    Severity::Critical,
                    config,
                    format!("owned by uid {}", f.uid),
                );
            } else if f.mode & 0o022 != 0 {
                push(
                    "own-config-exposed",
                    Severity::Critical,
                    config,
                    format!("mode {:o} lets others edit the actions scout runs", f.mode & 0o777),
                );
            }
        }
    }
    for (path, what) in [(trust_store, "trust store"), (index_db, "index")] {
        if let Ok(f) = pfs::facts(path) {
            if f.uid != euid {
                push(
                    "own-state-mode",
                    Severity::High,
                    path,
                    format!("{what} owned by uid {}", f.uid),
                );
            } else if f.mode & 0o077 != 0 {
                push(
                    "own-state-mode",
                    Severity::High,
                    path,
                    format!("{what} is mode {:o}, not 600", f.mode & 0o777),
                );
            }
        }
    }
    for suffix in ["-wal", "-shm"] {
        let mut s = index_db.as_os_str().to_os_string();
        s.push(suffix);
        let sibling = std::path::PathBuf::from(s);
        if let Ok(f) = pfs::facts(&sibling) {
            if f.mode & 0o077 != 0 {
                push(
                    "own-state-mode",
                    Severity::High,
                    &sibling,
                    format!("mode {:o}, not 600", f.mode & 0o777),
                );
            }
        }
    }
    if let Some(dir) = index_db.parent() {
        if let Ok(f) = pfs::facts(dir) {
            if f.mode & 0o077 != 0 {
                push(
                    "own-state-mode",
                    Severity::High,
                    dir,
                    format!("data directory is mode {:o}, not 700", f.mode & 0o777),
                );
            }
        }
    }
    for wrapper in wrapper_candidates {
        if let Ok(f) = pfs::facts(wrapper) {
            sites.push(WrapperSite { path: wrapper.clone(), line: None });
            if f.uid != euid {
                push(
                    "own-wrapper-writable",
                    Severity::High,
                    wrapper,
                    format!("owned by uid {}", f.uid),
                );
            } else if f.mode & 0o022 != 0 {
                push(
                    "own-wrapper-writable",
                    Severity::High,
                    wrapper,
                    format!("mode {:o}: a sourced file others can edit", f.mode & 0o777),
                );
            }
        }
    }
    // An rc file is sourced by every shell the user starts: one others
    // can edit is a finding whether or not the wrapper is in it.
    for rc in rc_files {
        let Ok(f) = pfs::facts(rc) else { continue };
        if !f.is_file {
            continue;
        }
        if let Some(line) = wrapper_line(rc) {
            sites.push(WrapperSite { path: rc.clone(), line: Some(line) });
        }
        if f.uid != euid {
            push(
                "own-wrapper-writable",
                Severity::High,
                rc,
                format!("shell rc owned by uid {}", f.uid),
            );
        } else if f.mode & 0o022 != 0 {
            push(
                "own-wrapper-writable",
                Severity::High,
                rc,
                format!("mode {:o}: a file your shell sources on every start", f.mode & 0o777),
            );
        }
    }
    (out, sites)
}
