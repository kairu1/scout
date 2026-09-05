//! The trust store and the first-run prompt. One hash per config path;
//! prompt on a new or changed hash; silent on a match; a missing terminal
//! refuses rather than trusting.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::actions::{Action, Step};
use crate::platform::fs as pfs;
use crate::platform::time::iso8601;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStatus {
    /// The store carries this exact hash for this config path.
    Trusted,
    /// No entry for this config path.
    New,
    /// An entry exists with a different hash.
    Changed { previous: String },
}

pub struct TrustStore {
    path: PathBuf,
    entries: HashMap<String, String>,
}

impl TrustStore {
    /// Load (or start empty at) `path`. Format: one `<hex> <config-path>`
    /// per line.
    pub fn load(path: PathBuf) -> std::io::Result<TrustStore> {
        let mut entries = HashMap::new();
        match fs::read_to_string(&path) {
            Ok(content) => {
                for line in content.lines() {
                    if let Some((hash, config_path)) = line.split_once(' ') {
                        entries.insert(config_path.to_string(), hash.to_string());
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        Ok(TrustStore { path, entries })
    }

    pub fn status(&self, config_path: &Path, hash: &str) -> TrustStatus {
        match self.entries.get(&config_path.display().to_string()) {
            Some(stored) if stored == hash => TrustStatus::Trusted,
            Some(stored) => TrustStatus::Changed { previous: stored.clone() },
            None => TrustStatus::New,
        }
    }

    /// Record `hash` for `config_path` and persist, 0600 under a 0700
    /// parent, with no instant at which the file is wider than 0600.
    pub fn record(&mut self, config_path: &Path, hash: &str) -> std::io::Result<()> {
        self.entries.insert(config_path.display().to_string(), hash.to_string());
        if let Some(parent) = self.path.parent() {
            pfs::create_private_dir(parent)?;
        }
        let mut body = String::new();
        let mut sorted: Vec<_> = self.entries.iter().collect();
        sorted.sort();
        for (config_path, hash) in sorted {
            body.push_str(hash);
            body.push(' ');
            body.push_str(config_path);
            body.push('\n');
        }
        pfs::write_private(&self.path, body.as_bytes())
    }

    pub fn store_path(&self) -> &Path {
        &self.path
    }
}

/// Render the trust prompt on stderr and read the verdict from stdin.
/// Exactly `y` plus newline accepts (case-sensitive). The caller must
/// have verified a terminal is attached.
pub fn prompt(
    config_path: &Path,
    actions: &[Action],
    status: &TrustStatus,
) -> std::io::Result<bool> {
    let mut err = std::io::stderr().lock();
    let mtime = fs::metadata(config_path)
        .and_then(|m| m.modified())
        .map(iso8601)
        .unwrap_or_else(|_| "unknown".into());

    if let TrustStatus::Changed { previous } = status {
        writeln!(err, "scout: config CHANGED since it was last trusted (was {previous})")?;
    }
    writeln!(err, "scout: config: {}", config_path.display())?;
    writeln!(err, "scout: mtime:  {mtime}")?;
    writeln!(err, "scout: {} action(s):", actions.len())?;
    // Every field below is untrusted config rendered at the exact moment
    // the user decides whether to trust it. Raw, a name carrying an
    // escape sequence could clear the screen and forge the [y/N] line.
    let show = crate::ui::strip::clean;
    for action in actions {
        // `keybinding` and `unsafe_shell_template` are both inside the
        // hash, so changing either re-prompts; without rendering them the
        // new prompt would be byte-identical to the old one and the user
        // would be asked to re-approve with no visible reason.
        write!(err, "  [{}]", show(&action.name))?;
        if let Some(binding) = &action.keybinding {
            write!(err, " key={}", show(binding))?;
        }
        if action.unsafe_shell_template {
            write!(err, " UNSAFE-SHELL-TEMPLATE")?;
        }
        writeln!(err, " {}", show(&action.description))?;
        for step in &action.steps {
            match step {
                Step::Spawn { argv, wait, cwd } => {
                    let argv: Vec<String> = argv.iter().map(|t| show(&t.raw)).collect();
                    write!(err, "    spawn {argv:?} wait={wait}")?;
                    if let Some(cwd) = cwd {
                        write!(err, " cwd={}", show(&cwd.raw))?;
                    }
                    writeln!(err)?;
                }
                Step::Print { format } => writeln!(err, "    print {:?}", show(&format.raw))?,
                Step::Env { set } => {
                    let pairs: Vec<String> =
                        set.iter().map(|(k, v)| format!("{}={}", show(k), show(&v.raw))).collect();
                    writeln!(err, "    env {}", pairs.join(" "))?;
                }
                Step::BuiltinEdit => writeln!(err, "    builtin editor spawn")?,
            }
        }
    }
    write!(err, "trust these {} action(s) from {}? [y/N] ", actions.len(), config_path.display())?;
    err.flush()?;

    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches('\n') == "y")
}

pub fn tty_available() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}
