//! Engineers — loader gates, canonical projection, trust behaviour
//! (ADR-003 §3, ADR-004 §8-§10).

use std::fs;
use std::path::PathBuf;

use scout::config::loader::{load, load_file, LoadError};
use scout::config::{canonical, template::Template};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-cfg-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

const ADR_EXAMPLE: &str = r#"
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
/// non-TTY refusal reports (tests never have a TTY; ADR-003 §3 requires
/// exactly this refuse-then-verify workflow for automation).
fn load_pretrusted(dir: &PathBuf, config_toml: &str) -> Result<scout::config::Config, LoadError> {
    let config_path = dir.join("config.toml");
    fs::write(&config_path, config_toml).unwrap();
    let store = dir.join("trusted-config.sha256");
    match load_file(&config_path, store.clone(), true) {
        Err(LoadError::NonTtyUntrusted { hash, .. }) => {
            fs::write(&store, format!("{hash} {}\n", config_path.display())).unwrap();
            load_file(&config_path, store, true)
        }
        other => other,
    }
}

#[test]
fn valid_config_loads_and_merges_defaults() {
    let dir = temp_dir("valid");
    let config = load_pretrusted(&dir, ADR_EXAMPLE).unwrap();

    // User `edit` replaces the compiled default wholly; print-path
    // default survives; open-term present.
    let names: Vec<&str> = config.actions.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["edit", "open-term", "print-path"]);
    let edit = &config.actions[0];
    assert!(edit.from_user_config, "user edit must replace the compiled default");
    assert_eq!(config.enter_action().unwrap().name, "edit");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn canonical_projection_matches_adr_004_example() {
    let dir = temp_dir("canonical");
    let config = load_pretrusted(&dir, ADR_EXAMPLE).unwrap();
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
fn refusal_gates() {
    let dir = temp_dir("gates");
    let cases: &[(&str, &str)] = &[
        // wrong schema version
        ("schema_version = 2\n", "schema_version"),
        // unknown top-level key
        ("schema_version = 1\nbogus = true\n", "bogus"),
        // reserved [scout] key
        ("schema_version = 1\n[scout]\ntheme = \"dark\"\n", "reserved"),
        // unknown placeholder
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"{pat}\" } ]\n",
            "unknown placeholder",
        ),
        // single-slot violation
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"subl --wait {path}\"] } ]\n",
            "single-slot",
        ),
        // sh -c with placeholder, no attestation
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"cd {path} && make\"] } ]\n",
            "unsafe_shell_template",
        ),
        // wait on a print step
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\", wait = true } ]\n",
            "not valid on a `print` step",
        ),
        // single-string argv
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = \"subl {path}\" } ]\n",
            "array of strings",
        ),
        // duplicate names
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
            "duplicate",
        ),
        // two enter bindings
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"enter\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n[[action]]\nname = \"b\"\nkeybinding = \"enter\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
            "enter",
        ),
        // bad env name
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"env\", set = { \"1BAD\" = \"x\" } } ]\n",
            "POSIX",
        ),
        // empty env set
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"env\", set = {} } ]\n",
            "at least one entry",
        ),
        // zero steps
        ("schema_version = 1\n[[action]]\nname = \"a\"\nsteps = []\n", "1-32"),
        // bad on_failure
        (
            "schema_version = 1\n[[action]]\nname = \"a\"\non_failure = \"retry\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "on_failure",
        ),
        // non-ascii name
        (
            "schema_version = 1\n[[action]]\nname = \"édit\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "ASCII",
        ),
    ];

    for (i, (toml_text, needle)) in cases.iter().enumerate() {
        let err = load_pretrusted(&dir, toml_text).unwrap_err();
        let message = err.to_string();
        assert!(
            message.contains(needle),
            "case {i}: expected `{needle}` in error, got: {message}"
        );
    }

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn attested_sh_c_loads() {
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
fn unknown_keybinding_warns_but_loads() {
    let dir = temp_dir("keybind");
    let config = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"a\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert!(config.warnings.iter().any(|w| w.contains("alt-e") && w.contains("not dispatched")));
    // The compiled default still owns enter.
    assert_eq!(config.enter_action().unwrap().name, "edit");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn trust_changed_config_refuses_non_tty() {
    let dir = temp_dir("trust");
    let first = load_pretrusted(&dir, ADR_EXAMPLE).unwrap();
    assert!(first.trust_hash.is_some());

    // Mutate a step: the stored hash no longer matches → non-TTY refuse.
    let config_path = dir.join("config.toml");
    fs::write(&config_path, ADR_EXAMPLE.replace("subl", "code")).unwrap();
    let err = load_file(&config_path, dir.join("trusted-config.sha256"), true).unwrap_err();
    assert!(matches!(err, LoadError::NonTtyUntrusted { .. }), "got: {err}");

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn discovery_first_wins_symlink_falls_through_parse_error_halts() {
    let dir = temp_dir("discover");
    let real_target = dir.join("target.toml");
    fs::write(&real_target, "schema_version = 1\n").unwrap();

    // Entry 1: symlink → falls through. Entry 2: valid file → wins.
    let link = dir.join("linked.toml");
    std::os::unix::fs::symlink(&real_target, &link).unwrap();
    let second = dir.join("second.toml");
    fs::write(&second, "schema_version = 1\n").unwrap();
    let store = dir.join("store");
    // Pre-trust second.toml via the refuse-then-verify round trip.
    if let Err(LoadError::NonTtyUntrusted { hash, .. }) =
        load(&[link.clone(), second.clone()], store.clone(), true)
    {
        fs::write(&store, format!("{hash} {}\n", second.display())).unwrap();
    }
    let config = load(&[link.clone(), second.clone()], store.clone(), true).unwrap();
    assert_eq!(config.source.as_deref(), Some(second.as_path()));

    // A file that parses badly HALTS; it does not fall through.
    let broken = dir.join("broken.toml");
    fs::write(&broken, "schema_version = ").unwrap();
    let err = load(&[broken, second.clone()], store.clone(), true).unwrap_err();
    assert!(matches!(err, LoadError::Toml { .. }), "got: {err}");

    // Nothing in the chain → compiled defaults only, no trust prompt.
    let config = load(&[dir.join("absent.toml")], store, true).unwrap();
    assert!(config.source.is_none());
    assert_eq!(config.actions.len(), 2);

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn template_escapes_and_quoting() {
    // {{ }} escapes to literal braces.
    let t = Template::parse("a {{literal}} b").unwrap();
    assert!(!t.has_placeholder());

    // POSIX quoting at the print seam.
    use scout::config::template::posix_single_quote;
    assert_eq!(posix_single_quote("/plain/path"), "'/plain/path'");
    assert_eq!(posix_single_quote("/it's here"), "'/it'\\''s here'");
}
