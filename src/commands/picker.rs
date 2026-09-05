//! Bare `scout`: the picker, then one action, then exit.

use std::io::IsTerminal;

use super::{logging, open_default_db, warn};
use crate::actions::{self, ActionCtx};
use crate::{config, index, locations, platform, search, ui, Error};

pub fn picker() -> crate::Result<u8> {
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

    let conn = open_default_db()?;
    let index_state = search::index_state(&conn)?;
    let candidates = search::load_candidates(&conn)?;

    let request = ui::run(&config, &candidates, &index_state).map_err(Error::Ui)?;

    let Some(request) = request else {
        let _ = index::recovery::shutdown(conn);
        return Ok(0);
    };

    let Some(action) = config.actions.iter().find(|a| a.name == request.action_name) else {
        return Err(Error::ActionVanished(request.action_name));
    };
    let ctx =
        ActionCtx { path: request.path, query: request.query, home: home.display().to_string() };
    let outcome = actions::execute(action, &ctx, Some((&conn, request.candidate_id)));
    let _ = index::recovery::shutdown(conn);
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
