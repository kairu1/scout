//! The executor: run an action's steps in order against a selection,
//! land `env` bindings all-or-nothing, credit the first success, and
//! report how the chain ended.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::failure::FailureKind;
use super::template::ExpandCtx;
use super::{builtin, Action, OnFailure, PaneOp, Step};
use crate::platform::process;
use crate::platform::time::unix_now;

/// Runs a step's argv in a tmux pane at a directory. Supplied by the
/// session when scout is inside tmux; absent, a `pane` step runs
/// in-process instead.
pub type PaneRunner<'a> = dyn Fn(PaneOp, &Path, &[String]) -> Result<(), String> + 'a;

pub struct ActionCtx<'a> {
    /// Canonical absolute path of the selected candidate.
    pub path: PathBuf,
    /// Query buffer at dispatch time (may be empty, which is valid).
    pub query: String,
    pub home: String,
    /// Where `print` steps write. `None` is stdout, the one-shot contract
    /// `eval "$(scout)"` relies on; the wrapper passes a file so that a
    /// session's children keep stdout for themselves.
    pub print_to: Option<PathBuf>,
    pub pane_runner: Option<&'a PaneRunner<'a>>,
}

#[derive(Debug)]
pub struct ExecOutcome {
    pub any_success: bool,
    /// The first failing step (0-based) and why it failed. Carried out so
    /// the caller can say what happened; a reason that only reaches a log
    /// file is a reason the user never sees.
    pub failure: Option<(usize, FailureKind)>,
    /// Chain exit code: 0 if any step succeeded; the first failing step's
    /// code under `abort`; 2 under `continue` when every step failed.
    pub exit_code: i32,
    pub credited: bool,
    pub steps_run: usize,
}

