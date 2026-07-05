//! Phase 2 scaffolding CLI: `scout index <path>` and `scout open-db <path>`.
//! Arg parsing is hand-rolled — `clap` (ADR-002 slot 2) enters in Phase 3.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    tracing_subscriber::fmt().with_writer(std::io::stderr).init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [cmd, path] if cmd == "index" => cmd_index(PathBuf::from(path)),
        [cmd, path] if cmd == "open-db" => cmd_open_db(PathBuf::from(path)),
        _ => {
            eprintln!("usage: scout index <path> | scout open-db <db-path>");
            ExitCode::from(2)
        }
    }
}

fn default_db_path() -> Result<PathBuf, String> {
    let data_home = match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => {
            let home = std::env::var("HOME").map_err(|_| "HOME is unset".to_string())?;
            PathBuf::from(home).join(".local/share")
        }
    };
    Ok(data_home.join("scout/index.db"))
}

fn cmd_index(root: PathBuf) -> ExitCode {
    if let Err(err) = scout::index::signals::install() {
        eprintln!("scout: could not install signal handlers: {err}");
        return ExitCode::FAILURE;
    }
    let db_path = match default_db_path() {
        Ok(p) => p,
        Err(err) => {
            eprintln!("scout: {err}");
            return ExitCode::FAILURE;
        }
    };
    let mut conn = match scout::index::pragma::open(&db_path) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("scout: open {}: {err}", db_path.display());
            return ExitCode::FAILURE;
        }
    };

    let config = scout::index::walk::WalkConfig::new(root);
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
    let conn = match scout::index::pragma::open(&db_path) {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("scout: open {}: {err}", db_path.display());
            return ExitCode::FAILURE;
        }
    };
    let version = scout::index::schema::schema_version(&conn).unwrap_or(0);
    let rows: i64 = conn
        .query_row("SELECT count(*) FROM paths", [], |r| r.get(0))
        .unwrap_or(0);
    let generation: i64 = conn
        .query_row("SELECT current_generation FROM run_state WHERE id = 1", [], |r| r.get(0))
        .unwrap_or(0);
    println!(
        "{}: schema v{version}, {rows} paths, generation {generation}",
        db_path.display()
    );
    if let Err(err) = scout::index::recovery::shutdown(conn) {
        eprintln!("scout: shutdown: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
