//! The loader's gates, the canonical projection, and trust behaviour.

use std::collections::BTreeMap;
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
schema_version = 2

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
            fs::write(&store, format!("v2 {hash} {}\n", config_path.display())).unwrap();
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

    let keys = BTreeMap::new();
    let expected = concat!(
        "[{\"name\":\"edit\",\"keybinding\":\"enter\",\"on_failure\":\"abort\",",
        "\"unsafe_shell_template\":false,\"steps\":[{\"kind\":\"spawn\",",
        "\"argv\":[\"subl\",\"{path}\"],\"wait\":true,\"cwd\":null,\"pause\":true,\"pane\":null}],\"when\":null},\n",
        "{\"name\":\"open-term\",\"keybinding\":null,\"on_failure\":\"abort\",",
        "\"unsafe_shell_template\":false,\"steps\":[{\"kind\":\"spawn\",",
        "\"argv\":[\"alacritty\",\"--working-directory\",\"{path}\"],",
        "\"wait\":false,\"cwd\":null,\"pause\":true,\"pane\":null}],\"when\":null}]\n",
        "{\"keys\":{}}\n",
    );
    assert_eq!(canonical::projection(&user_actions, &keys), expected);
    assert!(canonical::HASH_HEADER.starts_with("scout/trust-hash-v2\n"));

    // Description edits must not change the hash (no re-prompt).
    let mut relabeled = user_actions.clone();
    relabeled[0].description = "something else".into();
    assert_eq!(
        canonical::trust_hash(&user_actions, &keys),
        canonical::trust_hash(&relabeled, &keys)
    );

    // A keybinding change must change the hash (re-prompt).
    let mut rebound = user_actions.clone();
    rebound[1].keybinding = Some("enter".into());
    assert_ne!(canonical::trust_hash(&user_actions, &keys), canonical::trust_hash(&rebound, &keys));

    // A `when` clause is execution-relevant and changes the hash; its keys
    // are emitted sorted, marker always as an array.
    let mut scoped = user_actions.clone();
    scoped[0].when = Some(
        scout::actions::When::new(
            Some(scout::actions::Kind::Repo),
            vec!["Cargo.toml".into()],
            vec![],
            Some("~/w/**".into()),
            None,
            Some(scout::actions::Mode::Session),
            "/home/u",
        )
        .unwrap(),
    );
    assert_ne!(canonical::trust_hash(&user_actions, &keys), canonical::trust_hash(&scoped, &keys));
    let projected = canonical::projection(&scoped, &keys);
    assert!(
        projected.contains(
            "\"when\":{\"glob\":\"~/w/**\",\"kind\":\"repo\",\"marker\":[\"Cargo.toml\"],\"mode\":\"session\"}"
        ),
        "{projected}"
    );

    // A `[keys]` table enters the hash too.
    let mut bound = BTreeMap::new();
    bound.insert("split-right".to_string(), "alt-right".to_string());
    assert_ne!(
        canonical::trust_hash(&user_actions, &keys),
        canonical::trust_hash(&user_actions, &bound)
    );

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_loader_refuses_each_invalid_shape_and_names_it() {
    let dir = temp_dir("gates");
    // (config text, a word the message must contain so the user can find
    // the offending thing). These check the message names the problem,
    // which is a UX contract; the error kind is checked separately.
    let cases: &[(&str, &str)] = &[
        ("schema_version = 3\n", "schema_version"),
        ("schema_version = 2\nbogus = true\n", "bogus"),
        ("schema_version = 2\n[scout]\ntheme = \"dark\"\n", "unknown key"),
        ("schema_version = 2\n[scout]\nsession = \"yes\"\n", "boolean"),
        // when: unknown key, empty table, bad kind, bad marker, bad ext, bad finding
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = { color = \"red\" }\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "color",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = {}\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "omit it",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = { kind = \"symlink\" }\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "repo|dir|file",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = { mode = \"tmux\" }\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "session|one-shot",
        ),
        // an empty argv is a shell only in a pane
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [] } ]\n",
            "with `pane`",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = { marker = \"src/Cargo.toml\" }\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "bare file name",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = { ext = [\".rs\"] }\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "without the dot",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = { finding = \"World Writable\" }\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "check name",
        ),
        // session fields: a pane is neither waited for nor paused
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"x\"], pane = \"split-right\", wait = true } ]\n",
            "`pane` cannot be combined with `wait = true`",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"x\"], pane = \"split-right\", pause = false } ]\n",
            "`pane` cannot be combined with `pause`",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"x\"], pane = \"sideways\" } ]\n",
            "split-right|split-down|new-window",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"x\"], pause = \"no\" } ]\n",
            "`pause` must be a boolean",
        ),
        // [keys]: unknown operation, bad chord, picker-owned key, two ops on
        // one key, a key an action already binds
        ("schema_version = 2\n[keys]\nfly = \"alt-f\"\n", "unknown operation"),
        ("schema_version = 2\n[keys]\nzoom = \"z\"\n", "is not a key"),
        ("schema_version = 2\n[keys]\nzoom = \"ctrl-c\"\n", "the picker itself uses"),
        ("schema_version = 2\n[keys]\nzoom = \"alt-q\"\nclose-pane = \"alt-q\"\n", "both use"),
        (
            "schema_version = 2\n[keys]\nzoom = \"alt-e\"\n[[action]]\nname = \"a\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "already an action's keybinding",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"{pat}\" } ]\n",
            "unknown placeholder",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"subl --wait {path}\"] } ]\n",
            "single-slot",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"cd {path} && make\"] } ]\n",
            "unsafe_shell_template",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\", wait = true } ]\n",
            "not valid on a `print` step",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"spawn\", argv = \"subl {path}\" } ]\n",
            "array of strings",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
            "duplicate",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nkeybinding = \"enter\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n[[action]]\nname = \"b\"\nkeybinding = \"enter\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
            "enter",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"env\", set = { \"1BAD\" = \"x\" } } ]\n",
            "POSIX",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"env\", set = {} } ]\n",
            "at least one entry",
        ),
        ("schema_version = 2\n[[action]]\nname = \"a\"\nsteps = []\n", "1-32"),
        (
            "schema_version = 2\n[[action]]\nname = \"a\"\non_failure = \"retry\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
            "on_failure",
        ),
        (
            "schema_version = 2\n[[action]]\nname = \"édit\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
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

/// `sh -c '<text>' sh {path}`: the placeholder is a positional parameter
/// the text reads as `"$1"`. The shell parses the text, which is fixed,
/// and never the parameter, so this needs no attestation; a placeholder
/// inside the text still does.
#[test]
fn a_placeholder_as_a_positional_parameter_of_sh_c_needs_no_attestation() {
    let dir = temp_dir("positional");
    let config = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"files\"\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"find . -type f | grep -i \\\"$1\\\"\", \"sh\", \"{query}\"] } ]\n",
    )
    .unwrap();
    assert!(config.actions.iter().any(|a| a.name == "files" && !a.unsafe_shell_template));
    let message = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"files\"\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"find . | grep {query}\", \"sh\"] } ]\n",
    )
    .unwrap_err()
    .to_string();
    assert!(message.contains("unsafe_shell_template"), "{message}");
    // A positional parameter is an ordinary element: it may not mix a
    // placeholder with anything else.
    let message = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"files\"\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"grep \\\"$1\\\" .\", \"sh\", \"{query} --\"] } ]\n",
    )
    .unwrap_err()
    .to_string();
    assert!(message.contains("argv[4]") && message.contains("mixes a placeholder"), "{message}");
    fs::remove_dir_all(&dir).unwrap();
}

