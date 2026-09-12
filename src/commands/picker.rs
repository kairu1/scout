//! Bare `scout`: the picker. One action and exit by default; with
//! `--session` (or `[scout] session = true`) the picker returns after each
//! action, in-process children get the terminal for their lifetime, every
//! indexed tree can be walked again without leaving, and an action
//! that prints a command for the shell still ends the session because it
//! only works once scout is gone.
//!
//! A session outside tmux, with tmux installed, does not run here: after
//! the trust prompt this process becomes the tmux client of a session
//! whose first pane runs the picker again with `--tmux-owned`. That inner
//! picker detaches every client when it leaves, which is what returns
//! the launcher's shell wrapper to its prompt.
//!
//! Owns: the session loop, the launch decision, the owned-pane exit
//! (error shown and held, keys unbound, handoff withdrawn, clients
//! detached), and the re-index thread. Refuses to know about: drawing,
//! the schema, how an action's steps run. Exposes: `picker`, `PickerArgs`.

use std::cell::RefCell;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use rusqlite::Connection;

use super::{logging, open_default_db, warn};
use crate::actions::{self, Action, ActionCtx, ExecOutcome};
use crate::platform::time::unix_now;
use crate::recon;
use crate::tmux::{self, Decision, Tmux};
use crate::ui::{self, Outcome, Picker, ReindexJob};
use crate::{config, index, locations, platform, search, Error};

/// The top-level flags that reach the picker.
#[derive(Debug, Clone, Default)]
pub struct PickerArgs {
    pub session: bool,
    pub print_to: Option<PathBuf>,
    pub no_tmux: bool,
    pub tmux_owned: bool,
}

pub fn picker(args: PickerArgs) -> crate::Result<u8> {
    logging::init(logging::Sink::StateFile);
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(Error::PickerNeedsTty);
    }
    let owned_pane = args.tmux_owned;
    let result = picker_inner(args);
    if owned_pane {
        // Whatever happened, the pane is about to vanish with this
        // process: an error the user has not seen is shown and held, the
        // tmux-wide keys and the handed-over file are withdrawn, and
        // every client is detached so the launcher's shell gets its
        // prompt back.
        if let Err(err) = &result {
            eprintln!("scout: {err}");
            if let Some(hint) = err.hint() {
                eprintln!("scout: {hint}");
            }
            eprintln!("scout: press any key to return to your shell");
            let _ = ui::terminal::wait_for_key();
        }
        if let Some(t) = Tmux::detect() {
            t.unbind_root_keys(&config::keys::Keys::default());
            t.forget_print_to();
            if let Err(message) = t.detach_clients() {
                warn(format!("could not detach: {message}"));
            }
        }
    }
    result
}

fn picker_inner(args: PickerArgs) -> crate::Result<u8> {
    // Config load, including the trust prompt, happens before the
    // alternate screen: a broken config is a fix-and-rerun moment, not a
    // degraded-UI moment. In a launch it happens in the user's own
    // terminal, before tmux, so the prompt is where they can see it.
    let chain = locations::discovery_chain()?;
    let trust_store = locations::trust_store()?;
    let config = config::load(&chain, trust_store, true)?;
    for warning in &config.warnings {
        warn(warning);
    }
    let session = args.session || config.session;

    let decision = if session && !args.tmux_owned {
        tmux::decide(config.tmux, args.no_tmux, tmux::installed(), tmux::inside())
    } else if session {
        Decision::UseSurrounding
    } else {
        Decision::InProcess
    };
    let mut no_tmux_reason: Option<&'static str> = None;
    match decision {
        Decision::Launch => {
            let launch = tmux::Launch {
                server: config.tmux_server,
                session: config.tmux_session.clone(),
                scout_exe: std::env::current_exe().unwrap_or_else(|_| "scout".into()),
                cwd: std::env::current_dir().ok(),
                print_to: args.print_to.clone(),
            };
            match launch.prepare() {
                Ok(client) => {
                    // Only returns on failure; the process is the client
                    // from here until it detaches.
                    let err = platform::process::exec_argv(&client);
                    if config.tmux == tmux::Policy::Require {
                        return Err(Error::io("exec tmux", err));
                    }
                    warn(format!("could not start tmux ({err}); running the session here"));
                    no_tmux_reason = Some("tmux failed to start; run scout doctor");
                }
                Err(message) => {
                    if config.tmux == tmux::Policy::Require {
                        return Err(Error::io("start tmux", std::io::Error::other(message)));
                    }
                    warn(format!("{message}; running the session here"));
                    no_tmux_reason = Some("tmux failed to start; run scout doctor");
                }
            }
        }
        Decision::Refuse => return Err(Error::TmuxRequired),
        Decision::InProcess if session => {
            no_tmux_reason = Some(if args.no_tmux || config.tmux == tmux::Policy::Never {
                "tmux is off for this session (--no-tmux or [scout] tmux); no panes"
            } else {
                "install tmux and scout will open panes and windows for you"
            });
        }
        Decision::InProcess | Decision::UseSurrounding => {}
    }

    let owned = args.tmux_owned && session;
    let result = run(&config, session, args.print_to, decision, no_tmux_reason, owned);
    if owned {
        // The user's own bindings are withdrawn here, where the keys are
        // known; the defaults are withdrawn again in `picker` for the
        // paths that never reach this line.
        if let Some(t) = Tmux::detect() {
            t.unbind_root_keys(&config.keys);
        }
    }
    result
}

