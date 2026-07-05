//! Hand-rolled XDG resolver (ADR-002 §Dirs resolution — `dirs` refused).
//! POSIX fallbacks documented inline; Windows is out of scope for v1.

use std::path::PathBuf;

/// `$HOME`, required at startup (ADR-004 §8: refuse when unset).
pub fn home() -> Result<PathBuf, String> {
    match std::env::var("HOME") {
        Ok(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Err("HOME is unset or empty; scout requires it".into()),
    }
}

fn xdg(var: &str, home_fallback: &str) -> Result<PathBuf, String> {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Ok(home()?.join(home_fallback)),
    }
}

/// `$XDG_CONFIG_HOME` or `~/.config`.
pub fn config_home() -> Result<PathBuf, String> {
    xdg("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_STATE_HOME` or `~/.local/state`.
pub fn state_home() -> Result<PathBuf, String> {
    xdg("XDG_STATE_HOME", ".local/state")
}

/// `$XDG_DATA_HOME` or `~/.local/share`.
pub fn data_home() -> Result<PathBuf, String> {
    xdg("XDG_DATA_HOME", ".local/share")
}

/// Default index DB location.
pub fn default_db_path() -> Result<PathBuf, String> {
    Ok(data_home()?.join("scout/index.db"))
}

/// Trust store path (ADR-003 §3).
pub fn trust_store_path() -> Result<PathBuf, String> {
    Ok(state_home()?.join("scout/trusted-config.sha256"))
}

/// Config discovery chain, in commander-pinned order (ADR-004 §8).
/// Entry 1 exists only when `$XDG_CONFIG_HOME` is set and non-empty.
pub fn discovery_chain() -> Result<Vec<PathBuf>, String> {
    let mut chain = Vec::new();
    if let Ok(v) = std::env::var("XDG_CONFIG_HOME") {
        if !v.is_empty() {
            chain.push(PathBuf::from(v).join("scout/config.toml"));
        }
    }
    chain.push(home()?.join(".config/scout/config.toml"));
    chain.push(PathBuf::from("/etc/scout/config.toml"));
    Ok(chain)
}
