//! Translating scout's named pane operations into tmux commands, and
//! starting the tmux session a `scout -s` runs inside.
//!
//! Owns: detecting tmux (once, at startup), building the argv for each
//! operation, remembering the panes scout opened, running the `pane-run`
//! shape that keeps a pane alive as a shell after its command, the launch
//! that turns a `scout -s` outside tmux into a picker inside one, and the
//! detach that hands the terminal back. Refuses to know about: the
//! picker, actions, the index. It takes a path and an argv and returns
//! whether tmux did as asked.
//! Exposes: `Tmux`, `Tmux::detect`, `Tmux::argv`, `Tmux::run`, `Policy`,
//! `Server`, `Launch`, `decide`, `Decision`, `handoff_sink`.
//!
//! Everything is argv: tmux runs the pane's command directly when it is
//! given as several arguments after `--`, so no shell ever parses a
//! scout-templated string. Two facts about tmux's own parser are handled
//! here: a single trailing string would go through the user's shell (so
//! scout never sends one), and an argument ending in `;` is a command
//! separator (so a trailing `;` is written `\;`). A third fact shapes the
//! launch: `-t NAME` matches a session by prefix, so every session target
//! is written `=NAME`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::keys::{tmux_key_name, Keys, Operation};
use crate::platform::fs as pfs;

/// The server name scout's own sessions live on (`tmux -L scout`). One
/// constant: it never comes from a file.
pub const PRIVATE_SERVER: &str = "scout";

/// The default session name, overridable by `[scout] tmux_session`.
pub const DEFAULT_SESSION: &str = "scout";

/// The pane option that marks the picker pane, so a later launch can
/// find a live picker. A pane option dies with its pane.
pub const ROLE_OPTION: &str = "@scout_role";

/// The session environment variable naming the most recent launcher's
/// `--print-to` file.
pub const PRINT_TO_VAR: &str = "SCOUT_PRINT_TO";

/// Whether session mode may start tmux (`[scout] tmux`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// Start tmux when it is installed; use it when already inside;
    /// otherwise the in-process session.
    #[default]
    Auto,
    /// Never touch tmux, even inside one.
    Never,
    /// Refuse to run a session without tmux.
    Require,
}

impl Policy {
    pub fn parse(s: &str) -> Option<Policy> {
        match s {
            "auto" => Some(Policy::Auto),
            "never" => Some(Policy::Never),
            "require" => Some(Policy::Require),
            _ => None,
        }
    }
}

/// Which tmux server the session lives on (`[scout] tmux_server`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Server {
    /// `tmux -L scout`: scout's own server, reading the user's config,
    /// with scout's options set in memory on top.
    #[default]
    Private,
    /// The user's default server. Scout sets no server options there.
    Shared,
}

impl Server {
    pub fn parse(s: &str) -> Option<Server> {
        match s {
            "private" => Some(Server::Private),
            "shared" => Some(Server::Shared),
            _ => None,
        }
    }

    /// The leading arguments that select the server.
    fn args(self) -> Vec<String> {
        match self {
            Server::Private => vec!["-L".into(), PRIVATE_SERVER.into()],
            Server::Shared => Vec::new(),
        }
    }
}

/// A session name tmux will not rewrite and scout's argv cannot be
/// confused by: letters, digits, `_` and `-`, one to 64 of them.
pub fn validate_session_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("must be 1 to 64 characters".into());
    }
    if let Some(bad) = name.chars().find(|c| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-'))
    {
        return Err(format!("character {bad:?} is not allowed (letters, digits, _ and - only)"));
    }
    Ok(())
}

/// What session mode should do about tmux, given the policy and the
/// facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Outside tmux, tmux installed: start (or attach to) scout's session.
    Launch,
    /// Already inside tmux: use the surrounding session.
    UseSurrounding,
    /// Run the session in this terminal, no panes.
    InProcess,
    /// `require` and no tmux.
    Refuse,
}

