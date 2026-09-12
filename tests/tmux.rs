//! tmux against a private server. Ignored by default: needs a `tmux`
//! binary (and `script` for the pty test). Run with
//! `cargo test --test tmux -- --ignored`.
//!
//! The private world: `TMUX_TMPDIR` points every tmux socket, including
//! the `-L scout` server scout starts, into the sandbox, so a real scout
//! session on this machine is never touched. `TMUX` is removed from every
//! environment here so scout sees "outside tmux" even when the test
//! itself runs inside one.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use scout::config::keys::Operation;
use scout::tmux::Tmux;

fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().map(|o| o.status.success()).unwrap_or(false)
}

/// Opens a split at a directory whose name ends in `;`, runs a command
/// through `pane-run` in it, and closes the pane scout opened.
#[test]
#[ignore]
fn split_at_a_semicolon_directory_runs_a_command_and_closes() {
    if !tmux_available() {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let sock = format!("scout-test-{}", std::process::id());
    let dir = std::env::temp_dir().join(format!("scout-tmux-{};", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sockets")).unwrap();
    // The socket lives in the sandbox, not the shared tmux directory, so
    // a run leaves nothing behind there.
    let sockets = dir.join("sockets");
    let t = |args: &[&str]| {
        Command::new("tmux")
            .env("TMUX_TMPDIR", &sockets)
            .arg("-L")
            .arg(&sock)
            .args(args)
            .output()
            .unwrap()
    };
    assert!(t(&["new-session", "-d", "-s", "probe", "-x", "120", "-y", "30"]).status.success());

    // Scout's own pane id, as detect() would read it.
    let own = String::from_utf8_lossy(&t(&["display-message", "-p", "#{pane_id}"]).stdout)
        .trim()
        .to_string();
    let mut tmux = Tmux {
        own_pane: own,
        session: "probe".into(),
        opened: Vec::new(),
        scout_exe: std::path::PathBuf::from(env!("CARGO_BIN_EXE_scout")),
    };
    // Point the argv at the private server: prepend -L.
    let argv = tmux
        .argv(
            Operation::SplitRight,
            Some(&dir),
            Some(&["sh".to_string(), "-c".to_string(), "pwd > out.txt".to_string()]),
        )
        .unwrap();
    let out = Command::new("tmux")
        .env("TMUX_TMPDIR", &sockets)
        .arg("-L")
        .arg(&sock)
        .args(&argv)
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let pane = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(pane.starts_with('%'), "{pane}");
    tmux.opened.push(pane.clone());

    // The command ran in the semicolon directory (the escape held).
    std::thread::sleep(std::time::Duration::from_millis(800));
    let written = std::fs::read_to_string(dir.join("out.txt")).expect("pane-run ran the command");
    assert_eq!(Path::new(written.trim()), dir.as_path());

    let close = tmux.argv(Operation::ClosePane, None, None).unwrap();
    assert_eq!(close, ["kill-pane", "-t", pane.as_str()]);
    assert!(Command::new("tmux")
        .env("TMUX_TMPDIR", &sockets)
        .arg("-L")
        .arg(&sock)
        .args(&close)
        .output()
        .unwrap()
        .status
        .success());

    let _ = t(&["kill-server"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The sandbox a launch runs in: its own HOME, config, index, temp dir
/// and tmux socket directory.
struct World {
    dir: PathBuf,
}

impl Drop for World {
    /// A failed assertion must not leave a private tmux server behind.
    fn drop(&mut self) {
        let _ = self.tmux(&["kill-server"]);
    }
}

impl World {
    fn new(tag: &str) -> World {
        let dir = std::env::temp_dir().join(format!("scout-launch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in [
            "home",
            "cfg/scout",
            "data",
            "state/scout",
            "tmp",
            "sockets",
            "tree/alpha",
            "tree/beta",
            // A directory whose name is tmux format syntax and a command
            // separator: opening a pane at it must open a pane at it.
            "tree/hostile #(touch PWNED);",
        ] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        World { dir }
    }

    /// The environment every process in the world gets: sandboxed XDG
    /// paths, a private tmux socket directory, the test binary first on
    /// PATH, bash as the shell, and no `TMUX`.
    fn env(&self, command: &mut Command) {
        let bin_dir = Path::new(env!("CARGO_BIN_EXE_scout")).parent().unwrap().to_path_buf();
        let path = format!("{}:{}", bin_dir.display(), std::env::var("PATH").unwrap_or_default());
        command
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .env("HOME", self.dir.join("home"))
            .env("XDG_CONFIG_HOME", self.dir.join("cfg"))
            .env("XDG_DATA_HOME", self.dir.join("data"))
            .env("XDG_STATE_HOME", self.dir.join("state"))
            .env("TMPDIR", self.dir.join("tmp"))
            .env("TMUX_TMPDIR", self.dir.join("sockets"))
            .env("PATH", path)
            .env("SHELL", "/bin/bash")
            .env("TERM", "xterm-256color");
    }

    fn scout(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_scout"));
        command.args(args);
        self.env(&mut command);
        command.output().expect("run scout")
    }

    /// A tmux command against scout's private server in this world.
    fn tmux(&self, args: &[&str]) -> Output {
        let mut command = Command::new("tmux");
        command.args(["-L", scout::tmux::PRIVATE_SERVER]).args(args);
        self.env(&mut command);
        command.output().expect("run tmux")
    }

    fn tmux_ok(&self, args: &[&str]) -> String {
        let out = self.tmux(args);
        assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// Write a config shaped like the reference one's navigation: Enter
    /// prints a `cd` in a one-shot run and opens a shell pane in a
    /// session, and `alt-c` is the session's exit action. Trust it the
    /// way a user would have, so the launch sees no prompt.
    fn trusted_config(&self) {
        let config_path = self.dir.join("cfg/scout/config.toml");
        std::fs::write(
            &config_path,
            "schema_version = 2\n\n\
             [[action]]\nname = \"go\"\nkeybinding = \"enter\"\nwhen = { mode = \"one-shot\" }\n\
             steps = [ { kind = \"print\", format = \"cd {path}\" } ]\n\
             [[action]]\nname = \"go\"\nkeybinding = \"enter\"\nwhen = { mode = \"session\" }\n\
             steps = [ { kind = \"spawn\", argv = [], cwd = \"{dir}\", pane = \"split-right\" } ]\n\
             [[action]]\nname = \"leave\"\nkeybinding = \"alt-c\"\nwhen = { mode = \"session\" }\n\
             steps = [ { kind = \"print\", format = \"cd {path}\" } ]\n",
        )
        .unwrap();
        let store = self.dir.join("state/scout/trusted-config.sha256");
        match scout::config::load_file(&config_path, store.clone(), true) {
            Err(scout::Error::TrustRequiresTty { hash, .. }) => {
                std::fs::write(&store, format!("v2 {hash} {}\n", config_path.display())).unwrap();
            }
            Ok(_) => {}
            Err(err) => panic!("config must load: {err}"),
        }
        scout::config::load_file(&config_path, store, true).expect("trusted now");
    }

    /// Run the shell wrapper's `scout -s` in a real pty. Returns the
    /// child (its stdin is held open so the pty never sees EOF) and a
    /// thread collecting everything the pty printed.
    fn wrapper_session(&self) -> PtyRun {
        let wrapper = Path::new(env!("CARGO_MANIFEST_DIR")).join("shell/scout.bash");
        let script = format!(
            "stty rows 30 cols 100; source '{}'; cd '{}'; scout -s; echo RC=$?; pwd",
            wrapper.display(),
            self.dir.display()
        );
        let mut command = Command::new("script");
        command.args(["-q", "-e", "-c", &script, "/dev/null"]);
        self.env(&mut command);
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = command.spawn().expect("script(1) provides the pty");
        let stdin = child.stdin.take();
        let mut stdout = child.stdout.take().unwrap();
        let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = buf.clone();
        let reader = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(n) = stdout.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        });
        PtyRun { child, _stdin: stdin, buf, reader: Some(reader) }
    }

    /// Everything worth knowing when a wait times out.
    fn diagnose(&self, run: &PtyRun) -> String {
        format!(
            "sessions: {}\npanes: {}\ntranscript so far:\n{}",
            String::from_utf8_lossy(&self.tmux(&["ls"]).stdout),
            String::from_utf8_lossy(
                &self
                    .tmux(&[
                        "list-panes",
                        "-s",
                        "-t",
                        "=scout",
                        "-F",
                        "#{pane_id} #{@scout_role} #{pane_dead} #{pane_current_command}"
                    ])
                    .stdout
            ),
            run.snapshot()
        )
    }

    fn picker_pane(&self) -> Option<String> {
        let listing = self.tmux(&[
            "list-panes",
            "-s",
            "-t",
            "=scout",
            "-F",
            "#{pane_id} #{@scout_role} #{pane_dead}",
        ]);
        if !listing.status.success() {
            return None;
        }
        String::from_utf8_lossy(&listing.stdout).lines().find_map(|line| {
            let f: Vec<&str> = line.split(' ').collect();
            (f.len() == 3 && f[1] == "picker" && f[2] == "0").then(|| f[0].to_string())
        })
    }

    fn pane_count(&self) -> usize {
        self.tmux_ok(&["list-panes", "-s", "-t", "=scout", "-F", "#{pane_id}"]).lines().count()
    }

    fn screen(&self, pane: &str) -> String {
        self.tmux_ok(&["capture-pane", "-p", "-t", pane])
    }

    fn send(&self, pane: &str, keys: &[&str]) {
        let mut args = vec!["send-keys", "-t", pane];
        args.extend(keys);
        self.tmux_ok(&args);
    }

    fn send_text(&self, pane: &str, text: &str) {
        self.tmux_ok(&["send-keys", "-t", pane, "-l", text]);
    }
}

struct PtyRun {
    child: std::process::Child,
    _stdin: Option<std::process::ChildStdin>,
    buf: Arc<Mutex<Vec<u8>>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl PtyRun {
    fn snapshot(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().unwrap()).into_owned()
    }

    /// Wait for the wrapper to return and give back what the pty printed.
    fn finish(mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if self.child.try_wait().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "the wrapper never returned:\n{}", self.snapshot());
            std::thread::sleep(Duration::from_millis(100));
        }
        self.reader.take().unwrap().join().unwrap();
        self.snapshot()
    }
}

fn wait_for<T>(
    what: &str,
    context: impl Fn() -> String,
    mut probe: impl FnMut() -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(value) = probe() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}\n{}", context());
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// The whole story, in a private world: `scout -s` from a plain shell
/// lands in a picker inside a tmux session scout started; a pane key
/// opens a split; Enter on a project returns the shell to that directory
/// after scout detaches; the session and the split survive; the next
/// `scout -s` attaches and opens a fresh picker beside them; the help
/// overlay names an alt-shift arrow as the key it received; Esc returns
/// the shell with nothing evaluated.
#[test]
#[ignore]
fn a_session_outside_tmux_lands_in_a_picker_with_panes_and_returns_the_cd() {
    if !tmux_available() {
        eprintln!("tmux not installed; skipping");
        return;
    }
    let world = World::new("story");
    world.trusted_config();
    let out = world.scout(&["index", world.dir.join("tree").to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = world.tmux(&["kill-server"]);

    // 1. Launch: the session appears with a picker pane.
    let run = world.wrapper_session();
    let picker = wait_for("the picker pane", || world.diagnose(&run), || world.picker_pane());
    wait_for(
        "the picker to draw",
        || world.diagnose(&run),
        || world.screen(&picker).contains("scout: session").then_some(()),
    );
    assert_eq!(world.pane_count(), 1);

    // 2. The split-right default opens a pane at the selection, and a
    //    hostile directory name opens as a directory, nothing more.
    world.send_text(&picker, "hostile");
    wait_for(
        "the hostile row to rank first",
        || world.diagnose(&run),
        || world.screen(&picker).contains("hostile").then_some(()),
    );
    world.send(&picker, &["M-Right"]);
    wait_for("the split", || world.diagnose(&run), || (world.pane_count() == 2).then_some(()));
    let paths = world.tmux_ok(&["list-panes", "-s", "-t", "=scout", "-F", "#{pane_current_path}"]);
    assert!(paths.contains("hostile #(touch PWNED);"), "the pane opened in the directory: {paths}");
    assert!(!world.dir.join("PWNED").exists(), "tmux ran the directory name as a command");
    for _ in 0..7 {
        world.send(&picker, &["BSpace"]);
    }

    // 3. Enter on alpha in a session is the session `go`: a shell pane at
    //    alpha, no command, and the picker stays. The one-shot `go` on
    //    the same key never reaches the picker.
    world.send_text(&picker, "alpha");
    world.send(&picker, &["Enter"]);
    wait_for("the go pane", || world.diagnose(&run), || (world.pane_count() == 3).then_some(()));
    let paths = world.tmux_ok(&["list-panes", "-s", "-t", "=scout", "-F", "#{pane_current_path}"]);
    assert!(paths.contains("tree/alpha"), "the session go opened a pane at alpha: {paths}");
    let commands =
        world.tmux_ok(&["list-panes", "-s", "-t", "=scout", "-F", "#{pane_current_command}"]);
    assert!(
        !commands.contains("pane-run"),
        "an empty argv is a shell, not pane-run running nothing: {commands}"
    );
    assert!(world.picker_pane().is_some(), "the picker survives a session action");
    // Back from the new pane and kill it, so the split from step 2 is the
    // one pane left over after the leave.
    world.send(&picker, &["M-h"]);
    world.send(&picker, &["M-x"]);
    wait_for(
        "the go pane closed",
        || world.diagnose(&run),
        || (world.pane_count() == 2).then_some(()),
    );

    // 3b. `leave` (alt-c) is the exit action: the cd reaches the wrapper
    //     after the detach.
    world.send(&picker, &["M-c"]);
    let transcript = run.finish();
    assert!(transcript.contains("RC=0"), "{transcript}");
    let alpha = world.dir.join("tree/alpha");
    let last = transcript.lines().rev().find(|l| l.trim().ends_with("alpha")).unwrap_or("");
    assert!(last.trim().ends_with(alpha.to_str().unwrap()), "pwd after cd:\n{transcript}");

    // 4. The session and the split are still there; the picker is gone.
    assert!(world.tmux(&["has-session", "-t", "=scout"]).status.success());
    assert_eq!(world.pane_count(), 1, "the split's shell keeps the session alive");
    assert!(world.picker_pane().is_none());

    // 5. Re-attach: a fresh picker beside the old split; the key test
    //    names an alt-shift arrow.
    let run = world.wrapper_session();
    let picker = wait_for("a fresh picker pane", || world.diagnose(&run), || world.picker_pane());
    wait_for(
        "the picker to draw",
        || world.diagnose(&run),
        || world.screen(&picker).contains("scout: session").then_some(()),
    );
    assert_eq!(world.pane_count(), 2);
    // While the picker runs on scout's own server, the focus keys and
    // focus-picker are tmux bindings too, aimed at this picker's pane.
    let root_keys = world.tmux_ok(&["list-keys", "-T", "root"]);
    assert!(root_keys.contains("M-S-Left") && root_keys.contains("select-pane -L"), "{root_keys}");
    assert!(
        root_keys.contains("M-h") && root_keys.contains(&format!("select-pane -t \"{picker}\"")),
        "{root_keys}"
    );
    assert!(root_keys.contains("M-q") && root_keys.contains("kill-pane"), "{root_keys}");
    world.send(&picker, &["?"]);
    wait_for(
        "the help overlay",
        || world.diagnose(&run),
        || world.screen(&picker).contains("last key").then_some(()),
    );
    world.send(&picker, &["M-S-Left"]);
    wait_for(
        "alt-shift-left to be named",
        || world.diagnose(&run),
        || world.screen(&picker).contains("alt-shift-left").then_some(()),
    );

    // 6. Esc closes the help, Esc leaves: nothing evaluated.
    world.send(&picker, &["Escape"]);
    std::thread::sleep(Duration::from_millis(300));
    world.send(&picker, &["Escape"]);
    let transcript = run.finish();
    assert!(transcript.contains("RC=0"), "{transcript}");
    let last =
        transcript.lines().rev().find(|l| l.contains(world.dir.to_str().unwrap())).unwrap_or("");
    assert!(!last.trim().ends_with("alpha"), "Esc must not cd anywhere:\n{transcript}");
    assert!(world.tmux(&["has-session", "-t", "=scout"]).status.success(), "session survives Esc");
    let root_keys = world.tmux_ok(&["list-keys", "-T", "root"]);
    assert!(!root_keys.contains("M-h"), "bindings are removed when the picker leaves: {root_keys}");

    // 7. Nothing was killed by scout; the test kills its own server.
    world.tmux_ok(&["kill-server"]);
    let _ = std::fs::remove_dir_all(&world.dir);
}
