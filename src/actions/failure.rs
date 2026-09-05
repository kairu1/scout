//! The closed set of ways a step can fail, the exit code each deserves,
//! and the plain-language hint for each.
//!
//! An enum rather than strings so that the hint table is an exhaustive
//! `match`: a new kind without a hint does not compile. `Display` renders
//! the stable codes that appear in logs (`undefined_placeholder:repo_root`,
//! `spawn:entity not found`, ...), which are an interface and are pinned
//! by a test.

use super::template::ExpandError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureKind {
    /// `{ext}` on a directory, `{repo_root}` with no `.git` ancestor.
    UndefinedPlaceholder(String),
    /// `{env.X}` with no binding from an earlier `env` step in this action.
    UndefinedEnv(String),
    /// NUL or newline inside a value at the print seam.
    HazardousPath,
    /// A filesystem error while resolving `{ext}` or `{repo_root}`.
    PathResolution,
    /// The compiled-in editor chain found nothing.
    NoEditor,
    /// stdout closed under a `print` step.
    PrintWrite,
    /// The child ran and exited non-zero (or was killed).
    ExitStatus(i32),
    /// The child could not be started.
    Spawn(std::io::ErrorKind),
    /// The failure happened while resolving the step's working directory.
    Cwd(Box<FailureKind>),
}

impl From<ExpandError> for FailureKind {
    fn from(err: ExpandError) -> Self {
        match err {
            ExpandError::UndefinedPlaceholder(which) => FailureKind::UndefinedPlaceholder(which),
            ExpandError::UndefinedEnv(name) => FailureKind::UndefinedEnv(name),
            ExpandError::HazardousPath => FailureKind::HazardousPath,
            ExpandError::PathResolution(_) => FailureKind::PathResolution,
        }
    }
}

impl std::fmt::Display for FailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FailureKind::UndefinedPlaceholder(which) => write!(f, "undefined_placeholder:{which}"),
            FailureKind::UndefinedEnv(name) => write!(f, "undefined_env:{name}"),
            FailureKind::HazardousPath => write!(f, "hazardous_path"),
            FailureKind::PathResolution => write!(f, "path"),
            FailureKind::NoEditor => write!(f, "no_editor"),
            FailureKind::PrintWrite => write!(f, "print_write"),
            FailureKind::ExitStatus(_) => write!(f, "exit_status"),
            FailureKind::Spawn(kind) => write!(f, "spawn:{kind}"),
            FailureKind::Cwd(inner) => write!(f, "cwd:{inner}"),
        }
    }
}

impl FailureKind {
    /// The exit code a chain that aborts on this failure reports.
    pub fn exit_code(&self) -> i32 {
        match self {
            FailureKind::NoEditor => 127,
            FailureKind::Spawn(std::io::ErrorKind::NotFound) => 127,
            FailureKind::Spawn(_) => 126,
            FailureKind::ExitStatus(code) => *code,
            _ => 1,
        }
    }

    /// What went wrong and what to do about it. `None` for nothing.
    pub fn hint(&self) -> Option<Hint> {
        let (context, inner) = match self {
            FailureKind::Cwd(inner) => {
                (" (while resolving the action's working directory)", inner.as_ref())
            }
            other => ("", other),
        };
        let (why, next): (String, Option<String>) = match inner {
            FailureKind::UndefinedPlaceholder(name) if name == "repo_root" => (
                "the selection is not inside a git repository, so {repo_root} has nothing to \
                 resolve to"
                    .to_string(),
                // This failure is a correct refusal, so the way out of it
                // is the whole of what the user needs from the message.
                Some(
                    "pick a path under a repository, or run an action built on {path} or {parent} \
                     instead"
                        .to_string(),
                ),
            ),
            FailureKind::UndefinedPlaceholder(name) => (
                format!("`{{{name}}}` could not be resolved for this selection"),
                Some(
                    "choose a selection it applies to, or edit the action to use a placeholder \
                     that always resolves"
                        .to_string(),
                ),
            ),
            FailureKind::UndefinedEnv(name) => (
                format!(
                    "`{{env.{name}}}` resolves only against an `env` step in the same action, \
                     never your shell's environment"
                ),
                Some(format!(
                    "add that step, or write `${name}` in a `print` template and let your shell \
                     expand it"
                )),
            ),
            FailureKind::NoEditor => (
                "no editor could be resolved".to_string(),
                Some("set $EDITOR or $VISUAL, or install a vi-family editor on PATH".to_string()),
            ),
            FailureKind::PathResolution => (
                "the selection's path is not valid UTF-8".to_string(),
                Some("rename it, or pick another selection".to_string()),
            ),
            FailureKind::PrintWrite => (
                "scout could not write to stdout".to_string(),
                Some("check that whatever you piped scout into is still reading".to_string()),
            ),
            FailureKind::ExitStatus(_) => (
                "the command ran and returned a non-zero status".to_string(),
                Some("run it yourself to see why".to_string()),
            ),
            FailureKind::Spawn(kind) => (
                format!("the command could not be started ({kind})"),
                Some("check that it exists on PATH".to_string()),
            ),
            FailureKind::HazardousPath => (
                "the path contains NUL or newline and cannot be passed to a shell safely"
                    .to_string(),
                // Deliberately no next step: nothing the user does at the
                // picker changes this, and inventing advice would be worse
                // than admitting there is none.
                None,
            ),
            // A nested Cwd is unreachable by construction; treat as inner.
            FailureKind::Cwd(_) => return inner.hint(),
        };
        Some(Hint { why, next, context })
    }
}

