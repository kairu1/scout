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
pub mod loader;
pub mod trust;

use std::path::PathBuf;

use crate::actions::{compiled_defaults, Action};

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
}

impl Config {
    pub fn builtin_only() -> Config {
        Config {
            actions: compiled_defaults(),
            warnings: Vec::new(),
            source: None,
            trust_hash: None,
        }
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
