//! Bare `scout`: the picker. One action and exit by default; with
//! `--session` (or `[scout] session = true`) the picker returns after each
//! action, in-process children get the terminal for their lifetime, the
//! last indexed tree can be walked again without leaving, and an action
//! that prints a command for the shell still ends the session because it
//! only works once scout is gone.

use std::cell::RefCell;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Instant;

use rusqlite::Connection;

use super::{logging, open_default_db, warn};
use crate::actions::{self, Action, ActionCtx, ExecOutcome};
use crate::platform::time::unix_now;
use crate::recon;
use crate::tmux::Tmux;
use crate::ui::{self, Outcome, Picker, ReindexJob};
use crate::{config, index, locations, platform, search, Error};

pub fn picker(session_flag: bool, print_to: Option<PathBuf>) -> crate::Result<u8> {
    logging::init(logging::Sink::StateFile);
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err(Error::PickerNeedsTty);
    }
    let home = platform::xdg::home()?;

    // Config load, including the trust prompt, happens before the
    // alternate screen: a broken config is a fix-and-rerun moment, not a
    // degraded-UI moment.
    let chain = locations::discovery_chain()?;
    let trust_store = locations::trust_store()?;
    let config = config::load(&chain, trust_store, true)?;
    for warning in &config.warnings {
        warn(warning);
    }
    let session = session_flag || config.session;

    let conn = open_default_db()?;
    let index_state = search::index_state(&conn)?;
    let candidates = search::load_candidates(&conn)?;

    // tmux is detected once. Outside a session there is nothing to keep a
    // pane for, so the check is skipped.
    let tmux: RefCell<Option<Tmux>> = RefCell::new(if session { Tmux::detect() } else { None });
    let in_tmux = tmux.borrow().is_some();
    let pane_runner = |op: actions::PaneOp, cwd: &Path, argv: &[String]| -> Result<(), String> {
        let mut guard = tmux.borrow_mut();
        let Some(t) = guard.as_mut() else { return Err("not inside tmux".into()) };
        let operation = match op {
            actions::PaneOp::SplitRight => crate::config::keys::Operation::SplitRight,
            actions::PaneOp::SplitDown => crate::config::keys::Operation::SplitDown,
            actions::PaneOp::NewWindow => crate::config::keys::Operation::NewWindow,
        };
        t.run(operation, Some(cwd), Some(argv))
    };

    let lookup = |id: i64| recon::store::unaccepted_for_row(&conn, id).unwrap_or_default();
    let mut picker = Picker::new(&config, candidates, index_state, &lookup, session, in_tmux);
    let home_text = home.display().to_string();

    loop {
        let outcome = picker.pick().map_err(Error::Ui)?;
        let Some(outcome) = outcome else {
            picker.finish();
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
                    Ok(stats) if stats.completed => {
                        let index_state = search::index_state(&conn)?;
                        let candidates = search::load_candidates(&conn)?;
                        picker.replace_candidates(candidates, index_state);
                        picker.set_notice(format!(
                            "re-indexed: {} paths, generation {}",
                            stats.inserted, stats.generation
                        ));
                    }
                    Ok(_) => picker
                        .set_notice("re-index did not complete; the previous index still serves"),
                    Err(message) => picker.set_notice(format!("re-index failed: {message}")),
                }
                continue;
            }
        };

        let Some(action) = config.actions.iter().find(|a| a.name == request.action_name) else {
            picker.finish();
            return Err(Error::ActionVanished(request.action_name));
        };
        let uses_pane =
            action.steps.iter().any(|s| matches!(s, actions::Step::Spawn { pane: Some(_), .. }));
        let ctx = ActionCtx {
            path: request.path.clone(),
            query: request.query.clone(),
            home: home_text.clone(),
            print_to: print_to.clone(),
            pane_runner: if in_tmux && session { Some(&pane_runner) } else { None },
        };

        if !session || action.ends_session() {
            // The v0.2.1 shape: leave the terminal, run, exit.
            picker.finish();
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

/// Start walking the last indexed root on its own thread with its own
/// connection. `None` when nothing has been indexed yet.
fn start_reindex(conn: &Connection) -> crate::Result<Option<ReindexJob>> {
    let Some(root) = super::index::last_root(conn) else { return Ok(None) };
    let db_path = locations::index_db()?;
    let progress = Arc::new(AtomicU64::new(0));
    let (tx, rx) = std::sync::mpsc::channel();
    let request = super::index::WalkRequest {
        root: root.clone(),
        hidden: false,
        follow: false,
        recon: false,
        progress: Some(progress.clone()),
    };
    std::thread::spawn(move || {
        let result = index::open(&db_path)
            .and_then(|writer| super::index::run_walk(writer, &request))
            .map_err(|err| err.to_string());
        let _ = tx.send(result);
    });
    Ok(Some(ReindexJob { root: root.display().to_string(), progress, done: rx }))
}
