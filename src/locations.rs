//! Where scout keeps its own files.
//!
//! Owns: the config discovery chain, the index database path, the trust
//! store path, the log file path. This module mirrors the "files scout
//! owns" table in the README; if one changes, the other must.
//! Refuses to know about: how `$XDG_*` is resolved (asks `platform::xdg`)
//! and what any of the files contain.
//! Exposes: one function per file.

use std::path::PathBuf;

use crate::platform::xdg;

/// Config discovery chain, in order. The first entry that opens as a
/// regular file wins; a parse error there halts rather than falling
/// through. Entry 1 exists only when `$XDG_CONFIG_HOME` is set.
pub fn discovery_chain() -> crate::Result<Vec<PathBuf>> {
    let mut chain = Vec::new();
    if let Ok(v) = std::env::var("XDG_CONFIG_HOME") {
        if !v.is_empty() {
            chain.push(PathBuf::from(v).join("scout/config.toml"));
        }
    }
    chain.push(xdg::home()?.join(".config/scout/config.toml"));
    chain.push(PathBuf::from("/etc/scout/config.toml"));
    Ok(chain)
}

/// The index and frecency store: `$XDG_DATA_HOME/scout/index.db`.
pub fn index_db() -> crate::Result<PathBuf> {
    Ok(xdg::data_home()?.join("scout/index.db"))
}

/// Hashes of the configs the user approved:
/// `$XDG_STATE_HOME/scout/trusted-config.sha256`.
pub fn trust_store() -> crate::Result<PathBuf> {
    Ok(xdg::state_home()?.join("scout/trusted-config.sha256"))
}

/// The directory the picker's log lives in.
pub fn state_dir() -> crate::Result<PathBuf> {
    Ok(xdg::state_home()?.join("scout"))
}

/// The picker's log: `$XDG_STATE_HOME/scout/scout.log`.
pub fn log_file() -> crate::Result<PathBuf> {
    Ok(state_dir()?.join("scout.log"))
}
