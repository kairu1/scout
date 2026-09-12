//! The config file: finding it, validating it, trusting it.
//!
//! Owns: the staged loader (discover, open without following symlinks,
//! cap, parse, check the schema version, validate every action, hash,
//! trust-prompt, merge with compiled defaults), the canonical projection
//! the trust hash is computed over, and the trust store.
//! Refuses to know about: executing anything, drawing anything. It
//! produces a `Config` and stops.
//! Exposes: `Config`, `load`, `load_file`, `discover`, the canonical
//! projection and the trust store.

pub mod canonical;
pub mod keys;
pub mod loader;
pub mod trust;

use std::path::PathBuf;

use crate::actions::{compiled_defaults, Action};

pub use keys::Keys;
pub use loader::{discover, load, load_file};

#[derive(Debug, Clone)]
pub struct Config {
    /// Merged action set: user actions in file order, then compiled
    /// defaults that were not overridden by name.
    pub actions: Vec<Action>,
    /// Non-fatal loader warnings (unknown keybindings and the like), for
    /// the caller to print.
    pub warnings: Vec<String>,
    /// The file that won discovery; `None` means compiled defaults only.
    pub source: Option<PathBuf>,
    /// Trust hash of the user action set; `None` when no file loaded.
    pub trust_hash: Option<String>,
    /// `[scout] session = true`: stay in the picker after an action.
    pub session: bool,
    /// `[scout] tmux`: whether a session may start tmux.
    pub tmux: crate::tmux::Policy,
    /// `[scout] tmux_session`: the session scout starts or attaches to.
    pub tmux_session: String,
    /// `[scout] tmux_server`: scout's own server or the user's default.
    pub tmux_server: crate::tmux::Server,
    /// Pane operations and the re-index key, resolved over the defaults.
    pub keys: Keys,
}

impl Config {
    pub fn builtin_only() -> Config {
        Config {
            actions: compiled_defaults(),
            warnings: Vec::new(),
            source: None,
            trust_hash: None,
            session: false,
            tmux: crate::tmux::Policy::default(),
            tmux_session: crate::tmux::DEFAULT_SESSION.to_string(),
            tmux_server: crate::tmux::Server::default(),
            keys: Keys::default(),
        }
    }

    /// The same config with only the actions a run in this mode offers.
    /// Settled once, before the picker draws: after this, names and
    /// chords are unique again (the loader allows sharing only across
    /// disjoint modes), so the picker's tables need no mode of their own.
    pub fn for_mode(&self, session: bool) -> Config {
        let mut config = self.clone();
        config.actions.retain(|a| a.offered_in(session));
        config
    }

    /// The action Enter dispatches: the unique `keybinding = "enter"`
    /// among user actions first, compiled defaults second.
    pub fn enter_action(&self) -> Option<&Action> {
        self.actions
            .iter()
            .filter(|a| a.keybinding.as_deref() == Some("enter"))
            .max_by_key(|a| a.from_user_config)
    }
}

/// User actions first in file order, then every compiled default whose
/// name no user action claimed. Whole-action replacement by name.
pub fn merge_with_defaults(user_actions: Vec<Action>) -> Vec<Action> {
    let mut merged = user_actions;
    for default in compiled_defaults() {
        if !merged.iter().any(|a| a.name == default.name) {
            merged.push(default);
        }
    }
    merged
}
