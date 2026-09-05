//! Runtime verbosity from `SCOUT_LOG`, and where log lines go: stderr for
//! the CLI, the state directory for the picker, which owns stderr.

/// Where log lines go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    Stderr,
    /// `$XDG_STATE_HOME/scout/scout.log`; silent if it cannot be opened,
    /// rather than corrupting the alternate screen.
    StateFile,
}

/// Level names `SCOUT_LOG` accepts as a bare directive.
const LOG_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// A bare directive (no `=`) is a level if it names one, and a *target*
/// otherwise, so `SCOUT_LOG=dbug` parses cleanly and then matches
/// nothing, silencing the very output the user just asked for. The
/// filter is still honoured (targeting a module is legitimate), but a
/// bare non-level is called out.
fn warn_on_bare_non_level(spec: &str) {
    for directive in spec.split(',').map(str::trim).filter(|d| !d.is_empty()) {
        if directive.contains('=') || directive.parse::<u8>().is_ok() {
            continue;
        }
        if !LOG_LEVELS.iter().any(|lvl| lvl.eq_ignore_ascii_case(directive)) {
            eprintln!(
                "scout: SCOUT_LOG=`{directive}` reads as a target name, not a level, so scout's \
                 own logs stay hidden. Levels are {}. To raise scout only: SCOUT_LOG=scout=debug",
                LOG_LEVELS.join(", ")
            );
        }
    }
}

/// The filter from `SCOUT_LOG` (a `tracing` directive: `debug`,
/// `scout::index=trace`, ...). Default `info`. Scout-specific rather than
/// `RUST_LOG` on purpose: a developer with `RUST_LOG=debug` exported for
/// another tool should not have scout start writing to their log file.
fn filter() -> tracing_subscriber::EnvFilter {
    const DEFAULT: &str = "info";
    match std::env::var("SCOUT_LOG") {
        Ok(spec) if !spec.is_empty() => match tracing_subscriber::EnvFilter::try_new(&spec) {
            Ok(filter) => {
                warn_on_bare_non_level(&spec);
                filter
            }
            Err(err) => {
                eprintln!(
                    "scout: SCOUT_LOG=`{spec}` is not a valid filter ({err}); using `{DEFAULT}`"
                );
                tracing_subscriber::EnvFilter::new(DEFAULT)
            }
        },
        _ => tracing_subscriber::EnvFilter::new(DEFAULT),
    }
}

/// Initialise tracing once. Safe to call more than once; later calls are
/// no-ops.
pub fn init(sink: Sink) {
    match sink {
        Sink::StateFile => {
            if let Ok(dir) = crate::locations::state_dir() {
                if std::fs::create_dir_all(&dir).is_ok() {
                    if let Ok(file) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(dir.join("scout.log"))
                    {
                        let _ = tracing_subscriber::fmt()
                            .with_env_filter(filter())
                            .with_writer(std::sync::Mutex::new(file))
                            .with_ansi(false)
                            .try_init();
                    }
                }
            }
        }
        Sink::Stderr => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(filter())
                .with_writer(std::io::stderr)
                .try_init();
        }
    }
}
