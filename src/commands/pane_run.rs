//! `scout pane-run --cwd DIR -- ARGV...`: the command tmux starts in a pane
//! scout opened. It runs the argv in the directory, prints one status
//! line, then becomes the user's shell in that directory, so the pane
//! stays useful after a quick command instead of closing. Every hop is
//! argv; no shell parses anything scout templated.
//!
//! Hidden from `--help`: it exists to be tmux's child. Without a terminal
//! it runs the command and exits with its status, which is what makes it
//! testable.
//!
//! Owns: running one argv in one directory with the sanitised
//! environment and becoming the shell afterwards. Refuses to know about:
//! tmux, actions, the picker. Exposes: `pane_run`.

use std::io::IsTerminal;
use std::path::Path;
use std::time::Instant;

use crate::platform::process;
use crate::Error;

pub fn pane_run(cwd: &Path, argv: &[String]) -> crate::Result<u8> {
    if argv.is_empty() {
        return Err(Error::UnknownFormat { given: String::new(), wanted: "a command after --" });
    }
    // The same environment an in-process child gets: `.` and empty
    // entries stripped from PATH, so a program planted in the selected
    // project is never the one that runs.
    let env = crate::actions::sanitized_process_env();
    let started = Instant::now();
    let code = match process::spawn_wait(argv, cwd, &env) {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("scout: {}: {err}", crate::ui::strip::clean(&argv[0]));
            if err.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            }
        }
    };
    eprintln!(
        "scout: {}: exit {code} in {:.1} s",
        crate::ui::strip::clean(&argv[0]),
        started.elapsed().as_secs_f64()
    );
    if !std::io::stdin().is_terminal() {
        return Ok(code.clamp(0, 255) as u8);
    }
    // Becomes the shell; only returns on failure, and then tries `sh` so
    // the pane is still a shell rather than gone.
    let err = process::exec_shell(cwd);
    eprintln!("scout: could not start $SHELL ({err}); starting sh");
    let err = process::exec_argv(&["sh".to_string()]);
    Err(Error::io("exec shell", err))
}
