//! `scout doctor` end-to-end (ADR-006).
//!
//! These run the real binary in a subprocess so each case gets its own
//! environment. `doctor` is defined by what it does to a machine, and
//! the two promises worth guarding — that it never writes, and that its
//! exit code separates "not set up yet" from "broken" — are only
//! observable from outside the process.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scout-doctor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).unwrap();
    std::fs::create_dir_all(dir.join("cfg")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::create_dir_all(dir.join("state")).unwrap();
    dir
}

fn doctor(dir: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scout"))
        .arg("doctor")
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("cfg"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env_remove("EDITOR")
        .env_remove("SCOUT_LOG")
        .output()
        .expect("run scout doctor")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A fresh machine has no index and no config. That is the correct
/// state on day one, not a fault, so `doctor` must report it without
/// failing — otherwise the exit code is useless to a script the moment
/// scout is installed.
#[test]
fn fresh_machine_warns_but_does_not_fail() {
    let dir = sandbox("fresh");
    let out = doctor(&dir);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(text.contains("no config found"), "{text}");
    assert!(text.contains("no index yet"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// ADR-006 §Decision: `doctor` never writes. The index open path
/// (`index::pragma::open`) creates directories, creates the file, runs
/// migrations and rebuilds a corrupt database — so a diagnostic that
/// reached for it would silently repair, or fabricate, the very state it
/// claims to be observing. This is the guard for that.
#[test]
fn doctor_creates_nothing() {
    let dir = sandbox("readonly");
    let db = dir.join("data/scout/index.db");
    let state = dir.join("state/scout");

    let out = doctor(&dir);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));

    assert!(!db.exists(), "doctor created the index database");
    assert!(!dir.join("data/scout").exists(), "doctor created the data directory");
    assert!(!state.exists(), "doctor created the state directory");
    assert!(!dir.join("cfg/scout").exists(), "doctor created the config directory");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A corrupt index is a genuine fault: it must FAIL, exit non-zero, and
/// — critically — still be corrupt afterwards, so the next tool to look
/// at it sees what doctor saw.
#[test]
fn corrupt_index_fails_and_is_left_intact() {
    let dir = sandbox("corrupt");
    let db_dir = dir.join("data/scout");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db = db_dir.join("index.db");
    let garbage: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&db, &garbage).unwrap();

    let out = doctor(&dir);
    let text = stdout(&out);

    assert_eq!(out.status.code(), Some(1), "corrupt index must fail: {text}");
    assert!(text.contains("FAIL"), "{text}");

    // Evidence preserved: byte-identical, and no rebuilt sibling.
    assert_eq!(std::fs::read(&db).unwrap(), garbage, "doctor rewrote the corrupt database");
    let siblings: Vec<_> = std::fs::read_dir(&db_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(siblings, vec!["index.db".to_string()], "doctor left extra files: {siblings:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The failure ADR-006 §Context case 1 exists for: discovery opens
/// O_NOFOLLOW, so a symlinked config is skipped in silence and the user
/// swears the file is right there. `doctor` must name it.
#[test]
fn symlinked_config_is_reported_not_silently_skipped() {
    let dir = sandbox("symlink");
    std::fs::create_dir_all(dir.join("cfg/scout")).unwrap();
    let real = dir.join("real-config.toml");
    std::fs::write(&real, "schema_version = 1\n").unwrap();
    std::os::unix::fs::symlink(&real, dir.join("cfg/scout/config.toml")).unwrap();

    let out = doctor(&dir);
    let text = stdout(&out);

    assert!(text.contains("symlink"), "{text}");
    assert!(text.contains("O_NOFOLLOW"), "{text}");
    // Skipped, so the run still falls through to compiled-in defaults.
    assert!(text.contains("no config found"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// ADR-006 §Decision: a closed allowlist, because this output exists to
/// be pasted. A secret-shaped variable in the environment must not
/// appear anywhere in the report.
#[test]
fn secrets_in_the_environment_are_never_printed() {
    let dir = sandbox("secrets");
    let out = Command::new(env!("CARGO_BIN_EXE_scout"))
        .arg("doctor")
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("cfg"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .env("GITHUB_TOKEN", "ghp_SUPERSECRETVALUE")
        .env("AWS_SECRET_ACCESS_KEY", "AKIA_SUPERSECRETVALUE")
        .output()
        .expect("run scout doctor");
    let text = stdout(&out);

    assert!(!text.contains("SUPERSECRETVALUE"), "doctor printed a secret value:\n{text}");
    assert!(!text.contains("GITHUB_TOKEN"), "doctor printed a secret name:\n{text}");
    assert!(!text.contains("AWS_SECRET"), "doctor printed a secret name:\n{text}");
    let _ = std::fs::remove_dir_all(&dir);
}
