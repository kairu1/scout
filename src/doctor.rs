//! Diagnostics (ADR-006). A read-only snapshot of the state scout
//! resolves at startup: which config wins the discovery chain, whether
//! it is trusted, what the index holds, and how the environment reads.
//!
//! Three rules from ADR-006 §Decision bind this module. It never
//! prompts and never writes — config loads with `interactive: false`, so
//! an untrusted config is a finding rather than a prompt. It prints a
//! closed allowlist of environment variables, never the environment,
//! because this output exists to be pasted into bug reports. And it
//! collapses `$HOME` to `~` for the same reason.

use std::path::{Path, PathBuf};

use crate::config::{loader, paths};

/// Severity of a single check. Only `Fail` affects the exit code —
/// a missing index is the correct state on a fresh machine (ADR-006
/// §Consequences), so it must not be confused with a broken one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Ok,
    Warn,
    Fail,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Ok => "ok",
            Level::Warn => "warn",
            Level::Fail => "FAIL",
        }
    }
}

#[derive(Debug)]
pub struct Check {
    pub name: String,
    pub level: Level,
    pub detail: String,
}

impl Check {
    fn new(name: &str, level: Level, detail: impl Into<String>) -> Check {
        Check { name: name.to_string(), level, detail: detail.into() }
    }
}

#[derive(Debug)]
pub struct Section {
    pub title: &'static str,
    pub checks: Vec<Check>,
}

#[derive(Debug)]
pub struct Report {
    pub sections: Vec<Section>,
}

impl Report {
    /// Worst severity anywhere in the report.
    pub fn worst(&self) -> Level {
        self.sections
            .iter()
            .flat_map(|s| s.checks.iter())
            .map(|c| c.level)
            .max()
            .unwrap_or(Level::Ok)
    }

    /// 0 when nothing failed, 1 when something did. Warnings do not
    /// fail: an unindexed scout is not a broken scout.
    pub fn exit_code(&self) -> u8 {
        if self.worst() == Level::Fail {
            1
        } else {
            0
        }
    }

    pub fn render(&self) -> String {
        let width = self
            .sections
            .iter()
            .flat_map(|s| s.checks.iter())
            .map(|c| c.name.chars().count())
            .max()
            .unwrap_or(0);
        let mut out = String::new();
        for section in &self.sections {
            out.push_str(&format!("\n{}\n", section.title));
            for check in &section.checks {
                out.push_str(&format!(
                    "  {:<5} {:<width$}  {}\n",
                    check.level.as_str(),
                    check.name,
                    check.detail,
                    width = width
                ));
            }
        }
        out
    }
}

/// Replace a leading `$HOME` with `~` (ADR-006: a username is not
/// diagnostic information and this output is meant to be pasted).
fn tilde(path: &Path, home: Option<&Path>) -> String {
    let text = path.display().to_string();
    match home {
        Some(h) if !h.as_os_str().is_empty() => {
            let h = h.display().to_string();
            if text == h {
                "~".to_string()
            } else if let Some(rest) = text.strip_prefix(&format!("{h}/")) {
                format!("~/{rest}")
            } else {
                text
            }
        }
        _ => text,
    }
}

/// What a path is, for a discovery-chain line. Symlinks matter here:
/// discovery opens `O_NOFOLLOW`, so a symlinked config silently falls
/// through to the next link, which is exactly the failure ADR-006
/// §Context case 1 describes.
fn describe_entry(path: &Path) -> (Level, &'static str) {
    match path.symlink_metadata() {
        Err(_) => (Level::Ok, "absent"),
        Ok(meta) if meta.file_type().is_symlink() => {
            (Level::Warn, "symlink — discovery skips it (O_NOFOLLOW)")
        }
        Ok(meta) if meta.is_file() => (Level::Ok, "regular file"),
        Ok(_) => (Level::Warn, "not a regular file — discovery skips it"),
    }
}

