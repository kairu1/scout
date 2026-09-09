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
    /// Run this session in the current terminal without tmux, even when
    /// tmux is installed (`[scout] tmux = "never"` for one run).
    #[arg(long)]
    no_tmux: bool,
    /// The picker was started by scout's own tmux launch; on exit,
    /// detach the client instead of just ending. Not meant to be typed.
    #[arg(long, hide = true)]
    tmux_owned: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Walk a tree into the index (streaming, gitignore-aware). With no
    /// path, walk every indexed tree again the way it was walked before.
    /// A path inside an indexed tree re-walks that tree; a new path adds
    /// a tree; a path that would contain one is refused.
    Index {
        path: Option<PathBuf>,
        /// Include hidden entries (remembered for this tree).
        #[arg(long, overrides_with = "no_hidden", requires = "path")]
        hidden: bool,
        /// Stop including hidden entries for this tree.
        #[arg(long, overrides_with = "hidden", requires = "path")]
        no_hidden: bool,
        /// Follow symlinks while walking (remembered for this tree).
        #[arg(long, overrides_with = "no_follow", requires = "path")]
        follow: bool,
        /// Stop following symlinks for this tree.
        #[arg(long, overrides_with = "follow", requires = "path")]
        no_follow: bool,
        /// Skip the cheap recon checks (ownership, mode, special bits,
        /// exposed secrets) that otherwise run on every path as it is
        /// indexed. Remembered for this tree.
        #[arg(long, overrides_with = "recon", requires = "path")]
        no_recon: bool,
        /// Run the cheap recon checks again for a tree that opted out.
        #[arg(long, overrides_with = "no_recon", requires = "path")]
        recon: bool,
        /// Drop the tree at PATH from the index. Its rows keep their
        /// history for a while and come back if the tree is indexed again.
        #[arg(long, requires = "path")]
        forget: bool,
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
        /// Only findings first seen after the previous `scout recon`
        /// run. Exits 1 when one of them is at or above --fail-on, so a
        /// script can branch on "anything new".
        #[arg(long)]
        since_last: bool,
    },
    /// Run a command in a directory, then become a shell there. What tmux
    /// starts in a pane scout opened; not meant to be typed.
    #[command(hide = true)]
    PaneRun {
        #[arg(long)]
        cwd: PathBuf,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        argv: Vec<String>,
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
        Some(Cmd::Index {
            path,
            hidden,
            no_hidden,
            follow,
            no_follow,
            no_recon,
            recon,
            forget,
        }) => {
            let pick = |on: bool, off: bool| {
                if on {
                    Some(true)
                } else if off {
                    Some(false)
                } else {
                    None
                }
            };
            let flags = scout::index::roots::WalkFlags {
                hidden: pick(hidden, no_hidden),
                follow: pick(follow, no_follow),
                recon: pick(recon, no_recon),
            };
            scout::commands::index(path, flags, forget)
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
        Some(Cmd::Recon { cmd: None, path, format, fail_on, all, since_last }) => {
            scout::commands::recon(path, &format, &fail_on, all, since_last)
        }
        Some(Cmd::PaneRun { cwd, argv }) => scout::commands::pane_run(&cwd, &argv),
        Some(Cmd::OpenDb { path }) => scout::commands::open_db(path),
        Some(Cmd::Doctor { format }) => scout::commands::doctor(&format),
        Some(Cmd::Query { query, limit, format, print0 }) => {
            scout::commands::query(&query, limit, &format, print0)
        }
        None => scout::commands::picker(scout::commands::PickerArgs {
            session: cli.session,
            print_to: cli.print_to,
            no_tmux: cli.no_tmux,
            tmux_owned: cli.tmux_owned,
        }),
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
