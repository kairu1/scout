//! The binary: parse arguments, dispatch to `scout::commands`, turn an
//! error into one stderr line and an exit code. Nothing else lives here.

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
    /// Print the state scout resolves at startup: config discovery,
    /// trust, index, environment. Read-only; never prompts.
    Doctor {
        /// `human` (default) or `tsv`.
        #[arg(long, default_value = "human")]
        format: String,
    },
    /// Rank candidates for a query and print them, best first.
    /// Exits 1 when nothing matched.
    Query {
        query: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// `paths` (default) or `tsv`: rank, visits, path. Path last,
        /// so an embedded tab cannot shift a field.
        #[arg(long, default_value = "paths")]
        format: String,
        /// Separate paths with NUL instead of newline, for `xargs -0`.
        #[arg(long)]
        print0: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Cmd::Index { path, hidden, follow }) => scout::commands::index(path, hidden, follow),
        Some(Cmd::OpenDb { path }) => scout::commands::open_db(path),
        Some(Cmd::Doctor { format }) => scout::commands::doctor(&format),
        Some(Cmd::Query { query, limit, format, print0 }) => {
            scout::commands::query(&query, limit, &format, print0)
        }
        None => scout::commands::picker(),
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("scout: {err}");
            if let Some(hint) = err.hint() {
                eprintln!("scout: {hint}");
            }
            ExitCode::from(err.exit_code())
        }
    }
}
