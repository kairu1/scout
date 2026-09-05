//! Translating scout's named pane operations into tmux commands.
//!
//! Owns: detecting tmux (once, at startup), building the argv for each
//! operation, remembering the panes scout opened, and running the
//! `pane-run` shape that keeps a pane alive as a shell after its command.
//! Refuses to know about: the picker, actions, the index. It takes a path
//! and an argv and returns whether tmux did as asked.
//! Exposes: `Tmux`, `Tmux::detect`, `Tmux::argv`, `Tmux::run`.
//!
//! Everything is argv: tmux runs the pane's command directly when it is
//! given as several arguments after `--`, so no shell ever parses a
//! scout-templated string. Two facts about tmux's own parser are handled
//! here: a single trailing string would go through the user's shell (so
//! scout never sends one), and an argument ending in `;` is a command
//! separator (so a trailing `;` is written `\;`).

use std::path::Path;
use std::process::Command;

use crate::config::keys::Operation;

/// A tmux session scout is running inside.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// scout's own pane, from `display-message -p '#{pane_id}'`.
    pub own_pane: String,
    /// Panes scout opened this session, newest last. `close-pane` only
    /// ever closes one of these.
    pub opened: Vec<String>,
    /// The scout binary a new pane runs `pane-run` from: this executable,
    /// so it works even when `scout` is not on the pane's PATH.
    pub scout_exe: std::path::PathBuf,
}

impl Tmux {
    /// `TMUX` set and non-empty, and tmux answering for this client. The
    /// variable alone is a hint (it survives a dead server, and can be
    /// exported by hand); the round trip is the proof.
    pub fn detect() -> Option<Tmux> {
        let tmux_var = std::env::var("TMUX").ok().filter(|v| !v.is_empty())?;
        let _ = tmux_var;
        let output =
            Command::new("tmux").args(["display-message", "-p", "#{pane_id}"]).output().ok()?;
        if !output.status.success() {
            return None;
        }
        let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !pane.starts_with('%') {
            return None;
        }
        let scout_exe = std::env::current_exe().unwrap_or_else(|_| "scout".into());
        Some(Tmux { own_pane: pane, opened: Vec::new(), scout_exe })
    }

    /// The tmux argv (without the leading `tmux`) for an operation at
    /// `path`, running `command` in the new pane when one is given. Focus,
    /// zoom and close take neither. `None` for `reindex`, which is not a
    /// tmux operation.
    pub fn argv(
        &self,
        op: Operation,
        path: Option<&Path>,
        command: Option<&[String]>,
    ) -> Option<Vec<String>> {
        let mut argv: Vec<String> = match op {
            Operation::SplitRight => vec!["split-window".into(), "-h".into()],
            Operation::SplitDown => vec!["split-window".into(), "-v".into()],
            Operation::NewWindow => vec!["new-window".into()],
            Operation::FocusLeft => vec!["select-pane".into(), "-L".into()],
            Operation::FocusRight => vec!["select-pane".into(), "-R".into()],
            Operation::FocusUp => vec!["select-pane".into(), "-U".into()],
            Operation::FocusDown => vec!["select-pane".into(), "-D".into()],
            Operation::Zoom => vec!["resize-pane".into(), "-Z".into()],
            Operation::ClosePane => {
                let pane = self.opened.last()?;
                vec!["kill-pane".into(), "-t".into(), pane.clone()]
            }
            Operation::Reindex => return None,
        };
        if matches!(op, Operation::SplitRight | Operation::SplitDown | Operation::NewWindow) {
            // Print the new pane's id so it can be closed later.
            argv.extend(["-P".into(), "-F".into(), "#{pane_id}".into()]);
            if let Some(path) = path {
                argv.push("-c".into());
                argv.push(path.display().to_string());
            }
            if let Some(command) = command {
                // The pane runs `scout pane-run`, which runs the command and
                // then becomes a shell in the same directory, so the pane
                // does not vanish when a quick command exits.
                argv.push("--".into());
                argv.push(self.scout_exe.display().to_string());
                argv.push("pane-run".into());
                if let Some(path) = path {
                    argv.push("--cwd".into());
                    argv.push(path.display().to_string());
                }
                argv.push("--".into());
                argv.extend(command.iter().cloned());
            }
        }
        Some(argv.into_iter().map(escape_trailing_semicolon).collect())
    }

