//! Executor semantics: chain policy, env landing, visit credit and its
//! rate limit, and the failure reason carried out to the caller.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use scout::actions::{execute, ActionCtx, FailureKind, OnFailure, Step};

use crate::{action, t, temp_dir};

fn spawn_step(argv: &[&str]) -> Step {
    Step::Spawn {
        argv: argv.iter().map(|a| t(a)).collect(),
        wait: true,
        cwd: None,
        pause: true,
        pane: None,
    }
}

fn ctx(path: PathBuf) -> ActionCtx<'static> {
    ActionCtx {
        path,
        query: String::new(),
        home: std::env::var("HOME").unwrap(),
        print_to: None,
        pane_runner: None,
    }
}

fn seeded_db(dir: &Path) -> (Connection, i64) {
    // The real open path registers exp() when the bundled SQLite lacks
    // math built-ins; visit credit depends on it.
    let conn = scout::index::open(&dir.join("index.db")).unwrap();
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
fn abort_stops_at_the_first_failure_and_credits_nothing() {
    let dir = temp_dir("abort");
    let (conn, id) = seeded_db(&dir);

    let a = action(
        "fails-first",
        OnFailure::Abort,
        vec![spawn_step(&["false"]), spawn_step(&["true"])],
    );
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(!outcome.any_success);
    assert!(!outcome.credited);
    assert_eq!(outcome.steps_run, 1, "abort must stop the chain");
    assert_eq!(outcome.exit_code, 1, "first failing step's exit code");
    assert_eq!(visits(&conn, id), 0, "credit suppressed when the first step fails");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_first_success_credits_even_when_a_later_step_aborts() {
    let dir = temp_dir("credit");
    let (conn, id) = seeded_db(&dir);

    let a = action(
        "succeeds-then-fails",
        OnFailure::Abort,
        vec![spawn_step(&["true"]), spawn_step(&["false"])],
    );
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(outcome.any_success);
    assert!(outcome.credited, "credit granted on first success is not retracted by a later abort");
    assert_eq!(outcome.exit_code, 0, "any success means exit 0");
    assert_eq!(visits(&conn, id), 1);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn one_credit_per_path_per_ten_second_window() {
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
fn continue_runs_every_step_and_exits_2_when_none_succeed() {
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
    assert_eq!(outcome.exit_code, 2, "2 when every step failed under continue");
    assert_eq!(visits(&conn, id), 0);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_failed_env_step_lands_nothing_and_a_later_reference_is_undefined() {
    let dir = temp_dir("env");
    let (conn, id) = seeded_db(&dir);

    // Step 1 sets GOOD and also references an undefined env var, so the
    // step fails and none of its bindings land (all-or-nothing). Step 2
    // references GOOD: undefined, not an empty expansion.
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
            Step::Spawn {
                argv: vec![t("printenv"), t("{env.GOOD}")],
                wait: true,
                cwd: None,
                pause: true,
                pane: None,
            },
        ],
    );
    let outcome = execute(&a, &ctx(dir.clone()), Some((&conn, id)));
    assert!(!outcome.any_success, "no step may succeed");
    assert_eq!(outcome.steps_run, 2);
    assert_eq!(visits(&conn, id), 0);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_env_step_reaches_spawned_children() {
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
            Step::Spawn {
                argv: vec![t("sh"), t("-c"), t(&script)],
                wait: true,
                cwd: None,
                pause: true,
                pane: None,
            },
        ],
    );
    let outcome = execute(&a, &ctx(dir.clone()), None);
    assert!(outcome.any_success);
    assert_eq!(fs::read_to_string(&marker).unwrap(), "from-scout");

    fs::remove_dir_all(&dir).unwrap();
}

/// The reason a step failed used to exist only as a log line, written in
/// picker mode, so a user whose action silently did nothing had no way
/// to reach it. The outcome carries it out as a typed kind.
#[test]
fn an_empty_argv_pane_step_hands_the_runner_no_command_at_dir() {
    let dir = temp_dir("emptyargv");
    let project = dir.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("main.rs"), "").unwrap();

    let seen: std::cell::RefCell<Vec<(PathBuf, Vec<String>)>> = std::cell::RefCell::new(Vec::new());
    let runner = |_op: scout::actions::PaneOp,
                  cwd: &Path,
                  argv: &[String],
                  _env: &[(String, String)]|
     -> Result<(), String> {
        seen.borrow_mut().push((cwd.to_path_buf(), argv.to_vec()));
        Ok(())
    };
    let a = action(
        "go",
        OnFailure::Abort,
        vec![Step::Spawn {
            argv: vec![],
            wait: false,
            cwd: Some(t("{dir}")),
            pause: true,
            pane: Some(scout::actions::PaneOp::SplitRight),
        }],
    );
    // A file selection: the pane opens at its directory.
    let ctx = ActionCtx {
        path: project.join("main.rs"),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: None,
        pane_runner: Some(&runner),
    };
    let outcome = execute(&a, &ctx, None);
    assert!(outcome.any_success, "{:?}", outcome.failure);
    assert_eq!(seen.borrow().as_slice(), [(project.clone(), Vec::<String>::new())]);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_outcome_carries_the_failure_kind() {
    let dir = temp_dir("failreason");
    let target = dir.join("not-a-repo");
    fs::create_dir_all(&target).unwrap();

    let a = action("status", OnFailure::Abort, vec![Step::Print { format: t("cd {repo_root}") }]);
    let ctx = ActionCtx {
        path: target.clone(),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: None,
        pane_runner: None,
    };

    let outcome = execute(&a, &ctx, None);
    assert!(!outcome.any_success);
    let (step, kind) = outcome.failure.expect("a failed action must report why");
    assert_eq!(step, 0, "first step failed");
    assert_eq!(kind, FailureKind::UndefinedPlaceholder("repo_root".into()));

    fs::remove_dir_all(&dir).unwrap();
}

/// The substitution the threat model refuses: a reference resolving to an
/// inherited variable of the same name. PATH is always set in the parent
/// environment, so this needs no mutation of it (tests that call
/// `set_var` race every sibling reading `vars()` on another thread).
#[test]
fn an_env_placeholder_never_falls_back_to_the_inherited_environment() {
    let dir = temp_dir("env-nofallback");
    let a =
        action("leak", OnFailure::Abort, vec![Step::Print { format: t("printf '%s' {env.PATH}") }]);
    let ctx = ActionCtx {
        path: dir.clone(),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: None,
        pane_runner: None,
    };

    let outcome = execute(&a, &ctx, None);
    assert!(!outcome.any_success, "the reference must not resolve");
    let (_, kind) = outcome.failure.expect("a reason");
    assert!(matches!(kind, FailureKind::UndefinedEnv(ref n) if n == "PATH"), "{kind}");

    fs::remove_dir_all(&dir).unwrap();
}

/// The legitimate case still works: a binding set by an earlier step in
/// the same action is visible to a later one.
#[test]
fn an_env_placeholder_resolves_a_binding_set_by_an_earlier_step() {
    let dir = temp_dir("env-binding");
    let a = action(
        "chain",
        OnFailure::Abort,
        vec![
            Step::Env { set: vec![("SCOUT_STEP_SET".into(), t("{name}"))] },
            Step::Print { format: t("printf '%s' {env.SCOUT_STEP_SET}") },
        ],
    );
    let ctx = ActionCtx {
        path: dir.clone(),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: None,
        pane_runner: None,
    };

    let outcome = execute(&a, &ctx, None);
    assert!(outcome.any_success, "failure: {:?}", outcome.failure);
    fs::remove_dir_all(&dir).unwrap();
}

/// A spawned child must still inherit a usable environment: the split
/// restricts what `{env.NAME}` sees, not what a process receives.
#[test]
fn a_spawned_child_still_inherits_the_process_environment() {
    let dir = temp_dir("env-child");
    let marker = dir.join("saw-path");
    let a = action(
        "spawn",
        OnFailure::Abort,
        vec![Step::Spawn {
            argv: vec![
                t("/bin/sh"),
                t("-c"),
                t(&format!("test -n \"$PATH\" && touch {}", marker.display())),
            ],
            wait: true,
            cwd: None,
            pause: true,
            pane: None,
        }],
    );
    let ctx = ActionCtx {
        path: dir.clone(),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: None,
        pane_runner: None,
    };

    let outcome = execute(&a, &ctx, None);
    assert!(outcome.any_success, "failure: {:?}", outcome.failure);
    assert!(marker.exists(), "child did not see PATH");
    fs::remove_dir_all(&dir).unwrap();
}

/// An `env` step's value is exported to later children as well as being
/// visible to later templates.
#[test]
fn an_env_step_exports_to_later_children() {
    let dir = temp_dir("env-export");
    let marker = dir.join("exported");
    let a = action(
        "export",
        OnFailure::Abort,
        vec![
            Step::Env { set: vec![("SCOUT_EXPORTED".into(), t("{name}"))] },
            Step::Spawn {
                argv: vec![
                    t("/bin/sh"),
                    t("-c"),
                    t(&format!("test -n \"$SCOUT_EXPORTED\" && touch {}", marker.display())),
                ],
                wait: true,
                cwd: None,
                pause: true,
                pane: None,
            },
        ],
    );
    let ctx = ActionCtx {
        path: dir.clone(),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: None,
        pane_runner: None,
    };

    let outcome = execute(&a, &ctx, None);
    assert!(outcome.any_success, "failure: {:?}", outcome.failure);
    assert!(marker.exists(), "the env step did not reach the child");
    fs::remove_dir_all(&dir).unwrap();
}

/// With `print_to` set, a print step writes to the file and nothing to
/// stdout, which is what lets a session's children keep stdout.
#[test]
fn print_steps_go_to_the_print_to_file_when_one_is_given() {
    let dir = temp_dir("print-to");
    let sink = dir.join("commands");
    let a = action("go", OnFailure::Abort, vec![Step::Print { format: t("cd {path}") }]);
    let ctx = ActionCtx {
        path: dir.clone(),
        query: String::new(),
        home: dir.display().to_string(),
        print_to: Some(sink.clone()),
        pane_runner: None,
    };
    let outcome = execute(&a, &ctx, None);
    assert!(outcome.any_success, "{:?}", outcome.failure);
    let written = fs::read_to_string(&sink).unwrap();
    assert_eq!(written, format!("cd '{}'\n", dir.display()));
    // A second print appends: the wrapper reads the file whole.
    execute(&a, &ctx, None);
    assert_eq!(fs::read_to_string(&sink).unwrap().lines().count(), 2);
    fs::remove_dir_all(&dir).unwrap();
}
