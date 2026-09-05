//! `scout recon`: report on the indexed ground; accept and revoke
//! exceptions; record a baseline. Reports never repair.

use std::path::{Path, PathBuf};

use super::{logging, open_default_db};
use crate::platform::time::unix_now;
use crate::recon::checks::Context;
use crate::recon::{run, store, Check, Report, Severity};
use crate::{config, index, locations, platform, Error};

/// The report. `path` narrows to one tree; `fail_on` sets the severity
/// that makes the exit code 1; `all` shows accepted findings too.
pub fn recon(path: Option<PathBuf>, format: &str, fail_on: &str, all: bool) -> crate::Result<u8> {
    if !matches!(format, "human" | "tsv") {
        return Err(Error::UnknownFormat { given: format.to_string(), wanted: "human|tsv" });
    }
    let Some(threshold) = Severity::parse(fail_on) else {
        return Err(Error::UnknownFormat {
            given: fail_on.to_string(),
            wanted: "low|high|critical",
        });
    };
    logging::init(logging::Sink::Stderr);
    let home = platform::xdg::home()?;
    let conn = open_default_db()?;
    let under = match &path {
        Some(p) => Some(
            std::fs::canonicalize(p).map_err(|e| Error::io(format!("recon {}", p.display()), e))?,
        ),
        None => None,
    };
    let ctx = Context { euid: platform::fs::euid(), now: unix_now(), home: &home };
    let stats = run::scan(&conn, under.as_deref(), &ctx)?;
    tracing::info!(paths = stats.paths, findings = stats.findings, "recon.scan");

    let findings = store::load(&conn, under.as_deref().map(|p| p.to_str().unwrap_or_default()))?;
    let own_state = match under {
        // A narrowed run is about that tree, not about scout itself.
        Some(_) => Vec::new(),
        None => {
            let chain = locations::discovery_chain()?;
            let config_path = config::discover(&chain)?;
            run::own_state(
                config_path.as_deref(),
                &locations::trust_store()?,
                &locations::index_db()?,
                &wrapper_candidates(),
                ctx.euid,
            )
        }
    };
    let report = Report { findings, own_state, show_accepted: all };
    match format {
        "tsv" => print!("{}", report.render_tsv()),
        _ => print!("{}", report.render(home.to_str())),
    }
    let code = report.exit_code(threshold);
    let _ = index::recovery::shutdown(conn);
    Ok(code)
}

/// Where the shell wrapper may live: beside the running binary, or under
/// the config directory. Only files that exist are checked.
fn wrapper_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("scout.bash"));
            out.push(dir.join("../shell/scout.bash"));
        }
    }
    if let Ok(cfg) = platform::xdg::config_home() {
        out.push(cfg.join("scout/scout.bash"));
    }
    out.into_iter().filter(|p| p.is_file()).collect()
}

fn canonical_text(path: &Path) -> crate::Result<String> {
    let canonical = std::fs::canonicalize(path)
        .map_err(|e| Error::io(format!("recon {}", path.display()), e))?;
    canonical
        .to_str()
        .map(str::to_string)
        .ok_or_else(|| Error::ConfigRefused(format!("{}: path is not valid UTF-8", path.display())))
}

fn check_named(name: &str) -> crate::Result<Check> {
    Check::parse(name).ok_or_else(|| Error::UnknownFormat {
        given: name.to_string(),
        wanted: "a check name such as world-writable-dir (see `scout recon`)",
    })
}

/// `scout recon accept PATH CHECK --reason TEXT`.
pub fn accept(path: &Path, check: &str, reason: &str) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    let check = check_named(check)?;
    let conn = open_default_db()?;
    let text = canonical_text(path)?;
    let outcome = store::accept(&conn, &text, check, reason, unix_now())?;
    let _ = index::recovery::shutdown(conn);
    match outcome {
        store::Accepted::Recorded => {
            println!("accepted {} on {text}", check.name());
            Ok(0)
        }
        store::Accepted::NoSuchFinding => {
            eprintln!(
                "scout: no {} finding on {text}; run `scout recon` first (accepting a finding that \
                 does not exist is not a decision)",
                check.name()
            );
            Ok(1)
        }
    }
}

/// `scout recon revoke PATH CHECK`.
pub fn revoke(path: &Path, check: &str) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    let check = check_named(check)?;
    let conn = open_default_db()?;
    let text = canonical_text(path)?;
    let removed = store::revoke(&conn, &text, check)?;
    let _ = index::recovery::shutdown(conn);
    if removed {
        println!("revoked {} on {text}", check.name());
        Ok(0)
    } else {
        eprintln!("scout: no exception for {} on {text}", check.name());
        Ok(1)
    }
}

/// `scout recon baseline PATH`: record the project's entry points.
pub fn baseline(path: &Path) -> crate::Result<u8> {
    logging::init(logging::Sink::Stderr);
    let conn = open_default_db()?;
    let text = canonical_text(path)?;
    let row: Option<i64> = conn
        .query_row(
            "SELECT rowid FROM paths WHERE path = :p AND tombstoned_at IS NULL",
            rusqlite::named_params! { ":p": text },
            |r| r.get(0),
        )
        .ok();
    let Some(path_id) = row else {
        eprintln!("scout: {text} is not in the index; run `scout index` over it first");
        let _ = index::recovery::shutdown(conn);
        return Ok(1);
    };
    let count = store::record_baseline(&conn, path_id, Path::new(&text), unix_now())?;
    let _ = index::recovery::shutdown(conn);
    println!("recorded {count} entry point(s) for {text}");
    Ok(0)
}
