//! The one error type.
//!
//! Owns: every way scout can fail to do what it was asked, with the
//! context a user needs to act on it, and the mapping from a failure to
//! an exit code and an optional hint.
//! Refuses to know about: printing. `main` prints; nothing here does.
//! Exposes: `Error`, `Error::exit_code`, `Error::hint`.
//!
//! Exit codes follow one rule: 2 means scout was asked something it does
//! not understand (usage), an action's own code is passed through when
//! an action failed, and everything else is 1.

use std::path::PathBuf;

use crate::actions::failure::{FailureKind, Hint};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HOME is unset or empty; scout requires it")]
    HomeUnset,

    /// An I/O failure with the operation that was being attempted.
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },

    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Refused at a boundary: a symlinked database, a foreign owner, a
    /// journal mode that was not honoured. The string names what.
    #[error("refused: {0}")]
    IndexRefused(String),

    #[error("corrupt: {0}")]
    IndexCorrupt(String),

    #[error("config refused: {0}")]
    ConfigRefused(String),

    #[error("toml parse error in {path}: {message}")]
    ConfigToml { path: PathBuf, message: String },

    #[error(
        "{path}: schema_version {found} is not supported by this scout; set `schema_version = \
         {supported}`"
    )]
    ConfigSchemaVersion { path: PathBuf, found: i64, supported: i64 },

    #[error("invalid config in {path}: {message}")]
    ConfigInvalid { path: PathBuf, message: String },

    #[error("config not trusted: declined at prompt")]
    TrustDeclined,

    #[error(
        "config at {config} requires an interactive trust decision but no TTY is attached; \
         run scout interactively once, or verify hash {hash} against trust store {store}"
    )]
    TrustRequiresTty { config: PathBuf, hash: String, store: PathBuf },

    /// An action ran and failed. `step` is 1-based, as shown to the user.
    #[error("action `{action}` failed at step {step} ({kind})")]
    ActionFailed { action: String, step: usize, kind: FailureKind, exit_code: i32 },

    #[error("action `{0}` vanished from the merged set")]
    ActionVanished(String),

    #[error("unknown --format `{given}` (want {wanted})")]
    UnknownFormat { given: String, wanted: &'static str },

    #[error("the picker needs a TTY; use 'scout query <q>' for non-interactive use")]
    PickerNeedsTty,

    /// A path that would contain an existing root.
    #[error("{given} contains the indexed root {existing}; roots never nest")]
    NestedRoot { given: PathBuf, existing: PathBuf },

    /// `scout index` with no path and nothing to walk again.
    #[error("nothing indexed yet")]
    NoRoots,

    /// `[scout] tmux = "require"` and no tmux to run the session in.
    #[error("session mode requires tmux and none was found on PATH")]
    TmuxRequired,

    #[error("could not install signal handlers: {0}")]
    Signals(#[source] std::io::Error),

    #[error("ui: {0}")]
    Ui(#[source] std::io::Error),
}

impl Error {
    /// Attach an operation name to an I/O error.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Error {
        Error::Io { context: context.into(), source }
    }

    /// The process exit code this failure deserves.
    pub fn exit_code(&self) -> u8 {
        match self {
            Error::UnknownFormat { .. } | Error::PickerNeedsTty => 2,
            Error::ActionFailed { exit_code, .. } => (*exit_code).clamp(0, 255) as u8,
            _ => 1,
        }
    }

    /// What the user can do about it, when there is something to say.
    pub fn hint(&self) -> Option<Hint> {
        match self {
            Error::ActionFailed { kind, .. } => kind.hint(),
            Error::NestedRoot { existing, .. } => Some(Hint {
                why: "two roots over the same rows would fight over their generations".into(),
                next: Some(format!(
                    "index the parent instead: `scout index --forget {}` first, then index the \
                     parent",
                    existing.display()
                )),
                context: "",
            }),
            Error::TmuxRequired => Some(Hint {
                why: "[scout] tmux = \"require\" refuses a session without panes".into(),
                next: Some(
                    "install tmux, or set `tmux = \"auto\"` to run the session in this terminal"
                        .into(),
                ),
                context: "",
            }),
            Error::NoRoots => Some(Hint {
                why: "`scout index` with no path walks every known tree again".into(),
                next: Some("run `scout index <path>` once".into()),
                context: "",
            }),
            Error::ConfigSchemaVersion { found: 1, .. } => Some(Hint {
                why: "what changed in schema 2: actions may carry `when = { ... }` (optional), \
                      `[keys]` maps pane operations to keys (optional), and `[scout] session = \
                      true` is allowed"
                    .to_string(),
                next: Some(
                    "existing v1 actions are valid v2 actions unchanged; the trust prompt will \
                     appear once because the hash format changed"
                        .to_string(),
                ),
                context: "",
            }),
            _ => None,
        }
    }
}

/// A bare `?` on an I/O error reads as "io: <cause>", which is what the
/// previous per-module error types printed.
impl From<std::io::Error> for Error {
    fn from(source: std::io::Error) -> Self {
        Error::Io { context: "io".into(), source }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_errors_exit_2_action_failures_pass_their_code_everything_else_1() {
        assert_eq!(Error::UnknownFormat { given: "x".into(), wanted: "a|b" }.exit_code(), 2);
        assert_eq!(Error::PickerNeedsTty.exit_code(), 2);
        assert_eq!(
            Error::ActionFailed {
                action: "a".into(),
                step: 1,
                kind: FailureKind::ExitStatus(3),
                exit_code: 3
            }
            .exit_code(),
            3
        );
        assert_eq!(
            Error::ActionFailed {
                action: "a".into(),
                step: 1,
                kind: FailureKind::ExitStatus(300),
                exit_code: 300
            }
            .exit_code(),
            255,
            "codes clamp into the byte an exit status can carry"
        );
        assert_eq!(Error::HomeUnset.exit_code(), 1);
        assert_eq!(Error::TrustDeclined.exit_code(), 1);
    }

    #[test]
    fn a_v1_schema_refusal_says_what_to_change() {
        let err = Error::ConfigSchemaVersion { path: "c.toml".into(), found: 1, supported: 2 };
        assert!(err.to_string().contains("schema_version = 2"), "{err}");
        let hint = err.hint().expect("a v1 file gets migration advice");
        assert!(hint.next.is_some());
        // A future version, or garbage, gets no advice we cannot stand behind.
        assert!(Error::ConfigSchemaVersion { path: "c.toml".into(), found: 7, supported: 2 }
            .hint()
            .is_none());
    }

    #[test]
    fn only_action_failures_v1_refusals_and_root_errors_carry_a_hint() {
        assert!(Error::HomeUnset.hint().is_none());
        assert!(Error::NoRoots.hint().is_some());
        assert!(Error::NestedRoot { given: "/a".into(), existing: "/a/b".into() }.hint().is_some());
        let err = Error::ActionFailed {
            action: "a".into(),
            step: 1,
            kind: FailureKind::NoEditor,
            exit_code: 127,
        };
        assert!(err.hint().is_some());
    }
}