/// Execute `action` against `ctx`. `visit` carries the index connection
/// and candidate rowid for the credit hook; `None` skips crediting.
pub fn execute(action: &Action, ctx: &ActionCtx, visit: Option<(&Connection, i64)>) -> ExecOutcome {
    // Two maps, because they serve two contracts that pull in opposite
    // directions. `child_env` is what a spawned process inherits: the
    // sanitised process environment plus anything an `env` step sets.
    // `bindings` is what `{env.NAME}` resolves against, and it starts
    // EMPTY: a reference to a binding whose setter step failed must not
    // fall back to whatever the user happened to export.
    let mut child_env = sanitized_process_env();
    let mut bindings: HashMap<String, String> = HashMap::new();
    let mut any_success = false;
    let mut credited = false;
    let mut first_fail_code: Option<i32> = None;
    let mut failure: Option<(usize, FailureKind)> = None;
    let mut steps_run = 0;

    for (index, step) in action.steps.iter().enumerate() {
        steps_run += 1;
        match run_step(step, ctx, &mut child_env, &mut bindings) {
            Ok(()) => {
                if !any_success {
                    any_success = true;
                    // First success wins; later steps never re-credit, and
                    // a later abort does not retract.
                    if let Some((conn, id)) = visit {
                        credited = credit_visit(conn, id);
                    }
                }
            }
            Err(kind) => {
                tracing::warn!(
                    action = %action.name,
                    step_index = index,
                    kind = %kind,
                    "action.failed"
                );
                if first_fail_code.is_none() {
                    first_fail_code = Some(kind.exit_code());
                    failure = Some((index, kind));
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

fn run_step(
    step: &Step,
    ctx: &ActionCtx,
    child_env: &mut HashMap<String, String>,
    bindings: &mut HashMap<String, String>,
) -> Result<(), FailureKind> {
    let expand_ctx =
        ExpandCtx { path: &ctx.path, query: &ctx.query, home: &ctx.home, env: bindings };
    match step {
        Step::Spawn { argv, wait, cwd, pane, .. } => {
            let mut expanded = Vec::with_capacity(argv.len());
            for template in argv {
                expanded.push(template.expand(&expand_ctx, false)?);
            }
            let cwd = match cwd {
                Some(template) => {
                    let raw = template
                        .expand(&expand_ctx, false)
                        .map_err(|e| FailureKind::Cwd(Box::new(e.into())))?;
                    let p = PathBuf::from(raw);
                    if p.is_absolute() {
                        p
                    } else {
                        Path::new(&ctx.home).join(p)
                    }
                }
                None => PathBuf::from(&ctx.home),
            };
            if let (Some(op), Some(runner)) = (pane, ctx.pane_runner) {
                // Scout stays in its pane; the command runs in a new one.
                // Failure to open the pane is a spawn failure.
                return runner(*op, &cwd, &expanded).map_err(|message| {
                    tracing::warn!(%message, "pane spawn failed");
                    FailureKind::Spawn(std::io::ErrorKind::Other)
                });
            }
            // Outside tmux a pane step runs here and waits, whatever `wait`
            // says, so its output is seen.
            let wait = *wait || pane.is_some();
            spawn(&expanded, wait, &cwd, child_env)
        }
        Step::BuiltinEdit => {
            let editor = builtin::resolve_editor(child_env).ok_or(FailureKind::NoEditor)?;
            let argv = vec![editor, ctx.path.display().to_string()];
            spawn(&argv, true, Path::new(&ctx.home), child_env)
        }
        Step::Print { format } => {
            let line = format.expand(&expand_ctx, true)?;
            match &ctx.print_to {
                Some(file) => {
                    let mut out = crate::platform::fs::open_append_nofollow(file)
                        .map_err(|_| FailureKind::PrintWrite)?;
                    out.write_all(line.as_bytes())
                        .and_then(|_| out.write_all(b"\n"))
                        .map_err(|_| FailureKind::PrintWrite)
                }
                None => {
                    let mut out = std::io::stdout().lock();
                    out.write_all(line.as_bytes())
                        .and_then(|_| out.write_all(b"\n"))
                        .and_then(|_| out.flush())
                        .map_err(|_| FailureKind::PrintWrite)
                }
            }
        }
        Step::Env { set } => {
            // All-or-nothing: evaluate every value before any binding lands.
            let mut staged = Vec::with_capacity(set.len());
            for (name, template) in set {
                staged.push((name.clone(), template.expand(&expand_ctx, false)?));
            }
            for (name, value) in staged {
                // Visible to later templates and exported to later children.
                child_env.insert(name.clone(), value.clone());
                bindings.insert(name, value);
            }
            Ok(())
        }
    }
}

fn spawn(
    argv: &[String],
    wait: bool,
    cwd: &Path,
    env: &HashMap<String, String>,
) -> Result<(), FailureKind> {
    if wait {
        match process::spawn_wait(argv, cwd, env) {
            Ok(status) if status.success() => Ok(()),
            Ok(status) => Err(FailureKind::ExitStatus(status.code().unwrap_or(1))),
            Err(err) => Err(FailureKind::Spawn(err.kind())),
        }
    } else {
        process::spawn_detached(argv, cwd, env).map_err(|err| FailureKind::Spawn(err.kind()))
    }
}

/// The environment a spawned child inherits: the process environment
/// with `.` and empty entries stripped from `PATH`. Secrets are
/// deliberately not stripped: editors and build tools need them.
pub fn sanitized_process_env() -> HashMap<String, String> {
    let mut env: HashMap<String, String> = std::env::vars().collect();
    if let Some(path) = env.get("PATH") {
        let sanitized: Vec<&str> =
            path.split(':').filter(|entry| !entry.is_empty() && *entry != ".").collect();
        env.insert("PATH".into(), sanitized.join(":"));
    }
    env
}

/// Credit the row. The ten-second window lives in the credit statement
/// itself, so this needs no memory of its own.
fn credit_visit(conn: &Connection, candidate_id: i64) -> bool {
    match crate::index::frecency::record_visit(conn, candidate_id, unix_now()) {
        Ok(credited) => credited,
        Err(err) => {
            tracing::warn!(%err, "visit credit failed");
            false
        }
    }
}
