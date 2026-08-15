//! Action executor (Engineers). Sequential-only chains, argv-level
//! execvp, the two shell seams, all-or-nothing env landing, and the
//! first-success-wins visit credit with the 10 s per-path rate limit
//! (ADR-001 §Visit credit, ADR-003 §2/§5, ADR-004 §3/§5).

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::config::template::{ExpandCtx, ExpandError};
use crate::config::{Action, OnFailure, Step};

/// One credit per (path, 10 s) window (ADR-001 §Rate limit — enforced
/// here, in the executor, not in the DB).
const CREDIT_WINDOW: Duration = Duration::from_secs(10);

static LAST_CREDIT: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub struct ActionCtx {
    /// Canonical absolute path of the selected candidate.
    pub path: PathBuf,
    /// Query buffer at dispatch time (may be empty — valid).
    pub query: String,
    pub home: String,
}

#[derive(Debug)]
pub struct ExecOutcome {
    pub any_success: bool,
    /// Why the first failing step failed, and which step it was. The
    /// reason was previously written only to the log — and the log is
    /// written in picker mode, so a user whose action did nothing had
    /// no way to find out why. Carried out so the caller can say it.
    pub failure: Option<(usize, String)>,
    /// Chain exit code (ADR-004 §5): 0 if any step succeeded; first
    /// failing step's code under abort; 2 under continue if all failed.
    pub exit_code: i32,
    pub credited: bool,
    pub steps_run: usize,
}

/// Execute `action` against `ctx`. `visit` carries the index connection
/// and candidate rowid for the credit hook; None skips crediting (e.g.
/// dry runs).
pub fn execute(action: &Action, ctx: &ActionCtx, visit: Option<(&Connection, i64)>) -> ExecOutcome {
    // Two maps, because `scope` was serving two contracts that pull in
    // opposite directions.
    //
    // `child_env` is what a spawned process inherits: the sanitized
    // process environment plus anything an `env` step sets. A child
    // without PATH or HOME is useless.
    //
    // `bindings` is what `{env.NAME}` resolves against, and it starts
    // EMPTY. ADR-003 §129 is explicit: a reference to a binding whose
    // setter step failed "does not expand to the shell's own environment
    // variable by the same name". Seeding it from the process
    // environment made exactly that silent fallback the default — a
    // failed `env` step resolved to whatever the user happened to have
    // exported, which is the substitution the threat model refuses.
    let mut child_env = sanitized_process_env();
    let mut bindings: HashMap<String, String> = HashMap::new();
    let mut any_success = false;
    let mut credited = false;
    let mut first_fail_code: Option<i32> = None;
    let mut failure: Option<(usize, String)> = None;
    let mut steps_run = 0;

    for (index, step) in action.steps.iter().enumerate() {
        steps_run += 1;
        let result = run_step(step, ctx, &mut child_env, &mut bindings);
        match result {
            Ok(()) => {
                if !any_success {
                    any_success = true;
                    // First success wins; later steps never re-credit,
                    // and a later abort does not retract (ADR-004 §5).
                    if let Some((conn, id)) = visit {
                        credited = credit_visit(conn, id, &ctx.path);
                    }
                }
            }
            Err((kind, code)) => {
                tracing::warn!(
                    action = %action.name,
                    step_index = index,
                    kind = %kind,
                    "action.failed"
                );
                if first_fail_code.is_none() {
                    first_fail_code = Some(code);
                    failure = Some((index, kind.clone()));
                }
                if action.on_failure == OnFailure::Abort {
                    break;
                }
            }
        }
    }

    let exit_code = if any_success {
        0
    } else if action.on_failure == OnFailure::Abort {
        first_fail_code.unwrap_or(1)
    } else {
        2
    };
    ExecOutcome { any_success, failure, exit_code, credited, steps_run }
}

