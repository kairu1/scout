//! The CLI from outside: machine-readable output and exit codes.
//!
//! The tab hazard is the point: paths may contain tabs, so the format
//! puts the path last and a consumer splits on the first N tabs. A test
//! that merely asserted the output "contains the path" would pass while
//! the format was broken, so these tests parse the way a consumer would.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scout-fmt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for sub in ["home", "cfg", "data", "state", "tree"] {
        std::fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_scout"))
        .args(args)
        .env("HOME", dir.join("home"))
        .env("XDG_CONFIG_HOME", dir.join("cfg"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_STATE_HOME", dir.join("state"))
        .output()
        .expect("run scout")
}

fn index(dir: &Path) {
    let out = run(dir, &["index", dir.join("tree").to_str().unwrap()]);
    assert!(out.status.success(), "index failed");
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// An exit code that never varies carries no information.
#[test]
fn query_exit_code_distinguishes_match_from_no_match() {
    let dir = sandbox("exit");
    std::fs::create_dir_all(dir.join("tree/findme")).unwrap();
    index(&dir);

    let hit = run(&dir, &["query", "findme"]);
    assert_eq!(hit.status.code(), Some(0), "{}", stdout(&hit));
    assert!(stdout(&hit).contains("findme"));

    let miss = run(&dir, &["query", "zzzznosuchthing"]);
    assert_eq!(miss.status.code(), Some(1), "no match must exit 1");
    assert!(stdout(&miss).trim().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The hazard the format exists to neutralise: a tab inside a path. Field
/// order — path last — is what makes this survivable.
#[test]
fn a_tab_in_a_path_cannot_shift_a_tsv_field() {
    let dir = sandbox("tabpath");
    let weird = dir.join("tree").join("we\tird");
    std::fs::create_dir_all(&weird).unwrap();
    index(&dir);

    let out = run(&dir, &["query", "ird", "--format", "tsv"]);
    let text = stdout(&out);
    let line = text.lines().find(|l| l.contains("ird")).unwrap_or_else(|| panic!("no row: {text}"));

    // Exactly how a consumer reads it: split on the first two tabs, take
    // the rest whole.
    let mut parts = line.splitn(3, '\t');
    let rank: f64 = parts.next().unwrap().parse().expect("rank parses");
    let visits: i64 = parts.next().unwrap().parse().expect("visits parses");
    let path = parts.next().unwrap();

    assert!(rank.is_finite());
    assert_eq!(visits, 0);
    assert_eq!(path, weird.to_str().unwrap(), "the path must survive whole, tab and all");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn print0_separates_with_nul() {
    let dir = sandbox("print0");
    std::fs::create_dir_all(dir.join("tree/alpha")).unwrap();
    std::fs::create_dir_all(dir.join("tree/alphabet")).unwrap();
    index(&dir);

    let out = run(&dir, &["query", "alpha", "--print0"]);
    let raw = out.stdout;
    assert!(raw.contains(&0u8), "records must be NUL-separated");
    assert!(!raw.contains(&b'\n'), "no newlines in --print0 output");
    let count = raw.iter().filter(|b| **b == 0).count();
    assert!(count >= 2, "expected both matches, got {count}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn doctor_tsv_is_parseable_and_detail_is_last() {
    let dir = sandbox("doctortsv");
    let out = run(&dir, &["doctor", "--format", "tsv"]);
    let text = stdout(&out);
    assert!(!text.is_empty());
    for line in text.lines() {
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        assert_eq!(parts.len(), 4, "every row has four fields: {line}");
        assert!(matches!(parts[0], "ok" | "warn" | "FAIL"), "level: {line}");
        // Detail is last, so it may contain anything except a tab —
        // which the renderer folds to a space.
        assert!(!parts[3].contains('\t'));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_format_is_refused_not_ignored() {
    let dir = sandbox("badfmt");
    for args in [vec!["query", "x", "--format", "yaml"], vec!["doctor", "--format", "yaml"]] {
        let out = run(&dir, &args);
        assert_eq!(out.status.code(), Some(2), "{args:?} must be refused");
        assert!(String::from_utf8_lossy(&out.stderr).contains("unknown --format"));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Piped output is data. The index refuses
/// NUL and newline in a path but permits ESC, so a directory name can
/// carry a terminal escape — and mangling it in a pipe would hand a
/// consumer a path that does not exist on disk.
///
/// Every format must therefore be byte-exact when piped. The
/// human-facing counterpart (stripping when stdout is a terminal) needs
/// a PTY and is covered by the harness, not here.
#[test]
fn piped_output_is_byte_exact_in_every_format() {
    let dir = sandbox("escape-pipe");
    let weird = dir.join("tree").join("evil\u{1b}]0;HIJACK\u{7}dir");
    std::fs::create_dir_all(&weird).unwrap();
    index(&dir);
    let expected = weird.to_str().unwrap();

    // Default paths format.
    let out = run(&dir, &["query", "evil"]);
    assert!(stdout(&out).contains(expected), "paths format mangled the path");

    // TSV: the path is the last field, so take everything after two tabs.
    let out = run(&dir, &["query", "evil", "--format", "tsv"]);
    let text = stdout(&out);
    let line = text.lines().find(|l| l.contains("evil")).expect("a row");
    let path = line.splitn(3, '\t').nth(2).expect("third field");
    assert_eq!(path, expected, "tsv mangled the path");

    // NUL-separated.
    let out = run(&dir, &["query", "evil", "--print0"]);
    let raw = String::from_utf8_lossy(&out.stdout);
    let record = raw.split('\0').next().expect("a record");
    assert_eq!(record, expected, "print0 mangled the path");

    let _ = std::fs::remove_dir_all(&dir);
}