fn config_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();

    let chain = match paths::discovery_chain() {
        Ok(chain) => chain,
        Err(err) => {
            checks.push(Check::new("chain", Level::Fail, err));
            return Section { title: "config", checks };
        }
    };

    for (i, entry) in chain.iter().enumerate() {
        let (level, what) = describe_entry(entry);
        checks.push(Check::new(
            &format!("chain[{i}]"),
            level,
            format!("{} — {what}", tilde(entry, home)),
        ));
    }

    // Ask the loader which link wins rather than re-deriving it.
    match loader::discover(&chain) {
        Ok(Some(winner)) => checks.push(Check::new("resolved", Level::Ok, tilde(&winner, home))),
        Ok(None) => checks.push(Check::new(
            "resolved",
            Level::Warn,
            "no config found — compiled-in defaults apply",
        )),
        Err(err) => checks.push(Check::new("resolved", Level::Fail, err.to_string())),
    }

    let trust_store = match paths::trust_store_path() {
        Ok(p) => p,
        Err(err) => {
            checks.push(Check::new("load", Level::Fail, err));
            return Section { title: "config", checks };
        }
    };

    // interactive:false — a diagnostic must never prompt (ADR-006).
    match loader::load(&chain, trust_store, false) {
        Ok(config) => {
            let from_user = config.source.is_some();
            checks.push(Check::new(
                "load",
                Level::Ok,
                format!(
                    "{} action(s) active ({})",
                    config.actions.len(),
                    if from_user { "user config merged over defaults" } else { "defaults only" }
                ),
            ));
            for warning in &config.warnings {
                checks.push(Check::new("warning", Level::Warn, warning.clone()));
            }
            if config.enter_action().is_none() {
                checks.push(Check::new(
                    "enter",
                    Level::Warn,
                    "no action bound to Enter — the picker will have nothing to run",
                ));
            }
        }
        Err(err) => checks.push(Check::new("load", Level::Fail, err.to_string())),
    }

    Section { title: "config", checks }
}

fn trust_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();
    match paths::trust_store_path() {
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
                    "nothing trusted yet — the first user config will prompt",
                ));
            } else {
                // A load with interactive:false succeeds only when the
                // config is already trusted, so the config section's
                // `load` line carries the verdict; repeating the hash
                // here would duplicate the loader's judgement.
                checks.push(Check::new(
                    "status",
                    Level::Ok,
                    "see config/load above — it fails when trust is stale",
                ));
            }
        }
        Err(err) => checks.push(Check::new("store", Level::Fail, err)),
    }
    Section { title: "trust", checks }
}

fn index_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();

    let db_path = match paths::default_db_path() {
        Ok(p) => p,
        Err(err) => {
            checks.push(Check::new("database", Level::Fail, err));
            return Section { title: "index", checks };
        }
    };
    checks.push(Check::new("database", Level::Ok, tilde(&db_path, home)));

    if !db_path.exists() {
        checks.push(Check::new(
            "state",
            Level::Warn,
            "no index yet — run 'scout index <path>' to populate",
        ));
        return Section { title: "index", checks };
    }

    // NOT index::pragma::open. That path creates the parent directory,
    // creates the file, runs migrations, and — on a corrupt database —
    // renames it aside and rebuilds. Calling it from a diagnostic would
    // repair the fault while reporting it, which is the behaviour
    // ADR-006 §Alternatives 4 rejects outright: the evidence is gone by
    // the time the user reads the line describing it. Read-only open
    // cannot create, migrate, or recover.
    if db_path.symlink_metadata().map(|m| m.file_type().is_symlink()).unwrap_or(false) {
        checks.push(Check::new(
            "open",
            Level::Fail,
            "database path is a symlink; scout refuses to open it (ADR-003 §4)",
        ));
        return Section { title: "index", checks };
    }
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
            // than emitting five more lines of -1 and "unknown".
            checks.push(Check::new("schema", Level::Fail, err.to_string()));
            return Section { title: "index", checks };
        }
    }

    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap_or(-1);
    let live: i64 = conn
        .query_row("SELECT count(*) FROM paths WHERE tombstoned_at IS NULL", [], |r| r.get(0))
        .unwrap_or(-1);
    checks.push(Check::new(
        "paths",
        if live > 0 { Level::Ok } else { Level::Warn },
        format!("{live} live, {rows} total"),
    ));

    let generations: Result<(i64, i64), _> = conn.query_row(
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
                     still serves — re-run 'scout index'"
                )
            };
            checks.push(Check::new("generation", level, detail));
        }
        Err(err) => checks.push(Check::new("generation", Level::Fail, err.to_string())),
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

/// Closed allowlist (ADR-006 §Decision). Never print the environment.
const ENV_ALLOWLIST: [&str; 4] = ["EDITOR", "VISUAL", "SCOUT_LOG", "TERM"];

