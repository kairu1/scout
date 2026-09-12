//! The five sections of the report and the facts each one reads.

use std::path::Path;

use super::{tilde, Check, Level, Section};
use crate::config::loader;
use crate::locations;
use crate::platform::xdg;

/// What a path is, for a discovery-chain line. Symlinks matter here:
/// discovery refuses to follow one, so a symlinked config silently falls
/// through to the next link, and the user swears the file is right there.
pub(crate) fn describe_entry(path: &Path) -> (Level, &'static str) {
    match path.symlink_metadata() {
        Err(_) => (Level::Ok, "absent"),
        Ok(meta) if meta.file_type().is_symlink() => {
            (Level::Warn, "symlink - discovery skips it (O_NOFOLLOW)")
        }
        Ok(meta) if meta.is_file() => (Level::Ok, "regular file"),
        Ok(_) => (Level::Warn, "not a regular file - discovery skips it"),
    }
}

pub(super) fn config_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();

    let chain = match locations::discovery_chain() {
        Ok(chain) => chain,
        Err(err) => {
            checks.push(Check::new("chain", Level::Fail, err.to_string()));
            return Section { title: "config", checks };
        }
    };

    for (i, entry) in chain.iter().enumerate() {
        let (level, what) = describe_entry(entry);
        checks.push(Check::new(
            &format!("chain[{i}]"),
            level,
            format!("{} - {what}", tilde(entry, home)),
        ));
    }

    // Ask the loader which link wins rather than re-deriving it.
    match loader::discover(&chain) {
        Ok(Some(winner)) => checks.push(Check::new("resolved", Level::Ok, tilde(&winner, home))),
        Ok(None) => checks.push(Check::new(
            "resolved",
            Level::Warn,
            "no config found - compiled-in defaults apply",
        )),
        Err(err) => checks.push(Check::new("resolved", Level::Fail, err.to_string())),
    }

    let trust_store = match locations::trust_store() {
        Ok(p) => p,
        Err(err) => {
            checks.push(Check::new("load", Level::Fail, err.to_string()));
            return Section { title: "config", checks };
        }
    };

    // Non-interactive: a diagnostic must never prompt.
    match loader::load(&chain, trust_store, false) {
        Ok(config) => {
            let from_user = config.source.is_some();
            // Mode-gated actions are counted per run, so the user can see
            // that a session and a one-shot offer different sets; an
            // unmoded action is in both counts.
            let per_mode = if config.actions.iter().any(|a| a.mode().is_some()) {
                format!(
                    "; a session offers {}, a one-shot {}",
                    config.for_mode(true).actions.len(),
                    config.for_mode(false).actions.len()
                )
            } else {
                String::new()
            };
            checks.push(Check::new(
                "load",
                Level::Ok,
                format!(
                    "{} action(s) loaded ({}{per_mode})",
                    config.actions.len(),
                    if from_user { "user config merged over defaults" } else { "defaults only" }
                ),
            ));
            for warning in &config.warnings {
                checks.push(Check::new("warning", Level::Warn, warning.clone()));
            }
            // Enter is resolved per run: a mode with no Enter action has
            // nothing to run, whatever the other mode binds.
            for (session, run) in [(false, "a one-shot run"), (true, "a session")] {
                if config.for_mode(session).enter_action().is_none() {
                    checks.push(Check::new(
                        "enter",
                        Level::Warn,
                        format!("no action bound to Enter in {run} - the picker will have nothing to run"),
                    ));
                }
            }
        }
        Err(err) => checks.push(Check::new("load", Level::Fail, err.to_string())),
    }

    Section { title: "config", checks }
}

pub(super) fn trust_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();
    match locations::trust_store() {
        Ok(path) => {
            let exists = path.exists();
            checks.push(Check::new(
                "store",
                Level::Ok,
                format!("{}{}", tilde(&path, home), if exists { "" } else { " (absent)" }),
            ));
            if !exists {
                checks.push(Check::new(
                    "status",
                    Level::Ok,
                    "nothing trusted yet - the first user config will prompt",
                ));
            } else {
                // A non-interactive load succeeds only when the config is
                // already trusted, so the config section's `load` line
                // carries the verdict.
                checks.push(Check::new(
                    "status",
                    Level::Ok,
                    "see config/load above - it fails when trust is stale",
                ));
            }
        }
        Err(err) => checks.push(Check::new("store", Level::Fail, err.to_string())),
    }
    Section { title: "trust", checks }
}