/// The policy table. `no_tmux` is the `--no-tmux` flag.
pub fn decide(policy: Policy, no_tmux: bool, installed: bool, inside: bool) -> Decision {
    if no_tmux || policy == Policy::Never {
        return Decision::InProcess;
    }
    if inside {
        return Decision::UseSurrounding;
    }
    if installed {
        return Decision::Launch;
    }
    match policy {
        Policy::Require => Decision::Refuse,
        _ => Decision::InProcess,
    }
}

/// True when a `tmux` binary answers on PATH.
pub fn installed() -> bool {
    Command::new("tmux").arg("-V").output().map(|o| o.status.success()).unwrap_or(false)
}

/// True when this process was started inside a tmux pane. A hint, as in
/// `Tmux::detect`; the round trip there is the proof.
pub fn inside() -> bool {
    std::env::var("TMUX").map(|v| !v.is_empty()).unwrap_or(false)
}

/// Everything a launch needs, all decided before any tmux command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub server: Server,
    pub session: String,
    /// The scout binary the picker runs from.
    pub scout_exe: PathBuf,
    /// The directory the session opens in; `None` leaves tmux's default.
    pub cwd: Option<PathBuf>,
    /// The launcher's `--print-to` file, forwarded to the picker and
    /// recorded in the session environment.
    pub print_to: Option<PathBuf>,
}

impl Launch {
    fn target(&self) -> String {
        format!("={}", self.session)
    }

    /// The picker's own argv, run by tmux as the pane's command.
    pub fn inner_argv(&self) -> Vec<String> {
        let mut argv =
            vec![self.scout_exe.display().to_string(), "--session".into(), "--tmux-owned".into()];
        if let Some(file) = &self.print_to {
            argv.push("--print-to".into());
            argv.push(file.display().to_string());
        }
        argv
    }

    fn cwd_args(&self) -> Vec<String> {
        // A directory whose name carries a control byte is not handed to
        // tmux's parser; the session opens where tmux would open it.
        match &self.cwd {
            Some(cwd) if !cwd.as_os_str().as_encoded_bytes().iter().any(|b| *b < 0x20) => {
                vec!["-c".into(), cwd.display().to_string()]
            }
            _ => Vec::new(),
        }
    }

    /// `new-session -d -s NAME [-c CWD] -- <picker argv>`: a detached
    /// session whose first pane is the picker. Server-selecting arguments
    /// first, every element escaped.
    pub fn new_session_argv(&self) -> Vec<String> {
        let mut argv = self.server.args();
        argv.extend(["new-session".into(), "-d".into(), "-s".into(), self.session.clone()]);
        argv.extend(self.cwd_args());
        argv.push("--".into());
        argv.extend(self.inner_argv());
        argv.into_iter().map(escape_trailing_semicolon).collect()
    }

    /// `new-window -t =NAME [-c CWD] -- <picker argv>`: a fresh picker in
    /// a session that has none.
    pub fn new_window_argv(&self) -> Vec<String> {
        let mut argv = self.server.args();
        argv.extend(["new-window".into(), "-t".into(), self.target()]);
        argv.extend(self.cwd_args());
        argv.push("--".into());
        argv.extend(self.inner_argv());
        argv.into_iter().map(escape_trailing_semicolon).collect()
    }

    /// `attach-session -t =NAME`: what the launcher becomes.
    pub fn attach_argv(&self) -> Vec<String> {
        let mut argv = self.server.args();
        argv.extend(["attach-session".into(), "-t".into(), self.target()]);
        argv
    }

    /// The full client argv to `exec`. `-u` tells the client the terminal
    /// takes UTF-8: the picker draws with UTF-8 glyphs anyway, and without
    /// the flag a shell with no UTF-8 locale gets `_` for every corner.
    pub fn client_argv(&self) -> Vec<String> {
        let mut client = vec!["tmux".to_string(), "-u".to_string()];
        client.extend(self.attach_argv());
        client
    }

