//! SCOUT binary — full CLI (clap, ADR-002 slot 2). Default invocation
//! is the TUI picker; `index`, `open-db`, and `query` compose in shells.
//! The TUI draws on stderr; stdout belongs to the print seam so
//! `eval "$(scout)"` works (ADR-003 §2).

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "scout", about = "Fast project finder and action launcher", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Walk a tree into the index (streaming, gitignore-aware).
    Index {
        path: PathBuf,
        /// Include hidden entries.
        #[arg(long)]
        hidden: bool,
        /// Follow symlinks while walking.
        #[arg(long)]
        follow: bool,
    },
    /// Open (and if needed recover) an index DB, print its vitals.
    OpenDb { path: PathBuf },
    /// Rank candidates for a query and print them, best first.
    Query {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Index { path, hidden, follow }) => cmd_index(path, hidden, follow),
        Some(Cmd::OpenDb { path }) => cmd_open_db(path),
        Some(Cmd::Query { query, limit }) => cmd_query(&query, limit),
        None => cmd_tui(),
    }
}

fn init_tracing(to_state_file: bool) {
    if to_state_file {
        // The TUI owns stderr; tracing goes to the state dir (Surgeon §5).
        if let Ok(state) = scout::config::paths::state_home() {
            let dir = state.join("scout");
            if std::fs::create_dir_all(&dir).is_ok() {
                if let Ok(file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(dir.join("scout.log"))
                {
                    let _ = tracing_subscriber::fmt()
                        .with_writer(std::sync::Mutex::new(file))
                        .with_ansi(false)
                        .try_init();
                    return;
                }
            }
        }
        // Fall back to silence rather than corrupting the alt-screen.
        return;
    }
    let _ = tracing_subscriber::fmt().with_writer(std::io::stderr).try_init();
}

fn open_default_db() -> Result<rusqlite::Connection, ExitCode> {
    let db_path = match scout::config::paths::default_db_path() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("scout: {err}");
            return Err(ExitCode::FAILURE);
        }
    };
    scout::index::pragma::open(&db_path).map_err(|err| {
        eprintln!("scout: open {}: {err}", db_path.display());
        ExitCode::FAILURE
    })
}

fn cmd_index(root: PathBuf, hidden: bool, follow: bool) -> ExitCode {
    init_tracing(false);
    if let Err(err) = scout::index::signals::install() {
        eprintln!("scout: could not install signal handlers: {err}");
        return ExitCode::FAILURE;
    }
    let mut conn = match open_default_db() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    let config = scout::index::walk::WalkConfig { root, follow_symlinks: follow, hidden };
    let stats = match scout::index::insert::batched_insert(
        &mut conn,
        scout::index::walk::walk(&config),
        scout::index::insert::DEFAULT_BATCH_SIZE,
    ) {
        Ok(stats) => stats,
        Err(err) => {
            eprintln!("scout: index run failed: {err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = scout::index::recovery::shutdown(conn) {
        eprintln!("scout: shutdown: {err}");
        return ExitCode::FAILURE;
    }
    println!(
        "indexed {} paths in {} batches (skipped {}, errors {}); generation {}{}",
        stats.inserted,
        stats.batches,
        stats.skipped,
        stats.errors,
        stats.generation,
        if stats.completed { "" } else { " INCOMPLETE — prior generation still serves" }
    );
    if stats.completed {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn cmd_open_db(db_path: PathBuf) -> ExitCode {
    init_tracing(false);
    let conn = match scout::index::pragma::open(&db_path) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("scout: open {}: {err}", db_path.display());
            return ExitCode::FAILURE;
        }
    };
    let version = scout::index::schema::schema_version(&conn).unwrap_or(0);
    let rows: i64 = conn.query_row("SELECT count(*) FROM paths", [], |r| r.get(0)).unwrap_or(0);
    let generation: i64 = conn
        .query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |r| r.get(0))
        .unwrap_or(0);
    println!("{}: schema v{version}, {rows} paths, generation {generation}", db_path.display());
    if let Err(err) = scout::index::recovery::shutdown(conn) {
        eprintln!("scout: shutdown: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn cmd_query(query: &str, limit: usize) -> ExitCode {
    init_tracing(false);
    let conn = match open_default_db() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    let state = match scout::search::index_state(&conn) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    match state {
        scout::search::IndexState::Empty => {
            eprintln!("no paths indexed — run 'scout index <path>' to populate");
            return ExitCode::SUCCESS;
        }
        scout::search::IndexState::FirstScanInProgress { rows_so_far } => {
            eprintln!(
                "indexing in progress ({rows_so_far} paths so far) — results will appear when \
                 the first scan completes"
            );
            return ExitCode::SUCCESS;
        }
        scout::search::IndexState::Ready { .. } => {}
    }
    let candidates = match scout::search::load_candidates(&conn) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut matcher = scout::search::matcher::NucleoMatcher::new();
    let now = scout::index::unix_now();
    for ranked in scout::search::search(&mut matcher, &candidates, query, now, limit) {
        println!("{}", ranked.path);
    }
    let _ = scout::index::recovery::shutdown(conn);
    ExitCode::SUCCESS
}

fn cmd_tui() -> ExitCode {
    init_tracing(true);
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eprintln!("scout: the picker needs a TTY; use 'scout query <q>' for non-interactive use");
        return ExitCode::from(2);
    }
    let home = match scout::config::paths::home() {
        Ok(h) => h,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };

    // Config load — including the trust prompt — happens BEFORE the
    // alt-screen (ADR-004 §10: a broken config is a fix-and-rerun
    // moment, not a degraded-UI moment).
    let chain = match scout::config::paths::discovery_chain() {
        Ok(chain) => chain,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    let trust_store = match scout::config::paths::trust_store_path() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    let config = match scout::config::loader::load(&chain, trust_store, true) {
        Ok(config) => config,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    for warning in &config.warnings {
        eprintln!("scout: warning: {warning}");
    }

    let conn = match open_default_db() {
        Ok(conn) => conn,
        Err(code) => return code,
    };
    let index_state = match scout::search::index_state(&conn) {
        Ok(state) => state,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    let candidates = match scout::search::load_candidates(&conn) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };

    let request = match scout::ui::run(&config, &candidates, &index_state) {
        Ok(request) => request,
        Err(err) => {
            eprintln!("scout: ui: {err}");
            return ExitCode::FAILURE;
        }
    };

    let Some(request) = request else {
        let _ = scout::index::recovery::shutdown(conn);
        return ExitCode::SUCCESS;
    };

    let Some(action) = config.actions.iter().find(|a| a.name == request.action_name) else {
        eprintln!("scout: action `{}` vanished from the merged set", request.action_name);
        return ExitCode::FAILURE;
    };
    let ctx = scout::actions::ActionCtx {
        path: request.path,
        query: request.query,
        home: home.display().to_string(),
    };
    let outcome = scout::actions::execute(action, &ctx, Some((&conn, request.candidate_id)));
    let _ = scout::index::recovery::shutdown(conn);
    ExitCode::from(outcome.exit_code.clamp(0, 255) as u8)
}