fn run(
    config: &config::Config,
    session: bool,
    print_to: Option<PathBuf>,
    decision: Decision,
    no_tmux_reason: Option<&'static str>,
    owned: bool,
) -> crate::Result<u8> {
    // Mode is settled here, once: the picker only ever sees the actions
    // this run offers, so a name or chord shared across modes is unique
    // again by the time anything dispatches on it.
    let config = &config.for_mode(session);
    let home = platform::xdg::home()?;
    let conn = open_default_db()?;
    let index_state = search::index_state(&conn)?;
    let candidates = search::load_candidates(&conn)?;

    // tmux is detected once. Outside a session there is nothing to keep a
    // pane for, so the check is skipped; with tmux switched off it is
    // ignored even inside one.
    let tmux: RefCell<Option<Tmux>> =
        RefCell::new(if session && decision == Decision::UseSurrounding {
            Tmux::detect()
        } else {
            None
        });
    let in_tmux = tmux.borrow().is_some();
    // On scout's own server the focus and zoom keys, and focus-picker,
    // are tmux bindings as well, so the same keys work from every pane.
    let own_server = owned && config.tmux_server == tmux::Server::Private;
    if owned {
        if let Some(t) = tmux.borrow().as_ref() {
            t.claim_picker(print_to.as_deref());
            if own_server {
                t.bind_root_keys(&config.keys);
            }
        }
    }
    let leave = |tmux: &RefCell<Option<Tmux>>| {
        if own_server {
            if let Some(t) = tmux.borrow().as_ref() {
                t.unbind_root_keys(&config.keys);
            }
        }
    };
    let pane_runner = |op: actions::PaneOp,
                       cwd: &Path,
                       argv: &[String],
                       env: &[(String, String)]|
     -> Result<(), String> {
        let mut guard = tmux.borrow_mut();
        let Some(t) = guard.as_mut() else { return Err("not inside tmux".into()) };
        let operation = match op {
            actions::PaneOp::SplitRight => crate::config::keys::Operation::SplitRight,
            actions::PaneOp::SplitDown => crate::config::keys::Operation::SplitDown,
            actions::PaneOp::NewWindow => crate::config::keys::Operation::NewWindow,
        };
        // No command: the pane is a shell at `cwd`, exactly what the
        // split keys open at the selection.
        let command = if argv.is_empty() { None } else { Some(argv) };
        t.run_with_env(operation, Some(cwd), command, env)
    };

    let lookup = |id: i64| recon::store::unaccepted_for_row(&conn, id).unwrap_or_default();
    let mut picker = Picker::new(config, candidates, index_state, &lookup, session, in_tmux);
    if let Some(reason) = no_tmux_reason {
        picker.set_no_tmux_reason(reason);
    }
    let home_text = home.display().to_string();

    loop {
        let outcome = picker.pick().map_err(Error::Ui)?;
        let Some(outcome) = outcome else {
            picker.finish();
            leave(&tmux);
            let _ = index::recovery::shutdown(conn);
            return Ok(0);
        };
        let request = match outcome {
            Outcome::Run(request) => request,
            Outcome::Accept { path, checks, then, .. } => {
                let text = path.display().to_string();
                for check in checks {
                    recon::store::accept(&conn, &text, check, "accepted from picker", unix_now())?;
                    tracing::info!(check = check.name(), path = %text, "recon.accept");
                }
                then
            }
            Outcome::Pane(op, path) => {
                let result = match tmux.borrow_mut().as_mut() {
                    Some(t) => t.run(op, path.as_deref(), None),
                    None => Err("not inside tmux".into()),
                };
                if let Err(message) = result {
                    picker.set_notice(format!("{}: {message}", op.name()));
                }
                continue;
            }
            Outcome::Reindex => {
                match start_reindex(&conn)? {
                    Some(job) => picker.start_reindex(job),
                    None => picker
                        .set_notice("nothing to re-index yet: run `scout index <path>` once first"),
                }
                continue;
            }
            Outcome::ReindexDone(result) => {
                match result {
                    Ok(report) => {
                        // Any completed walk moved rows; reload so the
                        // markers are current, then say how far it got.
                        if report.completed() > 0 {
                            let index_state = search::index_state(&conn)?;
                            let candidates = search::load_candidates(&conn)?;
                            picker.replace_candidates(candidates, index_state);
                        }
                        if report.all_completed() {
                            picker.set_notice(format!(
                                "re-indexed {} root(s): {} paths, generation {}",
                                report.roots_total,
                                report.inserted(),
                                report.generation().unwrap_or(0)
                            ));
                        } else {
                            picker.set_notice(format!(
                                "re-index stopped after {} of {} root(s); the previous index \
                                 still serves for the rest",
                                report.completed(),
                                report.roots_total
                            ));
                        }
                    }
                    Err(message) => picker.set_notice(format!("re-index failed: {message}")),
                }
                continue;
            }
        };

        let Some(action) = config.actions.iter().find(|a| a.name == request.action_name) else {
            picker.finish();
            leave(&tmux);
            return Err(Error::ActionVanished(request.action_name));
        };
        let uses_pane =
            action.steps.iter().any(|s| matches!(s, actions::Step::Spawn { pane: Some(_), .. }));
        // In an owned session the exit command goes to the file of the
        // shell that most recently attached, when that file passes the
        // sink checks; the launch-time file is the fallback.
        let sink = if owned && action.ends_session() {
            tmux.borrow().as_ref().and_then(|t| t.handoff_print_to()).or_else(|| print_to.clone())
        } else {
            print_to.clone()
        };
        let ctx = ActionCtx {
            path: request.path.clone(),
            query: request.query.clone(),
            home: home_text.clone(),
            print_to: sink,
            pane_runner: if in_tmux && session { Some(&pane_runner) } else { None },
        };

        if !session || action.ends_session() {
            // The v0.2.1 shape: leave the terminal, run, exit.
            picker.finish();
            leave(&tmux);
            let outcome = actions::execute(action, &ctx, Some((&conn, request.candidate_id)));
            after_fix_action(&conn, action, &ctx, request.candidate_id, &home);
            let _ = index::recovery::shutdown(conn);
            return report(action, outcome);
        }

        // A pane action inside tmux never touches this terminal: run it
        // and stay on screen.
        if uses_pane && in_tmux {
            let outcome = actions::execute(action, &ctx, Some((&conn, request.candidate_id)));
            if let Some((step, kind)) = &outcome.failure {
                picker.set_notice(format!("{} failed at step {} ({kind})", action.name, step + 1));
            }
            if let Ok((s, last, visits)) = row_frecency(&conn, request.candidate_id) {
                picker.update_row(request.candidate_id, s, last, visits);
            }
            continue;
        }

        // In-process, then back to the picker. The child owns the tty for
        // its lifetime; scout draws nothing until it has exited.
        picker.suspend();
        let started = Instant::now();
        let outcome = actions::execute(action, &ctx, Some((&conn, request.candidate_id)));
        if uses_pane {
            eprintln!(
                "scout: {}: not inside tmux, so it ran here instead of in a pane",
                action.name
            );
        }
        after_fix_action(&conn, action, &ctx, request.candidate_id, &home);
        if let Some((step, kind)) = &outcome.failure {
            eprintln!("scout: action `{}` failed at step {} ({kind})", action.name, step + 1);
            if let Some(hint) = kind.hint() {
                eprintln!("scout: {hint}");
            }
        }
        if action.pauses() {
            // Scout cannot show "what happened" without a pty; the child's
            // output is on the screen already. This line is scout's own.
            eprintln!(
                "scout: {}: exit {} in {:.1} s - press any key to return (ctrl-c leaves)",
                action.name,
                outcome.exit_code,
                started.elapsed().as_secs_f64()
            );
            if ui::terminal::wait_for_key().map_err(Error::Ui)? {
                leave(&tmux);
                let _ = index::recovery::shutdown(conn);
                return Ok(0);
            }
        }
        // Re-read the row: a credit moved its frecency, and a fix action
        // may have changed its findings. Cheaper than reloading 100k rows.
        if let Ok((s, last, visits)) = row_frecency(&conn, request.candidate_id) {
            picker.update_row(request.candidate_id, s, last, visits);
        }
    }
}

