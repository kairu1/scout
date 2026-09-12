//! The ways scout starts a child: wait for it on the terminal, detach it
//! so it can never wedge the terminal scout is drawing on, or become it.

use std::collections::HashMap;
use std::io;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

fn command(argv: &[String], cwd: &Path, env: &HashMap<String, String>) -> Command {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).env_clear().envs(env).current_dir(cwd);
    command
}

/// Run `argv` with the given environment and working directory, sharing
/// scout's stdio, and wait for it.
pub fn spawn_wait(
    argv: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
) -> io::Result<ExitStatus> {
    command(argv, cwd, env).status()
}

/// The user's shell as a one-element argv: `$SHELL`, else `sh`.
pub fn shell_argv() -> Vec<String> {
    vec![std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "sh".into())]
}

/// Replace this process with the user's shell (`$SHELL`, else `sh`) in
/// `cwd`. Only returns on failure.
pub fn exec_shell(cwd: &Path) -> io::Error {
    Command::new(&shell_argv()[0]).current_dir(cwd).exec()
}

/// Replace this process with `argv`, environment and directory inherited.
/// Only returns on failure. What the session launcher does to become the
/// tmux client.
pub fn exec_argv(argv: &[String]) -> io::Error {
    if argv.is_empty() {
        return io::Error::new(io::ErrorKind::InvalidInput, "empty argv");
    }
    Command::new(&argv[0]).args(&argv[1..]).exec()
}

/// Start `argv` in its own process group with null stdio and return at
/// once. The child outlives scout and cannot touch its terminal.
pub fn spawn_detached(
    argv: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
) -> io::Result<()> {
    let mut child = command(argv, cwd, env)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // A session lives long enough for unreaped children to pile up as
    // zombies; one waiting thread per child reaps it whenever it ends.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
