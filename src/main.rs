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
    /// Stay in the picker after an action instead of exiting. Actions that
    /// print a command for your shell still end the session, and are
    /// marked in the picker. `[scout] session = true` sets this by default.
    #[arg(short = 's', long)]
    session: bool,
    /// Write printed commands to this file instead of stdout. The shell
    /// wrapper passes a temporary file so that children keep stdout.
    #[arg(long, value_name = "FILE")]
    print_to: Option<PathBuf>,
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
        /// Run the cheap recon checks (ownership, mode, special bits,
        /// exposed secrets, symlink escapes) on every path as it is indexed.
        #[arg(long)]
        recon: bool,
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
    /// Report who else can edit the indexed paths and what is exposed:
    /// ownership, modes, special bits, credential files, symlink escapes,
    /// ACLs, changed entry points. Exits 1 when a finding at or above
    /// --fail-on is present and not accepted. Reports; never repairs.
    #[command(args_conflicts_with_subcommands = true, subcommand_precedence_over_arg = true)]
    Recon {
        #[command(subcommand)]
        cmd: Option<ReconCmd>,
        /// Report on one tree only (default: the whole index).
        path: Option<PathBuf>,
        /// `human` (default) or `tsv`: severity, check, first seen, last
        /// seen, accepted, path, detail.
        #[arg(long, default_value = "human")]
        format: String,
        /// Severity that makes the exit code 1: `low`, `high` (default) or
        /// `critical`.
        #[arg(long, default_value = "high")]
        fail_on: String,
        /// Show accepted findings too.
        #[arg(long)]
        all: bool,
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

#[derive(Subcommand)]
enum ReconCmd {
    /// Mark a finding as expected. It stays hidden until the underlying
    /// fact (mode, owner, link target) changes.
    Accept {
        path: PathBuf,
        /// The check name as printed by `scout recon`.
        check: String,
        #[arg(long)]
        reason: String,
    },
    /// Remove an accepted exception.
    Revoke { path: PathBuf, check: String },
    /// Record the project's executable entry points (Makefile, package.json,
    /// Cargo.toml, shell scripts, git hooks) so `scout recon` can report
    /// when one changes.
    Baseline { path: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Cmd::Index { path, hidden, follow, recon }) => {
            scout::commands::index(path, hidden, follow, recon)
        }
        Some(Cmd::Recon { cmd: Some(ReconCmd::Accept { path, check, reason }), .. }) => {
            scout::commands::recon::accept(&path, &check, &reason)
        }
        Some(Cmd::Recon { cmd: Some(ReconCmd::Revoke { path, check }), .. }) => {
            scout::commands::recon::revoke(&path, &check)
        }
        Some(Cmd::Recon { cmd: Some(ReconCmd::Baseline { path }), .. }) => {
            scout::commands::recon::baseline(&path)
        }
        Some(Cmd::Recon { cmd: None, path, format, fail_on, all }) => {
            scout::commands::recon(path, &format, &fail_on, all)
        }
        Some(Cmd::OpenDb { path }) => scout::commands::open_db(path),
        Some(Cmd::Doctor { format }) => scout::commands::doctor(&format),
        Some(Cmd::Query { query, limit, format, print0 }) => {
            scout::commands::query(&query, limit, &format, print0)
        }
        None => scout::commands::picker(cli.session, cli.print_to),
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