pub(super) fn index_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();

    let db_path = match locations::index_db() {
        Ok(p) => p,
        Err(err) => {
            checks.push(Check::new("database", Level::Fail, err.to_string()));
            return Section { title: "index", checks };
        }
    };
    checks.push(Check::new("database", Level::Ok, tilde(&db_path, home)));

    // Before `exists()`, which follows symlinks: a dangling symlink at the
    // DB path would otherwise report "no index yet" and exit 0, while
    // every other subcommand refuses the same path outright.
    if crate::platform::fs::is_symlink(&db_path) {
        checks.push(Check::new(
            "open",
            Level::Fail,
            "database path is a symlink; scout refuses to open it",
        ));
        return Section { title: "index", checks };
    }

    if !db_path.exists() {
        checks.push(Check::new(
            "state",
            Level::Warn,
            "no index yet - run 'scout index <path>' to populate",
        ));
        return Section { title: "index", checks };
    }

    // Not the normal open path. That path creates the parent directory,
    // creates the file, runs migrations, and on a corrupt database renames
    // it aside and rebuilds. A diagnostic that did that would repair the
    // fault while reporting it: the evidence is gone by the time the user
    // reads the line describing it.
    //
    // Read-only, and deliberately not `immutable=1`. That was tried, to
    // stop SQLite creating `-shm`/`-wal` sidecars while reading, and it
    // made the report lie three ways: `PRAGMA journal_mode` said `delete`
    // for a healthy WAL database, rows still in an uncheckpointed `-wal`
    // were invisible (a crashed indexer read as near-empty and healthy),
    // and the URI form broke on any `%`, `?` or `#` in the path. A
    // diagnostic that reads the truth beats one that writes no bytes.
    let conn = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => conn,
        Err(err) => {
            checks.push(Check::new("open", Level::Fail, format!("read-only open failed: {err}")));
            return Section { title: "index", checks };
        }
    };

    match crate::index::schema::schema_version(&conn) {
        Ok(v) => checks.push(Check::new("schema", Level::Ok, format!("version {v}"))),
        Err(err) => {
            // A corrupt file fails here first; say so and stop, rather
            // than emitting five more lines of "unknown".
            checks.push(Check::new("schema", Level::Fail, err.to_string()));
            return Section { title: "index", checks };
        }
    }

    // A failed count is a structural fault, not a `-1`.
    // "Live" means served: under a root that completed a walk, at or past
    // its generation, not tombstoned. Rows outside that are kept, not shown.
    let counts: std::result::Result<(i64, i64), _> = conn.query_row(
        "SELECT (SELECT count(*) FROM paths),
                (SELECT count(*) FROM paths p JOIN roots r ON p.root_id = r.id
                  WHERE r.current_generation >= 1
                    AND p.scan_generation >= r.current_generation
                    AND p.tombstoned_at IS NULL)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    match counts {
        Ok((rows, live)) => checks.push(Check::new(
            "paths",
            if live > 0 { Level::Ok } else { Level::Warn },
            format!("{live} live, {rows} total"),
        )),
        Err(err) => {
            checks.push(Check::new("paths", Level::Fail, format!("cannot count paths: {err}")));
            return Section { title: "index", checks };
        }
    }

    let generations: std::result::Result<(i64, i64), _> = conn.query_row(
        "SELECT current_generation, last_complete_generation FROM run_state WHERE id = 1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    );
    match generations {
        Ok((current, complete)) => {
            let level = if current == complete { Level::Ok } else { Level::Warn };
            let detail = if current == complete {
                format!("generation {current}, last run completed")
            } else {
                format!(
                    "generation {current} started but never completed; generation {complete} \
                     still serves - re-run 'scout index'"
                )
            };
            checks.push(Check::new("generation", level, detail));
        }
        Err(err) => checks.push(Check::new("generation", Level::Fail, err.to_string())),
    }

    // Which trees the index holds and how each was walked. A database
    // from before roots existed has no table; that is a warning to
    // re-index, never a repair.
    let roots: std::result::Result<Vec<(i64, String)>, _> = conn
        .prepare("SELECT path, hidden, follow, recon, current_generation FROM roots ORDER BY id")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| {
                let mut flags = Vec::new();
                if r.get::<_, i64>(1)? != 0 {
                    flags.push("hidden".to_string());
                }
                if r.get::<_, i64>(2)? != 0 {
                    flags.push("follow".to_string());
                }
                if r.get::<_, i64>(3)? == 0 {
                    flags.push("no-recon".to_string());
                }
                let generation: i64 = r.get(4)?;
                flags.push(if generation >= 1 {
                    format!("generation {generation}")
                } else {
                    "never completed a walk".to_string()
                });
                let path: String = r.get(0)?;
                Ok((generation, format!("{path} ({})", flags.join(", "))))
            })?
            .collect()
        });
    match roots {
        Ok(roots) if roots.is_empty() => {
            checks.push(Check::new("roots", Level::Warn, "none - run 'scout index <path>'"))
        }
        Ok(roots) => {
            // A root that never completed a walk serves nothing: a warning.
            let level = if roots.iter().all(|(g, _)| *g >= 1) { Level::Ok } else { Level::Warn };
            let lines: Vec<String> = roots.into_iter().map(|(_, l)| l).collect();
            checks.push(Check::new(
                "roots",
                level,
                format!("{}: {}", lines.len(), lines.join("; ")),
            ))
        }
        Err(err) => checks.push(Check::new(
            "roots",
            Level::Warn,
            format!("no roots table ({err}); run 'scout index <path>' to migrate"),
        )),
    }

    let journal: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap_or_else(|_| "unknown".into());
    checks.push(Check::new(
        "journal",
        if journal.eq_ignore_ascii_case("wal") { Level::Ok } else { Level::Warn },
        journal,
    ));

    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap_or_else(|err| format!("check failed: {err}"));
    checks.push(Check::new(
        "integrity",
        if integrity == "ok" { Level::Ok } else { Level::Fail },
        integrity,
    ));

    Section { title: "index", checks }
}

