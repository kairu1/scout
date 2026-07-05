//! Engineers — executor semantics: chain policy, env landing, visit
//! credit + rate limit (ADR-001 §Visit credit, ADR-004 §5).

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use rusqlite::Connection;
use scout::actions::{execute, ActionCtx};
use scout::config::template::{ExpandCtx, ExpandError, Template};
use scout::config::{Action, OnFailure, Step};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-exec-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn t(raw: &str) -> Template {
    Template::parse(raw).unwrap()
}

fn spawn_step(argv: &[&str]) -> Step {
    Step::Spawn { argv: argv.iter().map(|a| t(a)).collect(), wait: true, cwd: None }
}

fn action(name: &str, on_failure: OnFailure, steps: Vec<Step>) -> Action {
    Action {
        name: name.into(),
        description: String::new(),
        keybinding: None,
        on_failure,
        unsafe_shell_template: false,
        steps,
        from_user_config: true,
    }
}

fn ctx(path: PathBuf) -> ActionCtx {
    ActionCtx { path, query: String::new(), home: std::env::var("HOME").unwrap() }
}

fn seeded_db(dir: &std::path::Path) -> (Connection, i64) {
    // The real open path registers exp() when the bundled SQLite lacks
    // math built-ins; visit credit depends on it.
    let conn = scout::index::pragma::open(&dir.join("index.db")).unwrap();
    conn.execute(
        "INSERT INTO paths (path, scan_generation) VALUES (:p, 1)",
        rusqlite::named_params! { ":p": dir.display().to_string() },
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    (conn, id)
}

fn visits(conn: &Connection, id: i64) -> i64 {
    conn.query_row(
        "SELECT visits_total FROM paths WHERE rowid = :id",
        rusqlite::named_params! { ":id": id },
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn abort_on_first_failure_no_credit() {
    let dir = temp_dir("abort");
    let (conn, id) = seeded_db(&dir);

    let a = action("fails-first", OnFailure::Abort, vec![spawn_step(&["false"]), spawn_step(&["true"])]);
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(!outcome.any_success);
    assert!(!outcome.credited);
    assert_eq!(outcome.steps_run, 1, "abort must stop the chain");
    assert_eq!(outcome.exit_code, 1, "first failing step's exit code");
    assert_eq!(visits(&conn, id), 0, "credit suppressed when the first step fails");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn first_success_credits_even_when_later_step_aborts() {
    let dir = temp_dir("credit");
    let (conn, id) = seeded_db(&dir);

    let a = action("succeeds-then-fails", OnFailure::Abort, vec![spawn_step(&["true"]), spawn_step(&["false"])]);
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(outcome.any_success);
    assert!(outcome.credited, "credit granted on first success is not retracted by a later abort");
    assert_eq!(outcome.exit_code, 0, "any success → exit 0");
    assert_eq!(visits(&conn, id), 1);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn rate_limit_one_credit_per_window() {
    let dir = temp_dir("ratelimit");
    let (conn, id) = seeded_db(&dir);

    let a = action("ok", OnFailure::Abort, vec![spawn_step(&["true"])]);
    let first = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    let second = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(first.credited);
    assert!(!second.credited, "double-Enter within 10 s must not compound");
    assert_eq!(visits(&conn, id), 1);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn continue_runs_all_steps_exit_2_when_none_succeed() {
    let dir = temp_dir("continue");
    let (conn, id) = seeded_db(&dir);

    let a = action(
        "all-fail",
        OnFailure::Continue,
        vec![spawn_step(&["false"]), spawn_step(&["false"])],
    );
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(!outcome.any_success);
    assert_eq!(outcome.steps_run, 2, "continue must run the whole chain");
    assert_eq!(outcome.exit_code, 2, "SCOUT-defined 2 when every step failed under continue");
    assert_eq!(visits(&conn, id), 0);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn failed_env_step_lands_nothing_and_later_reference_aborts() {
    let dir = temp_dir("env");
    let (conn, id) = seeded_db(&dir);

    // Step 1 sets GOOD and also references an undefined env var → the
    // step fails and NONE of its bindings land (all-or-nothing).
    // Step 2 references GOOD → undefined_env, not empty expansion.
    let a = action(
        "env-chain",
        OnFailure::Continue,
        vec![
            Step::Env {
                set: vec![
                    ("GOOD".into(), t("value")),
                    ("BAD".into(), t("{env.SCOUT_TEST_UNDEFINED_VAR}")),
                ],
            },
            Step::Spawn { argv: vec![t("printenv"), t("{env.GOOD}")], wait: true, cwd: None },
        ],
    );
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(!outcome.any_success, "no step may succeed");
    assert_eq!(outcome.steps_run, 2);
    assert_eq!(visits(&conn, id), 0);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn env_overlay_reaches_spawned_children() {
    let dir = temp_dir("overlay");
    let marker = dir.join("marker");

    // sh -c without placeholders needs no attestation; it reads the
    // env-step binding and writes it to a file we can assert on.
    let script = format!("printf '%s' \"$SCOUT_TEST_VALUE\" > {}", marker.display());
    let a = action(
        "env-to-child",
        OnFailure::Abort,
        vec![
            Step::Env { set: vec![("SCOUT_TEST_VALUE".into(), t("from-scout"))] },
            Step::Spawn { argv: vec![t("sh"), t("-c"), t(&script)], wait: true, cwd: None },
        ],
    );
    let outcome = execute(&a, &ctx(dir.clone()), None);
    assert!(outcome.any_success);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "from-scout");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn print_seam_quotes_and_refuses_hazardous_paths() {
    let env = HashMap::new();
    let path = PathBuf::from("/tmp/it's a dir");
    let ctx = ExpandCtx { path: &path, query: "q", home: "/home/u", env: &env };

    let quoted = t("cd {path}").expand(&ctx, true).unwrap();
    assert_eq!(quoted, "cd '/tmp/it'\\''s a dir'");

    // Argv seam does NOT quote (execvp gets the literal bytes).
    let unquoted = t("{path}").expand(&ctx, false).unwrap();
    assert_eq!(unquoted, "/tmp/it's a dir");

    let evil = PathBuf::from("/tmp/evil\nname");
    let ctx = ExpandCtx { path: &evil, query: "", home: "/home/u", env: &env };
    assert_eq!(t("{path}").expand(&ctx, true).unwrap_err(), ExpandError::HazardousPath);
}

#[test]
fn undefined_query_is_valid_empty_but_env_is_not() {
    let env = HashMap::new();
    let path = PathBuf::from("/tmp");
    let ctx = ExpandCtx { path: &path, query: "", home: "/h", env: &env };
    assert_eq!(t("q={query}").expand(&ctx, false).unwrap(), "q=");
    assert!(matches!(
        t("{env.NOPE}").expand(&ctx, false),
        Err(ExpandError::UndefinedEnv(_))
    ));
}

#[test]
fn repo_root_resolves_through_git_file_or_dir() {
    let dir = temp_dir("reporoot");
    let repo = dir.join("repo");
    let nested = repo.join("a/b");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir(repo.join(".git")).unwrap();

    let env = HashMap::new();
    let ctx = ExpandCtx { path: &nested, query: "", home: "/h", env: &env };
    assert_eq!(
        t("{repo_root}").expand(&ctx, false).unwrap(),
        repo.display().to_string()
    );

    // Worktree-style .git FILE also counts.
    let wt = dir.join("worktree");
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), "gitdir: elsewhere\n").unwrap();
    let ctx = ExpandCtx { path: &wt, query: "", home: "/h", env: &env };
    assert_eq!(t("{repo_root}").expand(&ctx, false).unwrap(), wt.display().to_string());

    // No .git anywhere under temp root → undefined, aborts.
    let bare = temp_dir("bare");
    let ctx = ExpandCtx { path: &bare, query: "", home: "/h", env: &env };
    assert!(matches!(
        t("{repo_root}").expand(&ctx, false),
        Err(ExpandError::UndefinedPlaceholder(_))
    ));

    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&bare).unwrap();
}