/// Plain-language explanation of a failure reason, and what to do about
/// it. `None` when there is nothing useful to say at all.
///
/// `why` and `next` are separate fields rather than one sentence
/// *because a test cannot check prose*. The first version of the
/// next-step guard asserted that the `repo_root` hint contained the word
/// "select" — and it passed against the hint that gave no next step,
/// because "the **select**ion is not inside a git repository" contains
/// it. A guard that greps its own subject's wording measures the easy
/// half. `next: Option<_>` is a contract the guard can actually read.
#[derive(Debug, Clone)]
pub struct Hint {
    /// What went wrong, and why.
    pub why: String,
    /// What the user can do about it. `None` only where nothing they can
    /// do would change the outcome.
    pub next: Option<String>,
    /// Where it happened, when the reason arrived wrapped.
    pub context: &'static str,
}

impl std::fmt::Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.why)?;
        if let Some(next) = &self.next {
            write!(f, "; {next}")?;
        }
        // The context trails in parentheses rather than sitting
        // mid-sentence, so it never lands between the advice and its
        // verb.
        write!(f, "{}", self.context)
    }
}

/// Lives beside `fail_kind`, which produces the codes, because the two
/// are one contract. It previously sat private in the *binary* crate,
/// where no test could reach it — and it had drifted: `cwd:` prefixed
/// reasons never matched, and `undefined_env:` had no arm at all.
pub fn failure_hint(reason: &str) -> Option<Hint> {
    // A cwd failure wraps the underlying reason (`cwd:undefined_...`).
    // Strip the wrapper so the inner reason is still recognised.
    let (context, inner) = match reason.strip_prefix("cwd:") {
        Some(rest) => (" (while resolving the action's working directory)", rest),
        None => ("", reason),
    };
    let (why, next): (String, Option<String>) = match inner {
        "undefined_placeholder:repo_root" => (
            "the selection is not inside a git repository, so {repo_root} has nothing to \
             resolve to"
                .to_string(),
            // This failure is a correct refusal, so the way out of it is
            // the whole of what the user needs from this message.
            Some(
                "pick a path under a repository, or run an action built on {path} or {parent} \
                 instead"
                    .to_string(),
            ),
        ),
        r if r.starts_with("undefined_placeholder:") => {
            let name = r.split_once(':').map(|(_, n)| n).unwrap_or(r);
            (
                format!("`{{{name}}}` could not be resolved for this selection"),
                Some(
                    "choose a selection it applies to, or edit the action to use a placeholder \
                     that always resolves"
                        .to_string(),
                ),
            )
        }
        r if r.starts_with("undefined_env:") => {
            let name = r.split_once(':').map(|(_, n)| n).unwrap_or(r);
            (
                format!(
                    "`{{env.{name}}}` resolves only against an `env` step in the same action, \
                     never your shell's environment"
                ),
                Some(format!(
                    "add that step, or write `${name}` in a `print` template and let your shell \
                     expand it"
                )),
            )
        }
        "no_editor" => (
            "no editor could be resolved".to_string(),
            Some("set $EDITOR or $VISUAL, or install a vi-family editor on PATH".to_string()),
        ),
        "path" => (
            "the selection's path is not valid UTF-8".to_string(),
            Some("rename it, or pick another selection".to_string()),
        ),
        "print_write" => (
            "scout could not write to stdout".to_string(),
            Some("check that whatever you piped scout into is still reading".to_string()),
        ),
        "exit_status" => (
            "the command ran and returned a non-zero status".to_string(),
            Some("run it yourself to see why".to_string()),
        ),
        r if r.starts_with("spawn:") => {
            let detail = r.split_once(':').map(|(_, d)| d).unwrap_or(r);
            (
                format!("the command could not be started ({detail})"),
                Some("check that it exists on PATH".to_string()),
            )
        }
        "hazardous_path" => (
            "the path contains NUL or newline and cannot be passed to a shell safely".to_string(),
            // Deliberately no next step: nothing the user does at the
            // picker changes this, and inventing advice would be worse
            // than admitting there is none.
            None,
        ),
        _ => return None,
    };
    Some(Hint { why, next, context })
}

