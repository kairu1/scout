//! `scout doctor`: a read-only snapshot of the state scout resolves at
//! startup. Which config won discovery, whether it is trusted, what the
//! index holds, how the environment reads.
//!
//! Owns: the report model (`Report`, `Section`, `Check`, `Level`), both
//! renderings, and the `~` collapse for paths.
//! Refuses to: prompt, create, migrate, re-trust, repair, or open the
//! index through the normal open path (which does all of those). It
//! prints a closed allowlist of environment variables, never the
//! environment, because this output exists to be pasted into bug reports.
//! Exposes: `report()`.

mod checks;

use std::path::Path;

pub use checks::ENV_ALLOWLIST;

/// Severity of one check. Only `Fail` affects the exit code: a missing
/// index is the correct state on a fresh machine, not a broken one.
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
    /// Details carry config paths, environment values and log lines, all
    /// of them attacker-influenceable and all of them printed to a
    /// terminal. Stripping here covers every construction site rather
    /// than asking each one to remember.
    pub(crate) fn new(name: &str, level: Level, detail: impl Into<String>) -> Check {
        Check { name: name.to_string(), level, detail: crate::ui::strip::clean(&detail.into()) }
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

    /// 0 when nothing failed, 1 when something did. Warnings do not fail.
    pub fn exit_code(&self) -> u8 {
        if self.worst() == Level::Fail {
            1
        } else {
            0
        }
    }

    /// Tab-separated, one check per line: level, section, name, detail.
    /// Detail last, because it is the only unconstrained field.
    pub fn render_tsv(&self) -> String {
        let mut out = String::new();
        for section in &self.sections {
            for check in &section.checks {
                out.push_str(&format!(
                    "{}\t{}\t{}\t{}\n",
                    check.level.as_str(),
                    section.title,
                    check.name,
                    check.detail.replace(['\t', '\n'], " ")
                ));
            }
        }
        out
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

/// Replace a leading `$HOME` with `~`: a username is not diagnostic
/// information and this output is meant to leave the machine.
pub(crate) fn tilde(path: &Path, home: Option<&Path>) -> String {
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

/// Gather the full report. Read-only: nothing here prompts, writes, or
/// mutates state.
pub fn report() -> Report {
    let home = crate::platform::xdg::home().ok();
    let home = home.as_deref();
    Report {
        sections: vec![
            checks::config_section(home),
            checks::trust_section(home),
            checks::index_section(home),
            checks::environment_section(home),
            checks::logs_section(home),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn exit_code_fails_only_on_fail() {
        let mk = |level| Report {
            sections: vec![Section { title: "t", checks: vec![Check::new("c", level, "d")] }],
        };
        // A fresh machine with no index is full of warnings and is not
        // broken; only Fail may set a non-zero exit code.
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