/// Closed allowlist. Never print the environment: this output exists to
/// be pasted, and a debugging aid must not become a credential leak one
/// paste at a time. Widening it is a review decision.
pub const ENV_ALLOWLIST: [&str; 4] = ["EDITOR", "VISUAL", "SCOUT_LOG", "TERM"];

pub(super) fn environment_section(home: Option<&Path>) -> Section {
    use std::io::IsTerminal;
    let mut checks = Vec::new();

    match home {
        Some(h) => checks.push(Check::new("HOME", Level::Ok, h.display().to_string())),
        None => checks.push(Check::new("HOME", Level::Fail, "unset or empty; scout requires it")),
    }

    // Report each XDG variable and what it resolved to: "set" and "where
    // it points" are different questions and both get asked.
    for (var, resolved) in [
        ("XDG_CONFIG_HOME", xdg::config_home()),
        ("XDG_DATA_HOME", xdg::data_home()),
        ("XDG_STATE_HOME", xdg::state_home()),
    ] {
        let set = std::env::var(var).ok().filter(|v| !v.is_empty());
        let detail = match (&set, &resolved) {
            (Some(_), Ok(p)) => format!("{} (set)", tilde(p, home)),
            (None, Ok(p)) => format!("{} (default)", tilde(p, home)),
            (_, Err(err)) => err.to_string(),
        };
        let level = if resolved.is_ok() { Level::Ok } else { Level::Fail };
        checks.push(Check::new(var, level, detail));
    }

    for var in ENV_ALLOWLIST {
        match std::env::var(var).ok().filter(|v| !v.is_empty()) {
            Some(value) => checks.push(Check::new(var, Level::Ok, value)),
            None if var == "EDITOR" => checks.push(Check::new(
                var,
                Level::Warn,
                "unset - the built-in edit action falls back to a compiled default",
            )),
            None => checks.push(Check::new(var, Level::Ok, "unset")),
        }
    }

    checks.push(Check::new(
        "tty",
        Level::Ok,
        format!(
            "stdin {}, stdout {}, stderr {}",
            tty_word(std::io::stdin().is_terminal()),
            tty_word(std::io::stdout().is_terminal()),
            tty_word(std::io::stderr().is_terminal()),
        ),
    ));

    Section { title: "environment", checks }
}

fn tty_word(is_tty: bool) -> &'static str {
    if is_tty {
        "tty"
    } else {
        "not a tty"
    }
}

const LOG_TAIL_LINES: usize = 5;

pub(super) fn logs_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();
    let path = match locations::log_file() {
        Ok(p) => p,
        Err(err) => {
            checks.push(Check::new("file", Level::Fail, err.to_string()));
            return Section { title: "logs", checks };
        }
    };
    checks.push(Check::new("file", Level::Ok, tilde(&path, home)));

    match std::fs::read_to_string(&path) {
        Ok(body) => {
            let lines: Vec<&str> = body.lines().collect();
            checks.push(Check::new("lines", Level::Ok, lines.len().to_string()));
            for line in lines.iter().rev().take(LOG_TAIL_LINES).rev() {
                checks.push(Check::new("tail", Level::Ok, *line));
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => checks.push(Check::new(
            "lines",
            Level::Ok,
            "no log yet - the picker writes it, the CLI logs to stderr",
        )),
        // Anything else is a real fault, and reporting it as "absent" hid
        // it in the one subcommand whose job is to find faults.
        Err(err) => checks.push(Check::new("lines", Level::Warn, format!("unreadable: {err}"))),
    }

    Section { title: "logs", checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symlinked_chain_entry_is_flagged_not_silent() {
        // Discovery refuses to follow a symlink, so a symlinked config is
        // skipped without a word. doctor must say so.
        let dir = std::env::temp_dir().join(format!("scout-doctor-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let target = dir.join("real.toml");
        let link = dir.join("link.toml");
        std::fs::write(&target, "schema_version = 1\n").unwrap();
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(describe_entry(&target).0, Level::Ok);
        let (level, what) = describe_entry(&link);
        assert_eq!(level, Level::Warn);
        assert!(what.contains("symlink"), "{what}");
        assert_eq!(describe_entry(&dir.join("missing.toml")).0, Level::Ok);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_allowlist_excludes_secret_bearing_names() {
        // This output is meant to be pasted. The allowlist is closed;
        // widening it is a review decision, and this is the tripwire.
        assert_eq!(ENV_ALLOWLIST.len(), 4);
        for name in ["AWS_SECRET_ACCESS_KEY", "GITHUB_TOKEN", "SSH_AUTH_SOCK", "PATH"] {
            assert!(!ENV_ALLOWLIST.contains(&name), "{name} must not be printed");
        }
    }
}
