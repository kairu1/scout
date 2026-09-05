//! tmux operations against a private tmux server. Ignored by default:
//! needs a `tmux` binary. Run with `cargo test --test tmux -- --ignored`.

use std::path::Path;
use std::process::Command;

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
    std::fs::create_dir_all(&dir).unwrap();
    let t = |args: &[&str]| Command::new("tmux").arg("-L").arg(&sock).args(args).output().unwrap();
    assert!(t(&["new-session", "-d", "-s", "probe", "-x", "120", "-y", "30"]).status.success());

    // Scout's own pane id, as detect() would read it.
    let own = String::from_utf8_lossy(&t(&["display-message", "-p", "#{pane_id}"]).stdout)
        .trim()
        .to_string();
    let mut tmux = Tmux {
        own_pane: own,
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
    let out = Command::new("tmux").arg("-L").arg(&sock).args(&argv).output().unwrap();
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
