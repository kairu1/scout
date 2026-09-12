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
pub use when::{Applicability, Kind, Mode, When, WHEN_KEYS};
pub use PaneOp as Pane;

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

/// Where a `spawn` step runs when scout is inside tmux: a new pane or
/// window at the selection, with scout staying in its own pane. Outside
/// tmux the step runs in-process instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneOp {
    SplitRight,
    SplitDown,
    NewWindow,
}

impl PaneOp {
    pub fn parse(name: &str) -> Option<PaneOp> {
        match name {
            "split-right" => Some(PaneOp::SplitRight),
            "split-down" => Some(PaneOp::SplitDown),
            "new-window" => Some(PaneOp::NewWindow),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PaneOp::SplitRight => "split-right",
            PaneOp::SplitDown => "split-down",
            PaneOp::NewWindow => "new-window",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Step {
    Spawn {
        argv: Vec<Template>,
        wait: bool,
        cwd: Option<Template>,
        /// In session mode, after a waited child exits, hold the screen
        /// with a status line until a key is pressed. Default true; an
        /// editor action sets it false. Meaningless outside a session.
        pause: bool,
        /// Run in a tmux pane or window instead of in-process.
        pane: Option<PaneOp>,
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

    /// The mode the action is limited to, if any.
    pub fn mode(&self) -> Option<Mode> {
        self.when.as_ref().and_then(|w| w.mode)
    }

    /// Whether a run in this mode offers the action at all.
    pub fn offered_in(&self, session: bool) -> bool {
        self.when.as_ref().is_none_or(|w| w.allows_mode(session))
    }

    /// Whether two actions can both be offered in some run: at least one
    /// has no mode, or their modes agree. What makes a shared name or
    /// chord ambiguous, or harmless.
    pub fn modes_overlap(&self, other: &Action) -> bool {
        match (self.mode(), other.mode()) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        }
    }

    /// An action with a `print` step only works after scout has gone (the
    /// wrapper evals what it printed), so it ends a session. Everything
    /// else returns to the picker.
    pub fn ends_session(&self) -> bool {
        self.steps.iter().any(|s| matches!(s, Step::Print { .. }))
    }

    /// Whether running this action in a session should hold the screen
    /// afterwards: some waited child asked for a pause, or the built-in
    /// editor ran (which never pauses).
    pub fn pauses(&self) -> bool {
        self.steps
            .iter()
            .any(|s| matches!(s, Step::Spawn { wait: true, pause: true, pane: None, .. }))
    }
}