/// Exit code and stderr for the one-shot shape.
fn report(action: &Action, outcome: ExecOutcome) -> crate::Result<u8> {
    // Say what failed, why, and what to do. A reason that only reaches
    // the log file is a reason the user never sees.
    match outcome.failure {
        Some((step, kind)) if outcome.exit_code != 0 => Err(Error::ActionFailed {
            action: action.name.clone(),
            step: step + 1,
            kind,
            exit_code: outcome.exit_code,
        }),
        Some((step, kind)) => {
            // A later step failed after an earlier one succeeded: the chain
            // succeeded by contract, but the user should still hear about it.
            eprintln!("scout: action `{}` failed at step {} ({kind})", action.name, step + 1);
            if let Some(hint) = kind.hint() {
                eprintln!("scout: {hint}");
            }
            Ok(0)
        }
        None => Ok(outcome.exit_code.clamp(0, 255) as u8),
    }
}

/// A fix action (one gated on a finding) that ran has changed the facts;
/// re-run the stat checks for this one row so the marker clears without
/// a full recon.
fn after_fix_action(conn: &Connection, action: &Action, ctx: &ActionCtx<'_>, id: i64, home: &Path) {
    if action.when.as_ref().is_none_or(|w| w.finding.is_none()) {
        return;
    }
    let recon_ctx = recon::checks::Context { euid: platform::fs::euid(), now: unix_now(), home };
    if let Err(err) = recon::run::rescan_path(conn, id, &ctx.path, &recon_ctx) {
        warn(format!("could not re-check {}: {err}", ctx.path.display()));
    }
}