    /// The server options scout sets on a server it started: modified
    /// keys reported distinctly, and no half-second hold on Esc.
    pub fn owned_server_options(&self) -> Vec<Vec<String>> {
        if self.server != Server::Private {
            return Vec::new();
        }
        [["extended-keys", "on"], ["escape-time", "10"]]
            .iter()
            .map(|[k, v]| {
                let mut argv = self.server.args();
                argv.extend(["set-option".into(), "-s".into(), (*k).into(), (*v).into()]);
                argv
            })
            .collect()
    }

    fn tmux(&self, argv: &[String]) -> Result<String, String> {
        let output = Command::new("tmux").args(argv).output().map_err(|e| format!("tmux: {e}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(format!("tmux: {}", String::from_utf8_lossy(&output.stderr).trim()))
        }
    }

    fn has_session(&self) -> bool {
        let mut argv = self.server.args();
        argv.extend(["has-session".into(), "-t".into(), self.target()]);
        self.tmux(&argv).is_ok()
    }

    /// The pane id of a live picker in the session, if one is running.
    pub fn live_picker_pane(&self) -> Option<String> {
        let mut argv = self.server.args();
        argv.extend([
            "list-panes".into(),
            "-s".into(),
            "-t".into(),
            self.target(),
            "-F".into(),
            // A tab in a format comes back as `_`; fields are space-separated
            // and none of these three can contain a space.
            format!("#{{pane_id}} #{{{}}} #{{pane_dead}}", ROLE_OPTION),
        ]);
        let listing = self.tmux(&argv).ok()?;
        listing.lines().find_map(|line| {
            let mut fields = line.split(' ');
            let pane = fields.next()?;
            let role = fields.next()?;
            let dead = fields.next()?;
            (role == "picker" && dead == "0" && pane.starts_with('%')).then(|| pane.to_string())
        })
    }

    /// Make the session ready to attach to: create it with a picker, or
    /// focus its live picker, or open a fresh picker in it; record the
    /// launcher's file; then return the client argv to `exec`. Every
    /// step is its own tmux invocation, never a `;`-chained one.
    pub fn prepare(&self) -> Result<Vec<String>, String> {
        if self.has_session() {
            match self.live_picker_pane() {
                Some(pane) => {
                    for verb in ["select-window", "select-pane"] {
                        let mut argv = self.server.args();
                        argv.extend([verb.into(), "-t".into(), pane.clone()]);
                        self.tmux(&argv)?;
                    }
                }
                None => {
                    self.tmux(&self.new_window_argv())?;
                }
            }
        } else {
            self.tmux(&self.new_session_argv())?;
            for option in self.owned_server_options() {
                // An older tmux without an option is not an error.
                if let Err(err) = self.tmux(&option) {
                    tracing::warn!(%err, "tmux option not set");
                }
            }
        }
        if let Some(file) = &self.print_to {
            let mut argv = self.server.args();
            argv.extend([
                "set-environment".into(),
                "-t".into(),
                self.target(),
                PRINT_TO_VAR.into(),
                file.display().to_string(),
            ]);
            self.tmux(&argv.into_iter().map(escape_trailing_semicolon).collect::<Vec<_>>())?;
        }
        Ok(self.client_argv())
    }
}

/// A path handed over through the session environment is accepted as a
/// print sink only when it is a regular file scout could have been
/// given directly: owned by the effective uid, private, not a symlink.
pub fn handoff_sink(path: &Path) -> Option<PathBuf> {
    let facts = pfs::facts(path).ok()?;
    if facts.is_symlink || !facts.is_file || facts.uid != pfs::euid() || facts.mode & 0o077 != 0 {
        return None;
    }
    Some(path.to_path_buf())
}

/// A tmux session scout is running inside.
#[derive(Debug, Clone)]
pub struct Tmux {
    /// scout's own pane, from `display-message -p '#{pane_id}'`.
    pub own_pane: String,
    /// The session scout's pane belongs to.
    pub session: String,
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
        if !inside() {
            return None;
        }
        let output = Command::new("tmux")
            .args(["display-message", "-p", "#{pane_id} #{session_name}"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let answer = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // The pane id carries no space; whatever follows is the session name.
        let (pane, session) = answer.split_once(' ')?;
        if !pane.starts_with('%') {
            return None;
        }
        let scout_exe = std::env::current_exe().unwrap_or_else(|_| "scout".into());
        Some(Tmux {
            own_pane: pane.to_string(),
            session: session.to_string(),
            opened: Vec::new(),
            scout_exe,
        })
    }

    /// Mark this pane as the picker and name this picker's file in the
    /// session environment, so a later launch finds both. Failures are
    /// logged: a launch that cannot find the picker opens another.
    pub fn claim_picker(&self, print_to: Option<&Path>) {
        let mark = ["set-option", "-p", "-t", &self.own_pane, ROLE_OPTION, "picker"];
        if let Err(err) = run_tmux(&mark.map(String::from)) {
            tracing::warn!(%err, "could not mark the picker pane");
        }
        if let Some(file) = print_to {
            let argv: Vec<String> = [
                "set-environment".to_string(),
                "-t".into(),
                format!("={}", self.session),
                PRINT_TO_VAR.into(),
                file.display().to_string(),
            ]
            .into_iter()
            .map(escape_trailing_semicolon)
            .collect();
            if let Err(err) = run_tmux(&argv) {
                tracing::warn!(%err, "could not record the print-to file");
            }
        }
    }

    /// The print sink the most recent launcher asked for, when it passes
    /// `handoff_sink`; else `None` and the caller keeps its own.
    pub fn handoff_print_to(&self) -> Option<PathBuf> {
        let argv = ["show-environment", "-t", &format!("={}", self.session), PRINT_TO_VAR]
            .map(String::from);
        let line = run_tmux(&argv).ok()?;
        let value = line.trim().strip_prefix(&format!("{PRINT_TO_VAR}="))?;
        handoff_sink(Path::new(value))
    }

    /// The tmux `bind-key -n` argvs that make the operations which work
    /// from any pane answer to the same keys everywhere on this server:
    /// focus moves, zoom, and focus-picker aimed at this pane. Installed
    /// only on a server scout owns; a shared server's root key table is
    /// the user's.
    pub fn root_bindings(&self, keys: &Keys) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        for (op, chord) in keys.iter() {
            if !op.works_from_any_pane() {
                continue;
            }
            let Some(key) = tmux_key_name(chord) else { continue };
            let command: Vec<String> = match op {
                // Kill the pane the key came from, unless it is the
                // picker's: the role option marks that one, and the
                // format check runs on the pane that received the key.
                Operation::KillPane => vec![
                    "if-shell".into(),
                    "-F".into(),
                    format!("#{{{}}}", ROLE_OPTION),
                    "display-message the picker leaves with esc".into(),
                    "kill-pane".into(),
                ],
                _ => match self.argv(op, None, None) {
                    Some(command) => command,
                    None => continue,
                },
            };
            let mut argv = vec!["bind-key".to_string(), "-n".to_string(), key];
            argv.extend(command);
            out.push(argv);
        }
        out
    }

