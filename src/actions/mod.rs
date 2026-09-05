//! Actions: what scout does when the user picks a row and presses a key.
//!
//! Owns: the declarative action model (`Action`, `Step`, `OnFailure`),
//! the template grammar that turns a step into argv or a printed line,
//! the compiled-in defaults, the executor, and the closed set of ways a
//! step can fail.
//! Refuses to know about: TOML, trust hashes, terminals, ranking. The
//! loader hands it a validated model; the picker hands it a selection.
//! Exposes: the model types, `execute`, `FailureKind`, `Hint`.
//!
//! The model is here rather than with the config because the config
//! produces it and the executor consumes it: a program can build an
//! `Action` in code and run it without a config file existing.

pub mod builtin;
pub mod exec;
pub mod failure;
pub mod template;
pub mod when;

pub use builtin::compiled_defaults;
pub use exec::{execute, sanitized_process_env, ActionCtx, ExecOutcome};
pub use failure::{FailureKind, Hint};
pub use template::Template;
pub use when::{Applicability, Kind, When};

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
    Spawn {
        argv: Vec<Template>,
        wait: bool,
        cwd: Option<Template>,
    },
    Print {
        format: Template,
    },
    Env {
        set: Vec<(String, Template)>,
    },
    /// The compiled-in `edit` fallback chain (`$VISUAL`, then `$EDITOR`,
    /// then a vi-family binary on `PATH`). Written in Rust because the
    /// template grammar deliberately has no way to express fallbacks.
    /// Never hashed: compiled defaults are outside the trust projection.
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
    /// Where the action is offered. `None` means everywhere.
    pub when: Option<When>,
    /// True when this action came from the on-disk config (and so was
    /// hashed); false for compiled defaults, trusted with the binary.
    pub from_user_config: bool,
}

impl Action {
    /// True when the action is offered for a selection with this
    /// applicability.
    pub fn applies(&self, a: &Applicability) -> bool {
        self.when.as_ref().is_none_or(|w| w.applies(a))
    }
}