/// Run one step. Err carries (failure kind for tracing, exit code).
fn run_step(
    step: &Step,
    ctx: &ActionCtx,
    child_env: &mut HashMap<String, String>,
    bindings: &mut HashMap<String, String>,
) -> Result<(), (String, i32)> {
    // `bindings`, not the process environment (ADR-003 §129).
    let expand_ctx =
        ExpandCtx { path: &ctx.path, query: &ctx.query, home: &ctx.home, env: bindings };
    match step {
        Step::Spawn { argv, wait, cwd } => {
            let mut expanded = Vec::with_capacity(argv.len());
            for template in argv {
                expanded.push(template.expand(&expand_ctx, false).map_err(expand_fail)?);
            }
            let cwd = match cwd {
                Some(template) => {
                    let raw = template
                        .expand(&expand_ctx, false)
                        .map_err(|e| (format!("cwd:{}", fail_kind(&e)), 1))?;
                    let p = PathBuf::from(raw);
                    if p.is_absolute() {
                        p
                    } else {
                        Path::new(&ctx.home).join(p)
                    }
                }
                None => PathBuf::from(&ctx.home),
            };
            spawn(&expanded, *wait, &cwd, child_env)
        }
        Step::BuiltinEdit => {
            // The inherited environment, not the step bindings: $EDITOR
            // is something the user exports, not something an action
            // sets.
            let editor = resolve_editor(child_env).ok_or_else(|| ("no_editor".to_string(), 127))?;
            let argv = vec![editor, ctx.path.display().to_string()];
            spawn(&argv, true, Path::new(&ctx.home), child_env)
        }
        Step::Print { format } => {
            let line = format.expand(&expand_ctx, true).map_err(expand_fail)?;
            let mut out = std::io::stdout().lock();
            out.write_all(line.as_bytes())
                .and_then(|_| out.write_all(b"\n"))
                .and_then(|_| out.flush())
                .map_err(|_| ("print_write".to_string(), 1))
        }
        Step::Env { set } => {
            // All-or-nothing landing (ADR-003 §5, ADR-004 §3): evaluate
            // every value before any binding lands.
            let mut staged = Vec::with_capacity(set.len());
            for (name, template) in set {
                staged.push((
                    name.clone(),
                    template.expand(&expand_ctx, false).map_err(expand_fail)?,
                ));
            }
            for (name, value) in staged {
                // A binding is both visible to later templates and
                // exported to later children.
                child_env.insert(name.clone(), value.clone());
                bindings.insert(name, value);
            }
            Ok(())
        }
    }
}

fn expand_fail(err: ExpandError) -> (String, i32) {
    (fail_kind(&err), 1)
}

fn fail_kind(err: &ExpandError) -> String {
    match err {
        ExpandError::UndefinedPlaceholder(which) => format!("undefined_placeholder:{which}"),
        ExpandError::UndefinedEnv(name) => format!("undefined_env:{name}"),
        ExpandError::HazardousPath => "hazardous_path".into(),
        ExpandError::PathResolution(_) => "path".into(),
    }
}

fn spawn(
    argv: &[String],
    wait: bool,
    cwd: &Path,
    scope: &HashMap<String, String>,
) -> Result<(), (String, i32)> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).env_clear().envs(scope).current_dir(cwd);
    if wait {
        match command.status() {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(("exit_status".into(), status.code().unwrap_or(1))),
            Err(err) => {
                let code = if err.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 };
                Err((format!("spawn:{}", err.kind()), code))
            }
        }
    } else {
        // Detached: own process group + null stdio so the child cannot
        // wedge the controlling terminal (ADR-003 §2; setsid semantics
        // approximated with std's process_group — no libc on the roster).
        command.process_group(0).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        match command.spawn() {
            Ok(_child) => Ok(()),
            Err(err) => {
                let code = if err.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 };
                Err((format!("spawn:{}", err.kind()), code))
            }
        }
    }
}

