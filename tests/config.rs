//! The loader's gates, the canonical projection, and trust behaviour.

use std::fs;
use std::path::PathBuf;

use scout::config::{canonical, load, load_file};
use scout::Error;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-cfg-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

const EXAMPLE: &str = r#"
schema_version = 1

[[action]]
name = "edit"
description = "Open the selection in subl"
keybinding = "enter"
steps = [ { kind = "spawn", argv = ["subl", "{path}"], wait = true } ]

[[action]]
name = "open-term"
steps = [ { kind = "spawn", argv = ["alacritty", "--working-directory", "{path}"], wait = false } ]
"#;

/// Load a config file, pre-trusting it by round-tripping the hash the
/// non-TTY refusal reports. Tests never have a TTY; this is exactly the
/// refuse-then-verify workflow automation is meant to use.
fn load_pretrusted(
    dir: &std::path::Path,
    config_toml: &str,
) -> Result<scout::config::Config, Error> {
    let config_path = dir.join("config.toml");
    fs::write(&config_path, config_toml).unwrap();
    let store = dir.join("trusted-config.sha256");
    match load_file(&config_path, store.clone(), true) {
        Err(Error::TrustRequiresTty { hash, .. }) => {
            fs::write(&store, format!("{hash} {}\n", config_path.display())).unwrap();
            load_file(&config_path, store, true)
        }
        other => other,
    }
}