    /// Run an operation. Split and window operations remember the pane
    /// they opened. Errors carry tmux's stderr.
    pub fn run(
        &mut self,
        op: Operation,
        path: Option<&Path>,
        command: Option<&[String]>,
    ) -> Result<(), String> {
        let Some(argv) = self.argv(op, path, command) else {
            return Err(match op {
                Operation::ClosePane => "no pane opened by scout to close".to_string(),
                _ => format!("{} is not a tmux operation", op.name()),
            });
        };
        let output = Command::new("tmux").args(&argv).output().map_err(|e| format!("tmux: {e}"))?;
        if !output.status.success() {
            return Err(format!("tmux: {}", String::from_utf8_lossy(&output.stderr).trim()));
        }
        match op {
            Operation::SplitRight | Operation::SplitDown | Operation::NewWindow => {
                let pane = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if pane.starts_with('%') {
                    self.opened.push(pane);
                }
            }
            Operation::ClosePane => {
                self.opened.pop();
            }
            _ => {}
        }
        Ok(())
    }
}

/// tmux treats an argument ending in `;` as a command separator; `\;` is
/// its escape. Every element scout hands over is escaped so a directory
/// named `x;` opens as a directory rather than truncating the command.
pub fn escape_trailing_semicolon(arg: String) -> String {
    if arg.ends_with(';') && !arg.ends_with("\\;") {
        let mut out = arg;
        out.pop();
        out.push_str("\\;");
        out
    } else {
        arg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmux() -> Tmux {
        Tmux { own_pane: "%0".into(), opened: vec!["%7".into()], scout_exe: "/opt/scout".into() }
    }

    #[test]
    fn operations_translate_to_the_documented_argv() {
        let t = tmux();
        let path = Path::new("/home/u/w/api");
        assert_eq!(
            t.argv(Operation::SplitRight, Some(path), None).unwrap(),
            ["split-window", "-h", "-P", "-F", "#{pane_id}", "-c", "/home/u/w/api"]
        );
        assert_eq!(
            t.argv(Operation::SplitDown, Some(path), None).unwrap(),
            ["split-window", "-v", "-P", "-F", "#{pane_id}", "-c", "/home/u/w/api"]
        );
        assert_eq!(
            t.argv(Operation::NewWindow, Some(path), None).unwrap(),
            ["new-window", "-P", "-F", "#{pane_id}", "-c", "/home/u/w/api"]
        );
        assert_eq!(t.argv(Operation::FocusLeft, None, None).unwrap(), ["select-pane", "-L"]);
        assert_eq!(t.argv(Operation::FocusRight, None, None).unwrap(), ["select-pane", "-R"]);
        assert_eq!(t.argv(Operation::FocusUp, None, None).unwrap(), ["select-pane", "-U"]);
        assert_eq!(t.argv(Operation::FocusDown, None, None).unwrap(), ["select-pane", "-D"]);
        assert_eq!(t.argv(Operation::Zoom, None, None).unwrap(), ["resize-pane", "-Z"]);
        assert_eq!(t.argv(Operation::ClosePane, None, None).unwrap(), ["kill-pane", "-t", "%7"]);
        assert_eq!(t.argv(Operation::Reindex, None, None), None);
    }

    #[test]
    fn close_pane_only_closes_a_pane_scout_opened() {
        let t = Tmux { own_pane: "%0".into(), opened: Vec::new(), scout_exe: "/opt/scout".into() };
        assert_eq!(t.argv(Operation::ClosePane, None, None), None);
    }

    #[test]
    fn a_command_runs_through_pane_run_as_separate_arguments() {
        let t = tmux();
        let argv = t
            .argv(
                Operation::SplitRight,
                Some(Path::new("/w/api")),
                Some(&[
                    "cargo".to_string(),
                    "test".to_string(),
                    "--".to_string(),
                    "it's".to_string(),
                ]),
            )
            .unwrap();
        let dashes = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[dashes + 1], "/opt/scout");
        assert_eq!(argv[dashes + 2], "pane-run");
        assert_eq!(&argv[dashes + 3..dashes + 5], ["--cwd", "/w/api"]);
        assert_eq!(&argv[dashes + 5..], ["--", "cargo", "test", "--", "it's"]);
        // No element is a shell command line: the apostrophe survives intact.
    }

    #[test]
    fn a_trailing_semicolon_is_escaped_in_every_element() {
        let t = tmux();
        let argv = t.argv(Operation::NewWindow, Some(Path::new("/w/x;")), None).unwrap();
        assert!(argv.contains(&"/w/x\\;".to_string()), "{argv:?}");
        assert_eq!(escape_trailing_semicolon("a;b".into()), "a;b", "only a trailing one matters");
        assert_eq!(escape_trailing_semicolon("x\\;".into()), "x\\;", "not doubled");
        assert_eq!(escape_trailing_semicolon("plain".into()), "plain");
    }
}