/// Plain-language explanation of a failure, and what to do about it.
///
/// `why` and `next` are separate fields because a test cannot check
/// prose: a guard that greps its own subject's wording measures the easy
/// half. `next: Option<_>` is a contract a test can read.
#[derive(Debug, Clone)]
pub struct Hint {
    /// What went wrong, and why.
    pub why: String,
    /// What the user can do about it. `None` only where nothing they can
    /// do would change the outcome.
    pub next: Option<String>,
    /// Where it happened, when the failure arrived wrapped.
    pub context: &'static str,
}

impl std::fmt::Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.why)?;
        if let Some(next) = &self.next {
            write!(f, "; {next}")?;
        }
        // The context trails in parentheses so it never lands between the
        // advice and its verb.
        write!(f, "{}", self.context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    fn every_kind() -> Vec<FailureKind> {
        vec![
            FailureKind::UndefinedPlaceholder("repo_root".into()),
            FailureKind::UndefinedPlaceholder("ext".into()),
            FailureKind::UndefinedEnv("TOKEN".into()),
            FailureKind::HazardousPath,
            FailureKind::PathResolution,
            FailureKind::NoEditor,
            FailureKind::PrintWrite,
            FailureKind::ExitStatus(3),
            FailureKind::Spawn(ErrorKind::NotFound),
            FailureKind::Spawn(ErrorKind::PermissionDenied),
            FailureKind::Cwd(Box::new(FailureKind::UndefinedPlaceholder("repo_root".into()))),
            FailureKind::Cwd(Box::new(FailureKind::UndefinedEnv("HOME".into()))),
            FailureKind::Cwd(Box::new(FailureKind::PathResolution)),
        ]
    }

    /// The rendered codes are a log interface; pin them.
    #[test]
    fn every_failure_kind_renders_its_stable_code() {
        let rendered: Vec<String> = every_kind().iter().map(|k| k.to_string()).collect();
        assert_eq!(
            rendered,
            vec![
                "undefined_placeholder:repo_root",
                "undefined_placeholder:ext",
                "undefined_env:TOKEN",
                "hazardous_path",
                "path",
                "no_editor",
                "print_write",
                "exit_status",
                "spawn:entity not found",
                "spawn:permission denied",
                "cwd:undefined_placeholder:repo_root",
                "cwd:undefined_env:HOME",
                "cwd:path",
            ]
        );
    }

    #[test]
    fn exit_codes_follow_the_shell_convention() {
        assert_eq!(FailureKind::NoEditor.exit_code(), 127);
        assert_eq!(FailureKind::Spawn(ErrorKind::NotFound).exit_code(), 127);
        assert_eq!(FailureKind::Spawn(ErrorKind::PermissionDenied).exit_code(), 126);
        assert_eq!(FailureKind::ExitStatus(42).exit_code(), 42);
        assert_eq!(FailureKind::HazardousPath.exit_code(), 1);
        assert_eq!(FailureKind::Cwd(Box::new(FailureKind::NoEditor)).exit_code(), 1);
    }

    #[test]
    fn a_wrapped_reason_keeps_the_inner_explanation_and_says_where() {
        let hint =
            FailureKind::Cwd(Box::new(FailureKind::UndefinedPlaceholder("repo_root".into())))
                .hint()
                .unwrap()
                .to_string();
        assert!(hint.contains("git repository"), "{hint}");
        assert!(hint.contains("working directory"), "{hint}");
    }

    /// A hint that stops at the diagnosis leaves the user where they were.
    /// This reads the `next` field rather than grepping the sentence: an
    /// earlier version asserted the `repo_root` hint contained "select"
    /// and passed against a hint with no next step at all, because
    /// "the **select**ion is not inside a git repository" contains it.
    #[test]
    fn hints_for_actionable_failures_carry_a_next_step() {
        for kind in every_kind() {
            if kind == FailureKind::HazardousPath {
                continue;
            }
            let hint = kind.hint().unwrap_or_else(|| panic!("no hint for {kind}"));
            let next = hint
                .next
                .as_ref()
                .unwrap_or_else(|| panic!("`{kind}` never says what to do next: {hint}"));
            assert!(!next.trim().is_empty(), "`{kind}` has an empty next step");
            assert!(hint.to_string().contains(next.as_str()), "next step lost in rendering");
        }
    }

    /// The converse, so `next` cannot quietly become a field that is
    /// always `Some` and therefore proves nothing.
    #[test]
    fn a_failure_the_user_cannot_act_on_offers_no_next_step() {
        assert!(FailureKind::HazardousPath.hint().unwrap().next.is_none());
    }
}
