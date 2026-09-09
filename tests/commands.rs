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

/// `pane-run` is what tmux starts in a pane scout opened. Without a
/// terminal it runs the command in the directory and exits with its
/// status instead of becoming a shell, which is how it can be tested.
#[test]
fn pane_run_runs_the_argv_in_the_directory_and_reports_its_status() {
    let dir = sandbox("pane-run");
    let marker = dir.join("tree/marker");
    let out = run(
        &dir,
        &[
            "pane-run",
            "--cwd",
            dir.join("tree").to_str().unwrap(),
            "--",
            "sh",
            "-c",
            "pwd > marker; exit 7",
        ],
    );
    assert_eq!(out.status.code(), Some(7), "the command's own exit status");
    let cwd = std::fs::read_to_string(&marker).expect("the command ran in --cwd");
    assert!(cwd.trim().ends_with("/tree"), "{cwd}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("sh: exit 7"), "{err}");
    // A command that cannot be started reports the shell convention:
    let out =
        run(&dir, &["pane-run", "--cwd", dir.to_str().unwrap(), "--", "no-such-command-scout"]);
    // 127 when the search says "not found", 126 when it says "not permitted"
    // (some PATH entries make the OS report a failed search as EACCES).
    assert!(matches!(out.status.code(), Some(126) | Some(127)), "{:?}", out.status.code());
    let _ = std::fs::remove_dir_all(&dir);
}

/// `scout index` with no path walks every root again the way it was
/// walked; a path that would contain a root is refused with the root
/// named; `--forget` drops one.
#[test]
fn index_without_a_path_rewalks_every_root_and_nesting_is_refused() {
    let dir = sandbox("roots");
    std::fs::create_dir_all(dir.join("tree/a/.hidden-dir")).unwrap();
    std::fs::create_dir_all(dir.join("tree/b")).unwrap();
    let a = dir.join("tree/a");
    let b = dir.join("tree/b");
    assert!(run(&dir, &["index", a.to_str().unwrap(), "--hidden"]).status.success());
    assert!(run(&dir, &["index", b.to_str().unwrap()]).status.success());

    // Both trees serve.
    let out = run(&dir, &["query", "hidden-dir"]);
    assert_eq!(out.status.code(), Some(0), "a's hidden row (walked --hidden) is served");
    assert_eq!(run(&dir, &["query", "b"]).status.code(), Some(0));

    // No path: two report lines, a still walked with --hidden.
    let out = run(&dir, &["index"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = stdout(&out);
    assert_eq!(text.lines().count(), 2, "{text}");
    assert!(text.lines().all(|l| l.starts_with("indexed ")), "{text}");
    assert_eq!(run(&dir, &["query", "hidden-dir"]).status.code(), Some(0));

    // Nesting refused, exit 1, the existing root named.
    // Refused with exit 1; the variant and its hint are asserted at the
    // library level (tests/index/walk.rs), not by their prose here.
    let out = run(&dir, &["index", dir.join("tree").to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(!out.stderr.is_empty(), "a refusal says something");

    // Forget b: its rows vanish from the candidate set.
    let out = run(&dir, &["index", "--forget", b.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).starts_with("forgot "));
    assert_eq!(run(&dir, &["query", "b"]).status.code(), Some(1));
    assert_eq!(run(&dir, &["index", "--forget", b.to_str().unwrap()]).status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// With nothing indexed, `scout index` alone says what to do.
#[test]
fn index_without_a_path_and_without_roots_says_to_index_something() {
    let dir = sandbox("no-roots");
    let out = run(&dir, &["index"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        !out.stderr.is_empty(),
        "the refusal says what to do (variant asserted in the library)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A path that does not exist, is not a directory, or is given relative
/// never becomes a root; a root whose directory has gone is reported
/// and leaves the previous index serving; walk flags need a path.
#[test]
fn index_refuses_bad_roots_and_an_absent_root_keeps_serving() {
    let dir = sandbox("bad-roots");
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("keep")).unwrap();
    std::fs::write(tree.join("file.txt"), b"x").unwrap();
    assert_eq!(run(&dir, &["index", dir.join("nope").to_str().unwrap()]).status.code(), Some(1));
    assert_eq!(
        run(&dir, &["index", tree.join("file.txt").to_str().unwrap()]).status.code(),
        Some(1)
    );
    let out = run(&dir, &["index", "--hidden"]);
    assert_eq!(out.status.code(), Some(2), "a flag without a path is a usage error");
    assert!(stdout(&run(&dir, &["doctor", "--format", "tsv"])).contains("roots\tnone"));

    assert!(run(&dir, &["index", tree.to_str().unwrap()]).status.success());
    assert_eq!(run(&dir, &["query", "keep"]).status.code(), Some(0));
    std::fs::rename(&tree, dir.join("tree-moved")).unwrap();
    let out = run(&dir, &["index"]);
    assert_eq!(out.status.code(), Some(1), "an absent root is not a completed walk");
    assert!(stdout(&out).contains("NOT PRESENT"), "{}", stdout(&out));
    std::fs::rename(dir.join("tree-moved"), &tree).unwrap();
    assert_eq!(
        run(&dir, &["query", "keep"]).status.code(),
        Some(0),
        "the previous index still serves"
    );

    // --no-hidden turns a remembered flag off; --forget accepts a
    // trailing slash on a tree that is gone.
    std::fs::create_dir_all(tree.join(".dot")).unwrap();
    assert!(run(&dir, &["index", tree.to_str().unwrap(), "--hidden"]).status.success());
    assert_eq!(run(&dir, &["query", ".dot"]).status.code(), Some(0));
    assert!(run(&dir, &["index", tree.to_str().unwrap(), "--no-hidden"]).status.success());
    assert_eq!(run(&dir, &["query", ".dot"]).status.code(), Some(1), "--no-hidden undoes --hidden");
    let doctor = stdout(&run(&dir, &["doctor", "--format", "tsv"]));
    assert!(doctor.contains("roots\t1:"), "{doctor}");
    assert!(doctor.contains("generation "), "the doctor line says how far each root got: {doctor}");
    std::fs::remove_dir_all(&tree).unwrap();
    let with_slash = format!("{}/", tree.display());
    assert!(run(&dir, &["index", "--forget", &with_slash]).status.success());
    assert_eq!(run(&dir, &["query", "keep"]).status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}
