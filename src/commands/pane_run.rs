//! `scout pane-run --cwd DIR -- ARGV...`: the command tmux starts in a pane
//! scout opened. It runs the argv in the directory, prints one status
//! line, then becomes the user's shell in that directory, so the pane
//! stays useful after a quick command instead of closing. Every hop is
//! argv; no shell parses anything scout templated.
//!
//! Hidden from `--help`: it exists to be tmux's child. Without a terminal
//! it runs the command and exits with its status, which is what makes it
//! testable.

use std::io::IsTerminal;
use std::path::Path;
use std::time::Instant;

use crate::platform::process;
use crate::Error;

pub fn pane_run(cwd: &Path, argv: &[String]) -> crate::Result<u8> {
    if argv.is_empty() {
        return Err(Error::UnknownFormat { given: String::new(), wanted: "a command after --" });
    }
    let env: std::collections::HashMap<String, String> = std::env::vars().collect();
    let started = Instant::now();
    let code = match process::spawn_wait(argv, cwd, &env) {
        Ok(status) => status.code().unwrap_or(1),
        Err(err) => {
            eprintln!("scout: {}: {err}", argv[0]);
            if err.kind() == std::io::ErrorKind::NotFound {
                127
            } else {
                126
            }
        }
    };
    eprintln!("scout: {}: exit {code} in {:.1} s", argv[0], started.elapsed().as_secs_f64());
    if !std::io::stdin().is_terminal() {
        return Ok(code.clamp(0, 255) as u8);
    }
    // Becomes the shell; only returns on failure.
    let err = process::exec_shell(cwd);
    Err(Error::io("exec shell", err))
}