/// `[keys]` operations exist only in a session, so an action's chord
/// that a session never offers is no clash.
#[test]
fn a_one_shot_only_chord_does_not_block_a_keys_entry() {
    let dir = temp_dir("keys-mode");
    let config = load_pretrusted(
        &dir,
        "schema_version = 2\n[keys]\nclose-pane = \"alt-x\"\n\
         [[action]]\nname = \"a\"\nkeybinding = \"alt-x\"\nwhen = { mode = \"one-shot\" }\n\
         steps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    let err = load_pretrusted(
        &dir,
        "schema_version = 2\n[keys]\nclose-pane = \"alt-x\"\n\
         [[action]]\nname = \"a\"\nkeybinding = \"alt-x\"\nwhen = { mode = \"session\" }\n\
         steps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap_err();
    assert!(err.to_string().contains("already an action's keybinding"), "{err}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_attested_sh_c_loads() {
    let dir = temp_dir("attested");
    let config = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"make\"\nunsafe_shell_template = true\nsteps = [ { kind = \"spawn\", argv = [\"sh\", \"-c\", \"cd {path} && make\"] } ]\n",
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
        "schema_version = 2\n[[action]]\nname = \"a\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
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
        "schema_version = 2\n[[action]]\nname = \"a\"\nkeybinding = \"tab\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
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
        "schema_version = 2\n[[action]]\nname = \"a\"\nkeybinding = \"f7\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert!(config.warnings.iter().any(|w| w.contains("f7")), "{:?}", config.warnings);
    fs::remove_dir_all(&dir).unwrap();
}

/// The session set: an action may reuse a name, a chord or `enter` when
/// the two never meet in one run. `for_mode` is what keeps them apart.
#[test]
fn a_name_and_a_chord_are_shared_across_disjoint_modes_only() {
    let dir = temp_dir("mode-share");
    let twins = "schema_version = 2\n\
        [[action]]\nname = \"go\"\nkeybinding = \"enter\"\nwhen = { mode = \"one-shot\" }\n\
        steps = [ { kind = \"print\", format = \"cd {path}\" } ]\n\
        [[action]]\nname = \"go\"\nkeybinding = \"enter\"\nwhen = { mode = \"session\" }\n\
        steps = [ { kind = \"spawn\", argv = [], cwd = \"{dir}\", pane = \"split-right\" } ]\n\
        [[action]]\nname = \"status\"\nkeybinding = \"alt-s\"\nwhen = { marker = \".git\", mode = \"one-shot\" }\n\
        steps = [ { kind = \"print\", format = \"git status\" } ]\n\
        [[action]]\nname = \"status\"\nkeybinding = \"alt-s\"\nwhen = { marker = \".git\", mode = \"session\" }\n\
        steps = [ { kind = \"spawn\", argv = [\"git\", \"status\"], cwd = \"{repo_root}\" } ]\n\
        [[action]]\nname = \"both\"\nkeybinding = \"alt-b\"\n\
        steps = [ { kind = \"spawn\", argv = [\"true\"] } ]\n";
    let config = load_pretrusted(&dir, twins).unwrap();
    let user = |c: &scout::config::Config| -> Vec<String> {
        c.actions.iter().filter(|a| a.from_user_config).map(|a| a.name.clone()).collect()
    };
    assert_eq!(user(&config), ["go", "go", "status", "status", "both"]);

    let session = config.for_mode(true);
    assert_eq!(user(&session), ["go", "status", "both"]);
    let go = session.enter_action().expect("enter is bound in a session");
    assert!(
        matches!(go.steps.as_slice(), [scout::actions::Step::Spawn { argv, pane: Some(_), .. }] if argv.is_empty()),
        "the session `go` is the pane one"
    );
    let one_shot = config.for_mode(false);
    assert_eq!(user(&one_shot), ["go", "status", "both"]);
    assert!(matches!(
        one_shot.enter_action().unwrap().steps.as_slice(),
        [scout::actions::Step::Print { .. }]
    ));
    // The projection carries the mode after marker, and an empty argv.
    let user_actions: Vec<_> =
        config.actions.iter().filter(|a| a.from_user_config).cloned().collect();
    let projected = canonical::projection(&user_actions, &BTreeMap::new());
    assert!(
        projected.contains("\"when\":{\"marker\":[\".git\"],\"mode\":\"session\"}"),
        "{projected}"
    );
    assert!(projected.contains("\"argv\":[],\"wait\":true,\"cwd\":\"{dir}\""), "{projected}");

    // The same name with one side unmoded meets in a session: refused.
    let clash = twins.replace(
        "name = \"go\"\nkeybinding = \"enter\"\nwhen = { mode = \"session\" }\n",
        "name = \"go\"\nkeybinding = \"alt-g\"\n",
    );
    let message = load_pretrusted(&dir, &clash).unwrap_err().to_string();
    assert!(message.contains("duplicate action name `go`"), "{message}");
    // Two session actions on one chord: refused.
    let clash = twins.replace(
        "keybinding = \"alt-b\"\n",
        "keybinding = \"alt-s\"\nwhen = { mode = \"session\" }\n",
    );
    let message = load_pretrusted(&dir, &clash).unwrap_err().to_string();
    assert!(message.contains("alt-s") && message.contains("more than one action"), "{message}");
    // `enter` shared with an unmoded action: refused.
    let clash = twins.replace("keybinding = \"alt-b\"\n", "keybinding = \"enter\"\n");
    let message = load_pretrusted(&dir, &clash).unwrap_err().to_string();
    assert!(message.contains("more than one action binds `enter`"), "{message}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_duplicate_chord_is_refused_not_resolved() {
    let dir = temp_dir("keybind-dup");
    let err = load_pretrusted(
        &dir,
        "schema_version = 2\n         [[action]]\nname = \"a\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n         [[action]]\nname = \"b\"\nkeybinding = \"alt-e\"\nsteps = [ { kind = \"print\", format = \"y\" } ]\n",
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
        "schema_version = 2\n[[action]]\nname = \"a\"\nkeybinding = \"ctrl-c\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
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
    fs::write(&real_target, "schema_version = 2\n").unwrap();

    // Entry 1: symlink, falls through. Entry 2: valid file, wins.
    let link = dir.join("linked.toml");
    std::os::unix::fs::symlink(&real_target, &link).unwrap();
    let second = dir.join("second.toml");
    fs::write(&second, "schema_version = 2\n").unwrap();
    let store = dir.join("store");
    if let Err(Error::TrustRequiresTty { hash, .. }) =
        load(&[link.clone(), second.clone()], store.clone(), true)
    {
        fs::write(&store, format!("v2 {hash} {}\n", second.display())).unwrap();
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

/// A v1 file is refused, and the refusal names what to change and what
/// changed. Nothing is silently accepted.
#[test]
fn a_v1_config_is_refused_with_migration_advice() {
    let dir = temp_dir("v1");
    let err = load_pretrusted(
        &dir,
        "schema_version = 1\n[[action]]\nname = \"a\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap_err();
    assert!(matches!(err, Error::ConfigSchemaVersion { found: 1, supported: 2, .. }), "{err:?}");
    assert!(err.to_string().contains("schema_version = 2"), "{err}");
    let hint = err.hint().expect("migration advice");
    assert!(hint.why.contains("when"), "{}", hint.why);
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scout_session_is_the_one_allowed_setting() {
    let dir = temp_dir("session");
    let config = load_pretrusted(&dir, "schema_version = 2\n[scout]\nsession = true\n").unwrap();
    assert!(config.session);
    let config = load_pretrusted(&dir, "schema_version = 2\n").unwrap();
    assert!(!config.session, "default is exit after one action");
    fs::remove_dir_all(&dir).unwrap();
}

/// The `when` clause parses into the model: a single marker string becomes
/// a one-element list, and every key lands.
#[test]
fn a_when_clause_parses_into_the_action() {
    let dir = temp_dir("when");
    let config = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"test\"\nwhen = { kind = \"repo\", marker = \"Cargo.toml\", ext = [\"rs\"], glob = \"~/w/**\", finding = \"suid\" }\nsteps = [ { kind = \"spawn\", argv = [\"cargo\", \"test\"] } ]\n\n[[action]]\nname = \"any\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    let test = config.actions.iter().find(|a| a.name == "test").unwrap();
    let when = test.when.as_ref().expect("clause parsed");
    assert_eq!(when.kind, Some(scout::actions::Kind::Repo));
    assert_eq!(when.marker, vec!["Cargo.toml".to_string()]);
    assert_eq!(when.ext, vec!["rs".to_string()]);
    assert_eq!(when.glob.as_deref(), Some("~/w/**"));
    assert_eq!(when.finding.as_deref(), Some("suid"));
    assert!(config.actions.iter().find(|a| a.name == "any").unwrap().when.is_none());
    fs::remove_dir_all(&dir).unwrap();
}

/// `pause` defaults on for a waited child and can be switched off; `pane`
/// parses to its operation and drops the wait.
#[test]
fn session_step_fields_parse() {
    use scout::actions::{PaneOp, Step};
    let dir = temp_dir("session-fields");
    let config = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"edit\"\nsteps = [ { kind = \"spawn\", argv = [\"vi\", \"{path}\"], pause = false } ]\n\n[[action]]\nname = \"serve\"\nsteps = [ { kind = \"spawn\", argv = [\"npm\", \"start\"], pane = \"split-right\" } ]\n\n[[action]]\nname = \"test\"\nsteps = [ { kind = \"spawn\", argv = [\"make\", \"test\"] } ]\n\n[[action]]\nname = \"go\"\nsteps = [ { kind = \"print\", format = \"cd {path}\" } ]\n",
    )
    .unwrap();
    let by = |name: &str| config.actions.iter().find(|a| a.name == name).unwrap();
    match &by("edit").steps[0] {
        Step::Spawn { pause, pane, wait, .. } => {
            assert!(!pause);
            assert!(pane.is_none());
            assert!(wait);
        }
        other => panic!("{other:?}"),
    }
    match &by("serve").steps[0] {
        Step::Spawn { pane, .. } => assert_eq!(*pane, Some(PaneOp::SplitRight)),
        other => panic!("{other:?}"),
    }
    match &by("test").steps[0] {
        Step::Spawn { pause, .. } => assert!(pause, "pause defaults on"),
        other => panic!("{other:?}"),
    }
    // Session classification: only a print step ends the session; only a
    // waited, pausing, in-process spawn pauses.
    assert!(by("go").ends_session());
    assert!(!by("test").ends_session());
    assert!(by("test").pauses());
    assert!(!by("edit").pauses(), "an editor owned the screen; nothing to read afterwards");
    assert!(!by("serve").pauses(), "a pane is not waited for");
    fs::remove_dir_all(&dir).unwrap();
}

/// `[keys]` resolves over the defaults; an explicit entry wins, a default
/// that collides with an action chord yields with a warning, and the
/// explicit entries (only) enter the trust hash.
#[test]
fn keys_table_resolves_over_defaults_and_is_hashed() {
    use scout::config::keys::Operation;
    let dir = temp_dir("keys");
    let with_keys = load_pretrusted(
        &dir,
        "schema_version = 2\n[keys]\nzoom = \"Alt-Shift-Z\"\n[[action]]\nname = \"win\"\nkeybinding = \"alt-w\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert_eq!(with_keys.keys.key_for(Operation::Zoom), Some("alt-shift-z"), "normalised");
    assert_eq!(with_keys.keys.key_for(Operation::SplitRight), Some("alt-right"), "default kept");
    assert_eq!(with_keys.keys.key_for(Operation::NewWindow), None, "alt-w belongs to the action");
    assert!(
        with_keys.warnings.iter().any(|w| w.contains("new-window")),
        "{:?}",
        with_keys.warnings
    );
    assert_eq!(with_keys.keys.key_for(Operation::Reindex), Some("ctrl-r"));

    let without = load_pretrusted(
        &dir,
        "schema_version = 2\n[[action]]\nname = \"win\"\nkeybinding = \"alt-w\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert_ne!(with_keys.trust_hash, without.trust_hash, "a [keys] entry changes the hash");
    let empty_table = load_pretrusted(
        &dir,
        "schema_version = 2\n[keys]\n[[action]]\nname = \"win\"\nkeybinding = \"alt-w\"\nsteps = [ { kind = \"print\", format = \"x\" } ]\n",
    )
    .unwrap();
    assert_eq!(empty_table.trust_hash, without.trust_hash, "an empty table hashes like none");
    fs::remove_dir_all(&dir).unwrap();
}

/// `[scout]` names how a session uses tmux: a policy, a session name and
/// a server. Values outside the closed sets, and a session name tmux
/// would rewrite, refuse the file. None of it enters the trust hash.
#[test]
fn scout_tmux_settings_parse_validate_and_stay_out_of_the_hash() {
    use scout::tmux::{Policy, Server};
    let dir = temp_dir("tmux-settings");
    let config = load_pretrusted(
        &dir,
        "schema_version = 2\n[scout]\nsession = true\ntmux = \"require\"\n\
         tmux_session = \"work-2\"\ntmux_server = \"shared\"\n",
    )
    .unwrap();
    assert_eq!(config.tmux, Policy::Require);
    assert_eq!(config.tmux_session, "work-2");
    assert_eq!(config.tmux_server, Server::Shared);
    let hash_a = config.trust_hash.clone().unwrap();

    let config = load_pretrusted(&dir, "schema_version = 2\n").unwrap();
    assert_eq!(config.tmux, Policy::Auto, "default policy");
    assert_eq!(config.tmux_session, "scout");
    assert_eq!(config.tmux_server, Server::Private);
    assert_eq!(config.trust_hash.unwrap(), hash_a, "tmux settings do not change what runs");

    for (text, problem) in [
        ("schema_version = 2\n[scout]\ntmux = \"sometimes\"\n", "auto"),
        ("schema_version = 2\n[scout]\ntmux = true\n", "string"),
        ("schema_version = 2\n[scout]\ntmux_session = \"a;b\"\n", "not allowed"),
        ("schema_version = 2\n[scout]\ntmux_session = \"a.b\"\n", "not allowed"),
        ("schema_version = 2\n[scout]\ntmux_session = \"\"\n", "1 to 64"),
        ("schema_version = 2\n[scout]\ntmux_server = \"mine\"\n", "private"),
    ] {
        match load_pretrusted(&dir, text) {
            Err(Error::ConfigInvalid { message, .. }) => {
                assert!(message.contains(problem), "{text:?}: {message}")
            }
            other => panic!("{text:?} must be refused as invalid: {other:?}"),
        }
    }
    fs::remove_dir_all(&dir).unwrap();
}
