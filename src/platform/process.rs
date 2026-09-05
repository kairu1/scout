//! The two ways scout starts a child: wait for it on the terminal, or
//! detach it so it can never wedge the terminal scout is drawing on.

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

/// Start `argv` in its own process group with null stdio and return at
/// once. The child outlives scout and cannot touch its terminal.
pub fn spawn_detached(
    argv: &[String],
    cwd: &Path,
    env: &HashMap<String, String>,
) -> io::Result<()> {
    command(argv, cwd, env)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
}
