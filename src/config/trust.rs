//! Trust store + first-run prompt (ADR-003 §3). Hash-pinned per config
//! path; prompt on new or changed hash; silent on match; non-TTY
//! refuses rather than auto-trusting.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

use super::{Action, Step};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStatus {
    /// Store carries this exact hash for this config path.
    Trusted,
    /// No entry for this config path.
    New,
    /// Entry exists with a different hash.
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

    /// Record `hash` for `config_path` and persist at mode 0600 under a
    /// 0700 parent (ADR-003 §3).
    pub fn record(&mut self, config_path: &Path, hash: &str) -> std::io::Result<()> {
        self.entries.insert(config_path.display().to_string(), hash.to_string());
        if let Some(parent) = self.path.parent() {
            if !parent.exists() {
                fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
            }
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
        fs::write(&self.path, body)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    }

    pub fn store_path(&self) -> &Path {
        &self.path
    }
}

/// Render the trust prompt on stderr and read the verdict from stdin.
/// Exactly `y` + newline accepts (case-sensitive). Caller must have
/// verified a TTY is attached.
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
    for action in actions {
        writeln!(err, "  [{}] {}", action.name, action.description)?;
        for step in &action.steps {
            match step {
                Step::Spawn { argv, wait, cwd } => {
                    let argv: Vec<&str> = argv.iter().map(|t| t.raw.as_str()).collect();
                    write!(err, "    spawn {argv:?} wait={wait}")?;
                    if let Some(cwd) = cwd {
                        write!(err, " cwd={}", cwd.raw)?;
                    }
                    writeln!(err)?;
                }
                Step::Print { format } => writeln!(err, "    print {:?}", format.raw)?,
                Step::Env { set } => {
                    let pairs: Vec<String> =
                        set.iter().map(|(k, v)| format!("{k}={}", v.raw)).collect();
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

/// ISO-8601 UTC from SystemTime, hand-rolled (`chrono` refused,
/// ADR-002). Civil-from-days per the well-known Hinnant algorithm.
pub fn iso8601(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, m, s) = (tod / 3600, tod % 3600 / 60, tod % 60);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}