#[test]
fn a_valid_config_loads_and_merges_with_the_defaults() {
    let dir = temp_dir("valid");
    let config = load_pretrusted(&dir, EXAMPLE).unwrap();

    // The user's `edit` replaces the compiled default wholly; the
    // `print-path` default survives; `open-term` is present.
    let names: Vec<&str> = config.actions.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["edit", "open-term", "print-path"]);
    let edit = &config.actions[0];
    assert!(edit.from_user_config, "user edit must replace the compiled default");
    assert_eq!(config.enter_action().unwrap().name, "edit");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_canonical_projection_is_byte_stable_for_the_documented_example() {
    let dir = temp_dir("canonical");
    let config = load_pretrusted(&dir, EXAMPLE).unwrap();
    let user_actions: Vec<_> =
        config.actions.iter().filter(|a| a.from_user_config).cloned().collect();

    let expected = concat!(
        "[{\"name\":\"edit\",\"keybinding\":\"enter\",\"on_failure\":\"abort\",",
        "\"unsafe_shell_template\":false,\"steps\":[{\"kind\":\"spawn\",",
        "\"argv\":[\"subl\",\"{path}\"],\"wait\":true,\"cwd\":null}]},\n",
        "{\"name\":\"open-term\",\"keybinding\":null,\"on_failure\":\"abort\",",
        "\"unsafe_shell_template\":false,\"steps\":[{\"kind\":\"spawn\",",
        "\"argv\":[\"alacritty\",\"--working-directory\",\"{path}\"],",
        "\"wait\":false,\"cwd\":null}]}]\n",
    );
    assert_eq!(canonical::projection(&user_actions), expected);

    // Description edits must not change the hash (no re-prompt).
    let mut relabeled = user_actions.clone();
    relabeled[0].description = "something else".into();
    assert_eq!(canonical::trust_hash(&user_actions), canonical::trust_hash(&relabeled));

    // A keybinding change must change the hash (re-prompt).
    let mut rebound = user_actions.clone();
    rebound[1].keybinding = Some("enter".into());
    assert_ne!(canonical::trust_hash(&user_actions), canonical::trust_hash(&rebound));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_loader_refuses_each_invalid_shape_and_names_it() {
    let dir = temp_dir("gates");
    // (config text, a word the message must contain so the user can find
    // the offending thing). These check the message names the problem,
    // which is a UX contract; the error kind is checked separately.
    let cases: &[(&str, &str)] = &[
        ("schema_version = 2\n", "schema_version"),
        ("schema_version = 1\nbogus = true\n", "bogus"),
        ("schema_version = 1\n[scout]\ntheme = \"dark\"\n", "reserved"),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"{pat}\" } ]\n",
            "unknown placeholder",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"subl --wait {path}\"] } ]\n",
            "single-slot",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"cd {path} && make\"] } ]\n",
            "unsafe_shell_template",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\", wait = true } ]\n",
            "not valid on a `print` step",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = \"subl {path}\" } ]\n",
            "array of strings",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
            "duplicate",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"enter\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n[[action]]\nname = \"b\"\nkeybinding = \"enter\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
            "enter",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"env\", set = { \"1BAD\" = \"x\" } } ]\n",
            "POSIX",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"env\", set = {} } ]\n",
            "at least one entry",
        ),
        ("schema_version = 1\n[[action]]\nname = \"a\"\nsteps = []\n", "1-32"),
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\non_failure = \"retry\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "on_failure",
        ),
        (
            "schema_version = 1\n[[action]]\nname = \"édit\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "ASCII",
        ),
    ];

    for (i, (toml_text, needle)) in cases.iter().enumerate() {
        let err = load_pretrusted(&dir, toml_text).unwrap_err();
        let message = err.to_string();
        assert!(message.contains(needle), "case {i}: expected `{needle}` in error, got: {message}");
        assert!(
            matches!(
                err,
                Error::ConfigSchemaVersion { .. }
                    | Error::ConfigToml { .. }
                    | Error::ConfigInvalid { .. }
            ),
            "case {i}: a refusal must be a config error, got {err:?}"
        );
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_attested_sh_c_loads() {
    let dir = temp_dir("attested");
    let config = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"make\"\nunsafe_shell_template = true\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"cd {path} && make\"] } ]\n",
    )
    .unwrap();
    assert!(config.actions.iter().any(|a| a.name == "make" && a.unsafe_shell_template));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn chord_bindings_load_without_a_warning() {
    let dir = temp_dir("keybind");
    let config = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert!(
        !config.warnings.iter().any(|w| w.contains("alt-e")),
        "alt-e dispatches; warnings: {:?}",
        config.warnings
    );
    // The compiled default still owns enter.
    assert_eq!(config.enter_action().unwrap().name, "edit");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn binding_tab_warns_and_says_why() {
    let dir = temp_dir("keybind-tab");
    let config = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"tab\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert!(
        config.warnings.iter().any(|w| w.contains("tab") && w.contains("action pane")),
        "{:?}",
        config.warnings
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_unknown_keybinding_warns() {
    let dir = temp_dir("keybind-unknown");
    let config = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"f7\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert!(config.warnings.iter().any(|w| w.contains("f7")), "{:?}", config.warnings);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_duplicate_chord_is_refused_not_resolved() {
    let dir = temp_dir("keybind-dup");
    let err = load_pretrusted(
        &dir,
        "schema_version = 1\n         [[action]]\nname = \"a\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n         [[action]]\nname = \"b\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("alt-e"), "{message}");
    assert!(message.contains("more than one action"), "{message}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_picker_owned_chord_is_refused() {
    let dir = temp_dir("keybind-owned");
    let err = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"ctrl-c\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap_err();
    let message = err.to_string();
    assert!(message.contains("ctrl-c") && message.contains("reserved"), "{message}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_changed_config_refuses_without_a_tty() {
    let dir = temp_dir("trust");
    let first = load_pretrusted(&dir, EXAMPLE).unwrap();
    assert!(first.trust_hash.is_some());

    // Mutate a step: the stored hash no longer matches.
    let config_path = dir.join("config.toml");
    fs::write(&config_path, EXAMPLE.replace("subl", "code")).unwrap();
    let err = load_file(&config_path, dir.join("trusted-config.sha256"), true).unwrap_err();
    assert!(matches!(err, Error::TrustRequiresTty { .. }), "got: {err}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn discovery_takes_the_first_regular_file_skips_symlinks_and_halts_on_a_parse_error() {
    let dir = temp_dir("discover");
    let real_target = dir.join("target.toml");
    fs::write(&real_target, "schema_version = 1\n").unwrap();

    // Entry 1: symlink, falls through. Entry 2: valid file, wins.
    let link = dir.join("linked.toml");
    std::os::unix::fs::symlink(&real_target, &link).unwrap();
    let second = dir.join("second.toml");
    fs::write(&second, "schema_version = 1\n").unwrap();
    let store = dir.join("store");
    if let Err(Error::TrustRequiresTty { hash, .. }) =
        load(&[link.clone(), second.clone()], store.clone(), true)
    {
        fs::write(&store, format!("{hash} {}\n", second.display())).unwrap();
    }
    let config = load(&[link.clone(), second.clone()], store.clone(), true).unwrap();
    assert_eq!(config.source.as_deref(), Some(second.as_path()));

    // A file that parses badly halts; it does not fall through.
    let broken = dir.join("broken.toml");
    fs::write(&broken, "schema_version = ").unwrap();
    let err = load(&[broken, second.clone()], store.clone(), true).unwrap_err();
    assert!(matches!(err, Error::ConfigToml { .. }), "got: {err}");

    // Nothing in the chain: compiled defaults only, no trust prompt.
    let config = load(&[dir.join("absent.toml")], store, true).unwrap();
    assert!(config.source.is_none());
    assert_eq!(config.actions.len(), 2);

    fs::remove_dir_all(&dir).unwrap();
}

/// The trust store must be 0600 from the moment it exists, not after a
/// write-then-chmod window.
#[test]
fn the_trust_store_is_written_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_dir("store-mode");
    let _ = load_pretrusted(&dir, EXAMPLE).unwrap();
    // load_pretrusted writes the store itself; make the loader record one.
    let mut store = scout::config::trust::TrustStore::load(dir.join("fresh-store")).unwrap();
    store.record(&dir.join("config.toml"), "deadbeef").unwrap();
    let mode = fs::metadata(dir.join("fresh-store")).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "trust store mode");
    fs::remove_dir_all(&dir).unwrap();
}
