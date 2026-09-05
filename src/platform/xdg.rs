//! The XDG base directories, resolved by hand: `$XDG_*` when set and
//! non-empty, otherwise the documented fallback under `$HOME`. Small
//! enough that a crate would cost more than it saves.

use std::path::PathBuf;

use crate::Error;

/// `$HOME`. Required: scout refuses to start without it.
pub fn home() -> crate::Result<PathBuf> {
    match std::env::var("HOME") {
        Ok(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Err(Error::HomeUnset),
    }
}

fn xdg(var: &str, home_fallback: &str) -> crate::Result<PathBuf> {
    match std::env::var(var) {
        Ok(v) if !v.is_empty() => Ok(PathBuf::from(v)),
        _ => Ok(home()?.join(home_fallback)),
    }
}

/// `$XDG_CONFIG_HOME` or `~/.config`.
pub fn config_home() -> crate::Result<PathBuf> {
    xdg("XDG_CONFIG_HOME", ".config")
}

/// `$XDG_STATE_HOME` or `~/.local/state`.
pub fn state_home() -> crate::Result<PathBuf> {
    xdg("XDG_STATE_HOME", ".local/state")
}

/// `$XDG_DATA_HOME` or `~/.local/share`.
pub fn data_home() -> crate::Result<PathBuf> {
    xdg("XDG_DATA_HOME", ".local/share")
}
