//! The trust store and the first-run prompt. One hash per config path;
//! prompt on a new or changed hash; silent on a match; a missing terminal
//! refuses rather than trusting.
//!
//! Store format, one entry per line: `v2 <hex> <config-path>`. Lines
//! without a version prefix (`<hex> <config-path>`) were written by the
//! first hash format and are read as `v1`; they never match a current
//! hash, so a user upgrading is prompted once and told why.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::actions::{Action, Step};
use crate::platform::fs as pfs;
use crate::platform::time::iso8601;

/// The hash format this scout writes and accepts.
pub const STORE_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStatus {
    /// The store carries this exact hash, in the current format, for
    /// this config path.
    Trusted,
    /// No entry for this config path.
    New,
    /// An entry exists with a different hash or an older format.
    /// `previous` reads `v<N> <hex>`.
    Changed { previous: String },
}

pub struct TrustStore {
    path: PathBuf,
    /// config path -> (format version, hash)
    entries: HashMap<String, (u8, String)>,
}

impl TrustStore {
    /// Load (or start empty at) `path`.
    pub fn load(path: PathBuf) -> std::io::Result<TrustStore> {
        let mut entries = HashMap::new();
        match fs::read_to_string(&path) {
            Ok(content) => {
                for line in content.lines() {
                    let mut fields = line.splitn(3, ' ');
                    match (fields.next(), fields.next(), fields.next()) {
                        (Some(version), Some(hash), Some(config_path))
                            if version.len() == 2 && version.starts_with('v') =>
                        {
                            let v = version[1..].parse::<u8>().unwrap_or(1);
                            entries.insert(config_path.to_string(), (v, hash.to_string()));
                        }
                        (Some(hash), Some(config_path), None) => {
                            entries.insert(config_path.to_string(), (1, hash.to_string()));
                        }
                        // A path containing a space in an old-format line
                        // lands here; it was written as `<hex> <path with
                        // spaces>`, so the "third field" is the rest.
                        (Some(hash), Some(head), Some(rest)) => {
                            entries.insert(format!("{head} {rest}"), (1, hash.to_string()));
                        }
                        _ => {}
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
            Some((STORE_VERSION, stored)) if stored == hash => TrustStatus::Trusted,
            Some((version, stored)) => {
                TrustStatus::Changed { previous: format!("v{version} {stored}") }
            }
            None => TrustStatus::New,
        }
    }

    /// Record `hash` for `config_path` in the current format and persist,
    /// 0600 under a 0700 parent, with no instant at which the file is
    /// wider than 0600. Entries for other configs are kept as they were.
    pub fn record(&mut self, config_path: &Path, hash: &str) -> std::io::Result<()> {
        self.entries.insert(config_path.display().to_string(), (STORE_VERSION, hash.to_string()));
        if let Some(parent) = self.path.parent() {
            pfs::create_private_dir(parent)?;
        }
        let mut body = String::new();
        let mut sorted: Vec<_> = self.entries.iter().collect();
        sorted.sort();
        for (config_path, (version, hash)) in sorted {
            body.push_str(&format!("v{version} {hash} {config_path}\n"));
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
    keys: &std::collections::BTreeMap<String, String>,
    status: &TrustStatus,
) -> std::io::Result<bool> {
    let mut err = std::io::stderr().lock();
    let mtime = fs::metadata(config_path)
        .and_then(|m| m.modified())
        .map(iso8601)
        .unwrap_or_else(|_| "unknown".into());

    if let TrustStatus::Changed { previous } = status {
        if previous.starts_with(&format!("v{STORE_VERSION} ")) {
            writeln!(err, "scout: config CHANGED since it was last trusted (was {previous})")?;
        } else {
            writeln!(
                err,
                "scout: the trust format changed to v{STORE_VERSION}; this config needs approving \
                 once more (previously trusted as {previous})"
            )?;
        }
    }
    writeln!(err, "scout: config: {}", config_path.display())?;
    writeln!(err, "scout: mtime:  {mtime}")?;
    writeln!(err, "scout: {} action(s):", actions.len())?;
    // Every field below is untrusted config rendered at the exact moment
    // the user decides whether to trust it. Raw, a name carrying an
    // escape sequence could clear the screen and forge the [y/N] line.
    let show = crate::ui::strip::clean;
    for action in actions {
        // Everything inside the hash is rendered, so a re-prompt always
        // shows the user what changed.
        write!(err, "  [{}]", show(&action.name))?;
        if let Some(binding) = &action.keybinding {
            write!(err, " key={}", show(binding))?;
        }
        if action.unsafe_shell_template {
            write!(err, " UNSAFE-SHELL-TEMPLATE")?;
        }
        writeln!(err, " {}", show(&action.description))?;
        if let Some(when) = &action.when {
            writeln!(err, "    when: {}", show(&when.describe()))?;
        }
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
    if !keys.is_empty() {
        writeln!(err, "scout: [keys]:")?;
        for (op, key) in keys {
            writeln!(err, "  {} = {}", show(op), show(key))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with(body: &str) -> TrustStore {
        let dir = std::env::temp_dir().join(format!(
            "scout-trust-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("store");
        std::fs::write(&path, body).unwrap();
        TrustStore::load(path).unwrap()
    }

    #[test]
    fn a_v1_entry_reads_as_changed_and_names_its_format() {
        let store = store_with("abc123 /home/u/.config/scout/config.toml\n");
        let status = store.status(Path::new("/home/u/.config/scout/config.toml"), "abc123");
        assert_eq!(status, TrustStatus::Changed { previous: "v1 abc123".into() });
    }

    #[test]
    fn a_current_entry_is_trusted_and_a_different_hash_is_changed() {
        let store = store_with("v2 abc123 /c.toml\n");
        assert_eq!(store.status(Path::new("/c.toml"), "abc123"), TrustStatus::Trusted);
        assert_eq!(
            store.status(Path::new("/c.toml"), "def456"),
            TrustStatus::Changed { previous: "v2 abc123".into() }
        );
        assert_eq!(store.status(Path::new("/other.toml"), "abc123"), TrustStatus::New);
    }

    #[test]
    fn record_writes_the_current_format_and_keeps_other_entries() {
        let mut store = store_with("old111 /one.toml\nv2 new222 /two.toml\n");
        store.record(Path::new("/one.toml"), "fresh333").unwrap();
        let body = std::fs::read_to_string(store.store_path()).unwrap();
        // Sorted by config path; the untouched entry keeps its own version.
        assert_eq!(body, "v2 fresh333 /one.toml\nv2 new222 /two.toml\n");
        assert_eq!(store.status(Path::new("/two.toml"), "new222"), TrustStatus::Trusted);
        let _ = std::fs::remove_dir_all(store.store_path().parent().unwrap());
    }
}
