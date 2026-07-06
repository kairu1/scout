//! Config sector (Engineers). Shape per ADR-004; enforcement per
//! ADR-003. The loader is staged and fail-closed; the executor consumes
//! only the validated model.

pub mod canonical;
pub mod loader;
pub mod paths;
pub mod sha256;
pub mod template;
pub mod trust;

use std::path::PathBuf;

use template::Template;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFailure {
    Abort,
    Continue,
}

impl OnFailure {
    pub fn as_str(&self) -> &'static str {
        match self {
            OnFailure::Abort => "abort",
            OnFailure::Continue => "continue",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Step {
    Spawn { argv: Vec<Template>, wait: bool, cwd: Option<Template> },
    Print { format: Template },
    Env { set: Vec<(String, Template)> },
    /// Compiled-in `edit` fallback chain ($VISUAL → $EDITOR → vi-family
    /// on PATH). Rust-native because the strict placeholder grammar
    /// correctly refuses fallback logic (ADR-004 §7). Never hashed:
    /// compiled defaults are outside the trust projection.
    BuiltinEdit,
}

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub description: String,
    pub keybinding: Option<String>,
    pub on_failure: OnFailure,
    pub unsafe_shell_template: bool,
    pub steps: Vec<Step>,
    /// True when this action came from the on-disk config (hashed);
    /// false for compiled defaults (trusted with the binary).
    pub from_user_config: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Merged action set: user actions in file order, then compiled
    /// defaults that were not overridden by name (ADR-004 §7).
    pub actions: Vec<Action>,
    /// Non-fatal loader warnings (unknown keybindings etc.), for stderr.
    pub warnings: Vec<String>,
    /// The file that won discovery; None = compiled defaults only.
    pub source: Option<PathBuf>,
    /// Trust hash of the user action set; None when no file loaded.
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
    /// among user actions first, compiled defaults second (ADR-004 §6
    /// dispatch priority).
    pub fn enter_action(&self) -> Option<&Action> {
        self.actions
            .iter()
            .filter(|a| a.keybinding.as_deref() == Some("enter"))
            .max_by_key(|a| a.from_user_config)
    }
}

/// Compiled-in defaults (ADR-004 §7). Applied only where the user config
/// does not define the same `name`.
pub fn compiled_defaults() -> Vec<Action> {
    vec![
        Action {
            name: "edit".into(),
            description: "Open in $EDITOR (falls back to $VISUAL, then a vi-family binary on PATH)"
                .into(),
            keybinding: Some("enter".into()),
            on_failure: OnFailure::Abort,
            unsafe_shell_template: false,
            steps: vec![Step::BuiltinEdit],
            from_user_config: false,
        },
        Action {
            name: "print-path".into(),
            description: "Print the selection's absolute path to stdout".into(),
            keybinding: None,
            on_failure: OnFailure::Abort,
            unsafe_shell_template: false,
            // Emits a COMMAND (`printf '%s\n' '<path>'`), not a bare
            // value, so it survives the shell wrapper's eval allowlist
            // (ADR-004 §7, revised 2026-07-06). {path} is still quoted.
            steps: vec![Step::Print {
                format: Template::parse("printf '%s\\n' {path}").expect("static template"),
            }],
            from_user_config: false,
        },
    ]
}

/// `compiled_defaults ∪ user_actions`, user wins by name, whole-action
/// replacement (ADR-004 §7). User actions keep file order; surviving
/// defaults append after (menu order per §Binds-3rd-Rifles).
pub fn merge_with_defaults(user_actions: Vec<Action>) -> Vec<Action> {
    let mut merged = user_actions;
    for default in compiled_defaults() {
        if !merged.iter().any(|a| a.name == default.name) {
            merged.push(default);
        }
    }
    merged
}