/// $VISUAL → $EDITOR → first of vi/vim/nano on the sanitised PATH
/// (ADR-004 §7 — compiled-in fallback the template grammar refuses).
fn resolve_editor(scope: &HashMap<String, String>) -> Option<String> {
    for var in ["VISUAL", "EDITOR"] {
        if let Some(v) = scope.get(var) {
            if !v.is_empty() {
                return Some(v.clone());
            }
        }
    }
    let path = scope.get("PATH").cloned().unwrap_or_default();
    for candidate in ["vi", "vim", "nano"] {
        for dir in path.split(':') {
            if dir.is_empty() {
                continue;
            }
            let full = Path::new(dir).join(candidate);
            if full.is_file() {
                return Some(candidate.to_string());
            }
        }
    }
    None
}

/// Environment a spawned child inherits: the process env with PATH
/// stripped of `.` and
/// empty entries (ADR-003 §6). Secrets are deliberately NOT stripped
/// (ADR-003 §1).
pub fn sanitized_process_env() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    if let Some(path) = env.get("PATH") {
        let sanitized: Vec<&str> =
            path.split(':').filter(|entry| !entry.is_empty() && *entry != ".").collect();
        env.insert("PATH".into(), sanitized.join(":"));
    }
    env
}

fn credit_visit(conn: &Connection, candidate_id: i64, path: &Path) -> bool {
    let key = path.display().to_string();
    let mut last = LAST_CREDIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(previous) = last.get(&key) {
        if previous.elapsed() < CREDIT_WINDOW {
            return false;
        }
    }
    match crate::index::visit::record_visit(conn, candidate_id) {
        Ok(true) => {
            last.insert(key, Instant::now());
            true
        }
        Ok(false) => false,
        Err(err) => {
            tracing::warn!(%err, "visit credit failed");
            false
        }
    }
}

#[cfg(test)]
mod hint_tests {
    use super::failure_hint;

    /// Every reason this module PRODUCES must reach an arm.
    ///
    /// The producer set is scanned out of the source rather than written
    /// here by hand. The previous version was a hardcoded list, and it
    /// passed while four produced reasons — `path`, `print_write`,
    /// `exit_status` and `spawn:<kind>` — had no arm at all, including
    /// the two commonest real failures: a command not on PATH, and a
    /// command that exits non-zero. A parity guard carrying its own copy
    /// of the thing it checks is not a parity guard.
    #[test]
    fn every_produced_reason_shape_has_a_hint() {
        let source = include_str!("mod.rs");
        // (marker in the source, a representative reason it produces)
        let producers = [
            ("\"no_editor\".to_string()", "no_editor"),
            ("\"print_write\"", "print_write"),
            ("\"exit_status\"", "exit_status"),
            ("\"path\".into()", "path"),
            ("\"hazardous_path\".into()", "hazardous_path"),
            ("undefined_placeholder:{which}", "undefined_placeholder:repo_root"),
            ("undefined_env:{name}", "undefined_env:SOME_VAR"),
            ("spawn:{", "spawn:entity not found"),
            ("cwd:{}", "cwd:undefined_placeholder:repo_root"),
        ];
        let mut seen = 0usize;
        for (marker, sample) in producers {
            if !source.contains(marker) {
                continue;
            }
            seen += 1;
            assert!(failure_hint(sample).is_some(), "no hint for produced reason `{sample}`");
        }
        assert!(
            seen >= 8,
            "the producer scan matched only {seen} markers; it has lost track of the source and \
             is no longer guarding anything"
        );
        // Wrapped forms, which the plain scan cannot see.
        for wrapped in ["cwd:undefined_env:HOME", "cwd:path"] {
            assert!(failure_hint(wrapped).is_some(), "no hint for `{wrapped}`");
        }
    }

    #[test]
    fn a_wrapped_reason_keeps_the_inner_explanation_and_says_where() {
        let hint = failure_hint("cwd:undefined_placeholder:repo_root").unwrap().to_string();
        assert!(hint.contains("git repository"), "{hint}");
        assert!(hint.contains("working directory"), "{hint}");
    }

    #[test]
    fn an_unknown_reason_yields_no_hint_rather_than_a_wrong_one() {
        assert!(failure_hint("something_new").is_none());
    }