    /// Install the root bindings; failures are logged, the picker's own
    /// keys still work in its pane.
    pub fn bind_root_keys(&self, keys: &Keys) {
        for argv in self.root_bindings(keys) {
            if let Err(err) = run_tmux(&argv) {
                tracing::warn!(%err, "could not install a tmux key binding");
            }
        }
    }

    /// Remove what `bind_root_keys` installed. Called when the picker
    /// leaves: the focus-picker target is about to vanish.
    pub fn unbind_root_keys(&self, keys: &Keys) {
        for argv in self.root_bindings(keys) {
            let unbind = ["unbind-key".to_string(), "-n".to_string(), argv[2].clone()];
            let _ = run_tmux(&unbind);
        }
    }

    /// Detach every client of this session, returning the terminal to
    /// whoever ran the launcher. Nothing is killed.
    pub fn detach_clients(&self) -> Result<(), String> {
        let argv = ["detach-client", "-s", &format!("={}", self.session)].map(String::from);
        run_tmux(&argv).map(|_| ())
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
            Operation::FocusPicker => {
                vec!["select-pane".into(), "-t".into(), self.own_pane.clone()]
            }
            // From the picker there is no pane to kill but its own, and
            // that one leaves with Esc. The tmux-wide binding is the
            // operation's real home; see `root_bindings`.
            Operation::KillPane | Operation::Reindex => return None,
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
                Operation::KillPane => {
                    "kill-pane acts on the pane you press it in; the picker leaves with esc"
                        .to_string()
                }
                _ => format!("{} is not a tmux operation", op.name()),
            });
        };
        let stdout = run_tmux(&argv)?;
        match op {
            Operation::SplitRight | Operation::SplitDown | Operation::NewWindow => {
                let pane = stdout.trim().to_string();
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

/// One tmux invocation against the server `TMUX` names; stdout on
/// success, stderr in the error.
fn run_tmux(argv: &[String]) -> Result<String, String> {
    let output = Command::new("tmux").args(argv).output().map_err(|e| format!("tmux: {e}"))?;
    if !output.status.success() {
        return Err(format!("tmux: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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
        Tmux {
            own_pane: "%0".into(),
            session: "scout".into(),
            opened: vec!["%7".into()],
            scout_exe: "/opt/scout".into(),
        }
    }

    fn launch() -> Launch {
        Launch {
            server: Server::Private,
            session: "scout".into(),
            scout_exe: "/opt/scout".into(),
            cwd: Some("/home/u/w".into()),
            print_to: Some("/tmp/scout.abc".into()),
        }
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
    fn root_bindings_cover_focus_zoom_and_the_picker_and_nothing_else() {
        let t = tmux();
        let bindings = t.root_bindings(&Keys::default());
        assert_eq!(bindings.len(), 7, "{bindings:?}");
        let kill = bindings.iter().find(|b| b[2] == "M-q").expect("kill-pane bound");
        assert_eq!(kill[3], "if-shell");
        assert_eq!(kill[5], "#{@scout_role}", "guarded on the pane's role");
        assert_eq!(kill[7], "kill-pane");
        assert_eq!(t.argv(Operation::KillPane, None, None), None, "never from the picker itself");
        assert!(bindings.contains(
            &["bind-key", "-n", "M-S-Left", "select-pane", "-L"].map(String::from).to_vec()
        ));
        assert!(bindings
            .contains(&["bind-key", "-n", "M-z", "resize-pane", "-Z"].map(String::from).to_vec()));
        assert!(bindings.contains(
            &["bind-key", "-n", "M-h", "select-pane", "-t", "%0"].map(String::from).to_vec()
        ));
        assert!(
            !bindings.iter().any(|b| b.contains(&"split-window".to_string())),
            "splits stay the picker's: {bindings:?}"
        );
        assert_eq!(
            bindings.iter().filter(|b| b.contains(&"kill-pane".to_string())).count(),
            1,
            "only the guarded kill-pane binding kills anything: {bindings:?}"
        );
        assert_eq!(
            t.argv(Operation::FocusPicker, None, None).unwrap(),
            ["select-pane", "-t", "%0"]
        );
    }

    #[test]
    fn close_pane_only_closes_a_pane_scout_opened() {
        let t = Tmux { opened: Vec::new(), ..tmux() };
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

    #[test]
    fn a_session_name_is_letters_digits_underscore_and_dash() {
        assert!(validate_session_name("scout").is_ok());
        assert!(validate_session_name("work-2_b").is_ok());
        assert!(validate_session_name("").is_err());
        assert!(validate_session_name(&"a".repeat(65)).is_err());
        for bad in ["a;b", "a.b", "a:b", "a b", "a\nb", "sc\u{e9}out", "x;"] {
            assert!(validate_session_name(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn the_policy_table_decides_launch_surrounding_in_process_or_refuse() {
        use Decision::*;
        use Policy::*;
        // (policy, --no-tmux, installed, inside) -> decision
        let table = [
            (Auto, false, true, false, Launch),
            (Auto, false, true, true, UseSurrounding),
            (Auto, false, false, false, InProcess),
            (Auto, false, false, true, UseSurrounding),
            (Auto, true, true, true, InProcess),
            (Never, false, true, true, InProcess),
            (Never, false, true, false, InProcess),
            (Require, false, true, false, Launch),
            (Require, false, true, true, UseSurrounding),
            (Require, false, false, false, Refuse),
            (Require, true, true, false, InProcess),
        ];
        for (policy, no_tmux, installed, inside, want) in table {
            assert_eq!(
                decide(policy, no_tmux, installed, inside),
                want,
                "{policy:?} no_tmux={no_tmux} installed={installed} inside={inside}"
            );
        }
    }

    #[test]
    fn a_fresh_launch_creates_a_detached_session_running_the_picker() {
        let l = launch();
        assert_eq!(
            l.new_session_argv(),
            [
                "-L",
                "scout",
                "new-session",
                "-d",
                "-s",
                "scout",
                "-c",
                "/home/u/w",
                "--",
                "/opt/scout",
                "--session",
                "--tmux-owned",
                "--print-to",
                "/tmp/scout.abc"
            ]
        );
        assert_eq!(l.attach_argv(), ["-L", "scout", "attach-session", "-t", "=scout"]);
        assert_eq!(
            l.client_argv(),
            ["tmux", "-u", "-L", "scout", "attach-session", "-t", "=scout"],
            "the client is told the terminal takes UTF-8"
        );
        let win = l.new_window_argv();
        assert_eq!(&win[..5], ["-L", "scout", "new-window", "-t", "=scout"]);
        assert!(win.ends_with(&["--print-to".to_string(), "/tmp/scout.abc".to_string()]));
        // Exact-match targets: a session named scoutier is not scout.
        assert!(l.attach_argv().contains(&"=scout".to_string()));
    }

    #[test]
    fn a_shared_server_gets_no_server_options_and_no_socket_argument() {
        let l = Launch { server: Server::Shared, ..launch() };
        assert_eq!(&l.new_session_argv()[..3], ["new-session", "-d", "-s"]);
        assert!(l.owned_server_options().is_empty());
        let private = launch();
        let options = private.owned_server_options();
        assert_eq!(options.len(), 2);
        assert_eq!(options[0], ["-L", "scout", "set-option", "-s", "extended-keys", "on"]);
        assert_eq!(options[1], ["-L", "scout", "set-option", "-s", "escape-time", "10"]);
    }

    #[test]
    fn the_launch_argv_escapes_a_trailing_semicolon_and_drops_a_control_byte_cwd() {
        let l = Launch { cwd: Some("/w/x;".into()), print_to: None, ..launch() };
        let argv = l.new_session_argv();
        assert!(argv.contains(&"/w/x\\;".to_string()), "{argv:?}");
        assert!(!argv.contains(&"--print-to".to_string()));
        let l = Launch { cwd: Some("/w/bad\nname".into()), ..launch() };
        assert!(!l.new_session_argv().contains(&"-c".to_string()));
    }

    #[test]
    fn a_handoff_sink_must_be_a_private_regular_file_the_user_owns() {
        let dir = std::env::temp_dir().join(format!("scout-handoff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good");
        std::fs::write(&good, b"").unwrap();
        pfs::set_private(&good).unwrap();
        assert_eq!(handoff_sink(&good), Some(good.clone()));

        let loose = dir.join("loose");
        std::fs::write(&loose, b"").unwrap();
        std::fs::set_permissions(
            &loose,
            <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o644),
        )
        .unwrap();
        assert_eq!(handoff_sink(&loose), None, "group/other readable");

        let link = dir.join("link");
        std::os::unix::fs::symlink(&good, &link).unwrap();
        assert_eq!(handoff_sink(&link), None, "a symlink, even to a good file");
        assert_eq!(handoff_sink(&dir), None, "a directory");
        assert_eq!(handoff_sink(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