fn environment_section(home: Option<&Path>) -> Section {
    use std::io::IsTerminal;
    let mut checks = Vec::new();

    match home {
        Some(h) => checks.push(Check::new("HOME", Level::Ok, h.display().to_string())),
        None => checks.push(Check::new("HOME", Level::Fail, "unset or empty; scout requires it")),
    }

    // Report each XDG variable AND what it resolved to — "set" and
    // "where it points" are different questions and both get asked.
    for (var, resolved) in [
        ("XDG_CONFIG_HOME", paths::config_home()),
        ("XDG_DATA_HOME", paths::data_home()),
        ("XDG_STATE_HOME", paths::state_home()),
    ] {
        let set = std::env::var(var).ok().filter(|v| !v.is_empty());
        let detail = match (&set, &resolved) {
            (Some(_), Ok(p)) => format!("{} (set)", tilde(p, home)),
            (None, Ok(p)) => format!("{} (default)", tilde(p, home)),
            (_, Err(err)) => err.clone(),
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
                "unset — the built-in edit action falls back to a compiled default",
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

fn logs_section(home: Option<&Path>) -> Section {
    let mut checks = Vec::new();
    let path = match paths::state_home() {
        Ok(state) => state.join("scout/scout.log"),
        Err(err) => {
            checks.push(Check::new("file", Level::Fail, err));
            return Section { title: "logs", checks };
        }
    };
    checks.push(Check::new("file", Level::Ok, tilde(&path, home)));

    match std::fs::read_to_string(&path) {
        Ok(body) => {
            let lines: Vec<&str> = body.lines().collect();
            checks.push(Check::new("lines", Level::Ok, lines.len().to_string()));
            for line in lines.iter().rev().take(LOG_TAIL_LINES).rev() {
                checks.push(Check::new("tail", Level::Ok, crate::ui::strip::clean(line)));
            }
        }
        Err(_) => checks.push(Check::new(
            "lines",
            Level::Ok,
            "no log yet — the picker writes it, the CLI logs to stderr",
        )),
    }

    Section { title: "logs", checks }
}

/// Gather the full report. Read-only: nothing here prompts, writes, or
/// mutates state (ADR-006 §Decision).
pub fn report() -> Report {
    let home: Option<PathBuf> = paths::home().ok();
    let home = home.as_deref();
    Report {
        sections: vec![
            config_section(home),
            trust_section(home),
            index_section(home),
            environment_section(home),
            logs_section(home),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_fails_only_on_fail() {
        let mk = |level| Report {
            sections: vec![Section { title: "t", checks: vec![Check::new("c", level, "d")] }],
        };
        // A fresh machine with no index is full of warnings and is not
        // broken; only Fail may set a non-zero exit code (ADR-006).
        assert_eq!(mk(Level::Ok).exit_code(), 0);
        assert_eq!(mk(Level::Warn).exit_code(), 0);
        assert_eq!(mk(Level::Fail).exit_code(), 1);
    }

    #[test]
    fn worst_wins_across_sections() {
        let report = Report {
            sections: vec![
                Section { title: "a", checks: vec![Check::new("x", Level::Ok, "")] },
                Section { title: "b", checks: vec![Check::new("y", Level::Fail, "")] },
                Section { title: "c", checks: vec![Check::new("z", Level::Warn, "")] },
            ],
        };
        assert_eq!(report.worst(), Level::Fail);
        assert_eq!(report.exit_code(), 1);
    }

    #[test]
    fn tilde_collapses_only_a_real_home_prefix() {
        let home = PathBuf::from("/home/agent");
        assert_eq!(tilde(Path::new("/home/agent"), Some(&home)), "~");
        assert_eq!(tilde(Path::new("/home/agent/p/x"), Some(&home)), "~/p/x");
        // A sibling that merely shares the prefix must not collapse.
        assert_eq!(tilde(Path::new("/home/agentx/f"), Some(&home)), "/home/agentx/f");
        assert_eq!(tilde(Path::new("/etc/scout"), Some(&home)), "/etc/scout");
        assert_eq!(tilde(Path::new("/home/agent/p"), None), "/home/agent/p");
    }

    #[test]
    fn symlinked_chain_entry_is_flagged_not_silent() {
        // The ADR-006 case-1 failure: discovery opens O_NOFOLLOW, so a
        // symlinked config is skipped without a word. doctor must say so.
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
        // ADR-006: this output is meant to be pasted. The allowlist is
        // closed; widening it is a review decision, and this test is the
        // tripwire that makes widening deliberate.
        assert_eq!(ENV_ALLOWLIST.len(), 4);
        for name in ["AWS_SECRET_ACCESS_KEY", "GITHUB_TOKEN", "SSH_AUTH_SOCK", "PATH"] {
            assert!(!ENV_ALLOWLIST.contains(&name), "{name} must not be printed");
        }
    }

    #[test]
    fn render_marks_every_level_and_indents() {
        let report = Report {
            sections: vec![Section {
                title: "index",
                checks: vec![
                    Check::new("database", Level::Ok, "~/x/index.db"),
                    Check::new("state", Level::Warn, "no index yet"),
                    Check::new("integrity", Level::Fail, "malformed"),
                ],
            }],
        };
        let text = report.render();
        assert!(text.contains("index\n"));
        assert!(text.contains("  ok    database"));
        assert!(text.contains("  warn  state"));
        assert!(text.contains("  FAIL  integrity"));
    }
}