    /// A hint that stops at the diagnosis leaves the user where they
    /// were. The reported case — `edit-repo` on a selection outside any
    /// repository — printed what and why and then stopped, so the
    /// reasonable next question ("then what do I press?") had no answer
    /// anywhere in the output.
    ///
    /// This reads `next`, the field, rather than grepping the rendered
    /// sentence. The first version of this test *did* grep it, asserting
    /// the `repo_root` hint contained "select" — and passed against the
    /// hint that offered no next step at all, because "the **select**ion
    /// is not inside a git repository" contains that substring. It was
    /// written to catch exactly the defect it then failed to catch.
    #[test]
    fn hints_for_actionable_failures_carry_a_next_step() {
        let actionable = [
            "undefined_placeholder:repo_root",
            "undefined_placeholder:ext",
            "undefined_env:TOKEN",
            "no_editor",
            "path",
            "print_write",
            "exit_status",
            "spawn:entity not found",
            // Wrapped, because the wrapper must not swallow the advice.
            "cwd:undefined_placeholder:repo_root",
        ];
        for reason in actionable {
            let hint = failure_hint(reason).expect("actionable reason must have a hint");
            let next = hint
                .next
                .as_ref()
                .unwrap_or_else(|| panic!("`{reason}` never says what to do next: {hint}"));
            assert!(!next.trim().is_empty(), "`{reason}` has an empty next step");
            // And the rendered line must actually carry it, so a future
            // Display change cannot drop the half that matters.
            assert!(hint.to_string().contains(next.as_str()), "next step lost in rendering");
        }
    }

    /// The converse, so `next` cannot quietly become a field that is
    /// always `Some` and therefore proves nothing: a failure the user
    /// cannot act on says so by omission rather than by inventing advice.
    #[test]
    fn a_failure_the_user_cannot_act_on_offers_no_next_step() {
        assert!(failure_hint("hazardous_path").unwrap().next.is_none());
    }
}

#[cfg(test)]
mod editor_tests {
    use super::resolve_editor;
    use std::collections::HashMap;

    /// `BuiltinEdit` looks its editor up in the map a spawned child
    /// inherits, not in the `env`-step bindings — the split in
    /// `execute` would otherwise have made the built-in editor
    /// unresolvable on every machine.
    ///
    /// This tests the LOOKUP and never spawns anything. The integration
    /// test that preceded it ran `BuiltinEdit` for real against whatever
    /// editor the machine offered: on a box with no vi-family binary it
    /// passed in milliseconds by taking the "no editor" branch, and on a
    /// CI runner it launched vim, which opened a directory listing and
    /// blocked until the six-hour job limit. A test whose behaviour
    /// depends on which binaries the host happens to have is not a test
    /// of this code.
    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn visual_wins_then_editor() {
        assert_eq!(
            resolve_editor(&env(&[("VISUAL", "code"), ("EDITOR", "vi")])).as_deref(),
            Some("code")
        );
        assert_eq!(resolve_editor(&env(&[("EDITOR", "vi")])).as_deref(), Some("vi"));
    }

    /// An exported-but-empty variable is not a choice.
    #[test]
    fn empty_values_are_skipped() {
        assert_eq!(
            resolve_editor(&env(&[("VISUAL", ""), ("EDITOR", "nano")])).as_deref(),
            Some("nano")
        );
    }

    /// The PATH fallback finds a vi-family binary without consulting the
    /// process environment.
    #[test]
    fn falls_back_to_a_vi_family_binary_on_path() {
        let dir = std::env::temp_dir().join(format!("scout-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("vim");
        std::fs::write(&fake, "#!/bin/sh\nexit 0\n").unwrap();

        let path = dir.display().to_string();
        assert_eq!(resolve_editor(&env(&[("PATH", &path)])).as_deref(), Some("vim"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nothing_resolvable_is_none() {
        assert!(resolve_editor(&env(&[("PATH", "/nonexistent-dir-for-scout-test")])).is_none());
        assert!(resolve_editor(&env(&[])).is_none());
    }
}