fn row_frecency(conn: &Connection, id: i64) -> rusqlite::Result<(f64, i64, i64)> {
    conn.query_row(
        "SELECT S, last_update, visits_total FROM paths WHERE rowid = :id",
        rusqlite::named_params! { ":id": id },
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
}

/// Start walking every root again, each the way it was walked before,
/// on its own thread with its own connection. `None` when nothing has
/// been indexed yet.
fn start_reindex(conn: &Connection) -> crate::Result<Option<ReindexJob>> {
    if index::roots::list(conn)?.is_empty() {
        return Ok(None);
    }
    let db_path = locations::index_db()?;
    let progress = Arc::new(AtomicU64::new(0));
    let label = Arc::new(RwLock::new(String::new()));
    let (tx, rx) = std::sync::mpsc::channel();
    let (thread_progress, thread_label) = (progress.clone(), label.clone());
    std::thread::spawn(move || {
        let result = index::open_writer(&db_path)
            .and_then(|mut writer| {
                let report =
                    super::index::walk_all(&mut writer, Some(thread_progress), Some(thread_label));
                let _ = index::recovery::close_writer(writer);
                report
            })
            .map_err(|err| err.to_string());
        let _ = tx.send(result);
    });
    Ok(Some(ReindexJob { label, progress, done: rx }))
}
