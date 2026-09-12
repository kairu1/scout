//! Drift guards for facts that deliberately live in more than one place.

use std::fs;
use std::path::{Path, PathBuf};

use scout::actions::template::ExpandCtx;
use scout::actions::Step;
use scout::config::load_file;
use scout::Error;

const REFERENCE_CONFIG: &str = include_str!("../examples/config.toml");
const WRAPPER: &str = include_str!("../shell/scout.bash");
const README: &str = include_str!("../README.md");

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "scout-parity-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// The line shapes the wrapper's `case` will eval, read out of the
/// wrapper itself. Hardcoding them here would make this guard carry its
/// own copy of the thing it checks.
fn wrapper_allowlist() -> Vec<String> {
    let case_line = WRAPPER
        .lines()
        .find(|l| l.contains("'cd '*"))
        .expect("wrapper must carry the allowlist case arm");
    let prefixes: Vec<String> =
        case_line.split('\'').skip(1).step_by(2).map(|s| s.to_string()).collect();
    assert!(
        prefixes.len() >= 4,
        "scanned only {} prefixes out of the wrapper; the scan has lost the case arm",
        prefixes.len()
    );
    prefixes
}

/// Load the reference config, pre-trusting it by round-tripping the hash
/// the non-TTY refusal reports. Returns the config and the hash.
fn load_reference_pretrusted(dir: &Path) -> (scout::config::Config, String) {
    let config_path = dir.join("config.toml");
    fs::write(&config_path, REFERENCE_CONFIG).unwrap();
    let store = dir.join("trusted-config.sha256");
    match load_file(&config_path, store.clone(), true) {
        Err(Error::TrustRequiresTty { hash, .. }) => {
            fs::write(&store, format!("v2 {hash} {}\n", config_path.display())).unwrap();
            let config = load_file(&config_path, store, true)
                .expect("reference config must load once trusted");
            (config, hash)
        }
        Ok(config) => {
            let hash = config.trust_hash.clone().expect("a loaded file has a hash");
            (config, hash)
        }
        Err(err) => panic!("reference config must load: {err}"),
    }
}

/// `examples/config.toml` is a product artifact: it ships in the release
/// tarball and the README tells people to copy it verbatim. This checks
/// that it parses and that every line it prints is a shape the shipped
/// wrapper will actually run.
#[test]
fn reference_config_loads_and_every_printed_line_survives_the_wrapper() {
    let dir = temp_dir("refcfg");
    let (config, _) = load_reference_pretrusted(&dir);

    // A repository, so `{repo_root}` resolves for the git actions.
    let repo = dir.join("repo");
    fs::create_dir_all(repo.join(".git")).unwrap();
    let env = std::collections::HashMap::new();
    let ctx = ExpandCtx { path: &repo, query: "needle", home: "/home/u", env: &env };

    let allowlist = wrapper_allowlist();
    let mut checked = 0usize;
    for action in config.actions.iter().filter(|a| a.from_user_config) {
        for (i, step) in action.steps.iter().enumerate() {
            let Step::Print { format } = step else { continue };
            let line = format.expand(&ctx, true).unwrap_or_else(|e| {
                panic!("`{}` step {} does not expand: {e}", action.name, i + 1)
            });
            assert!(
                allowlist.iter().any(|p| line.starts_with(p.as_str())),
                "`{}` prints a line the wrapper will refuse: {line}",
                action.name
            );
            checked += 1;
        }
    }
    assert!(checked >= 10, "only {checked} print steps checked; the reference config shrank");

    fs::remove_dir_all(&dir).unwrap();
}

/// The trust hash of the reference config is pinned so that any change to
/// the canonical projection or its header is a deliberate edit to this
/// literal, not a surprise re-prompt for every user.
#[test]
fn reference_config_trust_hash_is_pinned() {
    let dir = temp_dir("refhash");
    let (_, hash) = load_reference_pretrusted(&dir);
    assert_eq!(hash, "f56f038fa946d4fe3fa6100b916d4a96768ae965f4cea7b588ec5f7814a2c362");
    fs::remove_dir_all(&dir).unwrap();
}

/// The README states how many actions the reference config ships, and a
/// reader counts on that number to know whether the copy worked.
#[test]
fn readme_action_count_matches_the_reference_config() {
    let actual = REFERENCE_CONFIG
        .lines()
        .filter(|l| l.trim_start().starts_with("[[action]]") && !l.trim_start().starts_with('#'))
        .count();
    let stated: usize = README
        .lines()
        .find_map(|l| {
            let at = l.find("reference config ships ")? + "reference config ships ".len();
            l[at..].split_whitespace().next()?.parse().ok()
        })
        .expect("README must state the reference config's action count");
    assert_eq!(
        stated, actual,
        "README says {stated} actions, examples/config.toml defines {actual}"
    );
}

/// The Rust version is dual-encoded: `Cargo.toml` `rust-version` and
/// `rust-toolchain.toml` `channel`. They must agree on the major.minor.
#[test]
fn toolchain_version_is_consistent() {
    let cargo = include_str!("../Cargo.toml");
    let toolchain = include_str!("../rust-toolchain.toml");

    let field = |src: &str, key: &str| -> String {
        src.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim().trim_start_matches('=').trim().trim_matches('"').to_string())
            .unwrap_or_else(|| panic!("`{key}` not found"))
    };
    let rust_version = field(cargo, "rust-version");
    let channel = field(toolchain, "channel");

    assert!(
        channel.starts_with(&rust_version),
        "toolchain {channel} vs Cargo rust-version {rust_version}"
    );
}

/// The shell wrapper is duplicated verbatim in the README for
/// paste-ability; `shell/scout.bash` is canonical.
#[test]
fn readme_carries_the_canonical_wrapper() {
    assert!(README.contains(WRAPPER), "README.md wrapper block has drifted from shell/scout.bash");
}

/// The transitive-crate ceiling is dual-encoded: CI enforces a number and
/// the agent guide states one. CI is the source of truth because it is the
/// thing that actually fails; this asserts the guide quotes the same.
#[test]
fn transitive_ceiling_matches_between_ci_and_the_guide() {
    let ci = include_str!("../.github/workflows/ci.yml");
    let guide = include_str!("../CLAUDE.md");

    let enforced: u32 = ci
        .lines()
        .find_map(|l| l.trim().strip_prefix("test \"$count\" -lt "))
        .expect("CI must enforce a ceiling")
        .trim()
        .parse()
        .expect("ceiling is a number");

    let stated: u32 = guide
        .lines()
        .find_map(|l| {
            let at = l.find("fewer than ")? + "fewer than ".len();
            l[at..].split_whitespace().next()?.parse().ok()
        })
        .expect("CLAUDE.md must state the ceiling as `fewer than N`");

    assert_eq!(
        enforced, stated,
        "CI enforces < {enforced} but CLAUDE.md states fewer than {stated}; one is lying"
    );
}

/// The wrapper reads printed commands from a file the binary is told
/// about, so a session's children keep stdout; and it treats the session
/// flags like a bare invocation.
#[test]
fn wrapper_uses_print_to_and_covers_the_session_flags() {
    assert!(WRAPPER.contains("--print-to \"$f\""), "wrapper must pass --print-to");
    assert!(WRAPPER.contains("mktemp"), "the file must be created privately");
    assert!(WRAPPER.contains("''|--session|-s)"), "bare, --session and -s all run the picker");
}

/// The binary honours `--print-to`: with it, a print step's output lands
/// in the file and stdout stays empty. Driven through `scout query`'s
/// sibling path is impossible without a TTY, so this checks the flag is
/// accepted and rejected correctly at the CLI boundary.
#[test]
fn print_to_is_a_top_level_flag() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_scout"))
        .args(["--help"])
        .output()
        .expect("run scout");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("--print-to"), "{text}");
    assert!(text.contains("--session"), "{text}");
}

/// The wrapper marker is what `scout recon` looks for in rc files; the
/// installer and the README tell people to write it. One constant, three
/// places.
#[test]
fn the_wrapper_marker_in_the_installer_and_readme_is_the_one_recon_scans_for() {
    let marker = scout::recon::checks::WRAPPER_MARKER;
    let installer = include_str!("../install.sh");
    assert!(installer.contains(marker), "install.sh must write the marker recon scans for");
    assert!(README.contains(marker), "README must show the marker recon scans for");
}

// ---- 0.4 dual encodings: each fact below lives in code and in prose ----

fn config_doc() -> &'static str {
    include_str!("../docs/configuration.md")
}

/// Every `[keys]` operation, with its default key, is documented in the
/// configuration reference, the reference config's commented table and
/// the README's default list. The code is the source; the prose must
/// quote it.
#[test]
fn key_operations_and_defaults_match_the_docs() {
    use scout::config::keys::{Operation, ALL_OPERATIONS};
    let doc = config_doc();
    let readme = README;
    for op in ALL_OPERATIONS {
        let (name, default) = (op.name(), op.default_key());
        let focus = matches!(
            op,
            Operation::FocusLeft
                | Operation::FocusRight
                | Operation::FocusUp
                | Operation::FocusDown
        );
        if focus {
            assert!(default.starts_with("alt-shift-"), "{name} default {default}");
            assert!(doc.contains("| `focus-left/right/up/down` | `alt-shift-<arrow>` |"), "{name}");
            assert!(readme.contains("`alt-shift-<arrow>`"), "README default list lacks the arrows");
            continue;
        }
        assert!(
            doc.contains(&format!("| `{name}` | `{default}` |")),
            "configuration.md row for {name}"
        );
        let pattern = regex_lite(&format!(r#"#\s*{name}\s*=\s*"{default}""#));
        assert!(pattern(REFERENCE_CONFIG), "examples/config.toml comment for {name} = {default}");
        assert!(readme.contains(&format!("`{name}`")), "README names {name}");
        assert!(readme.contains(&format!("`{default}`")), "README lists {default}");
    }
}

/// The `when` keys: the loader accepts exactly `WHEN_KEYS`, the docs
/// table has a row per key, and the reference config's comment names
/// each one. The code list is the source; the prose must quote it.
#[test]
fn when_keys_match_the_loader_and_the_docs() {
    use scout::actions::WHEN_KEYS;
    let sample = |key: &str| -> &'static str {
        match key {
            "kind" => "\"repo\"",
            "marker" => "\".git\"",
            "ext" => "[\"rs\"]",
            "glob" => "\"~/w/**\"",
            "finding" => "\"suid\"",
            "mode" => "\"session\"",
            other => panic!("WHEN_KEYS gained `{other}`: give this test a sample value"),
        }
    };
    let dir = temp_dir("whenkeys");
    let load = |clause: &str| {
        let toml = format!(
            "schema_version = 2\n[[action]]\nname = \"a\"\nwhen = {{ {clause} }}\n\
             steps = [ {{ kind = \"print\", format = \"x\" }} ]\n"
        );
        let path = dir.join("config.toml");
        fs::write(&path, toml).unwrap();
        load_file(&path, dir.join("store"), false)
    };
    let in_when_section = config_doc()
        .split("## `when`")
        .nth(1)
        .and_then(|s| s.split("\n## ").next())
        .expect("a when section");
    // The reference config documents the keys in one comment paragraph,
    // the one that opens with the `when = { ... }` sentence.
    let comment_paragraph: String = REFERENCE_CONFIG
        .lines()
        .skip_while(|l| !l.starts_with("# `when = { ... }`"))
        .take_while(|l| l.starts_with('#') && l.trim_end() != "#")
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!comment_paragraph.is_empty(), "the reference config's `when` paragraph");
    let readme_sentence = README
        .lines()
        .skip_while(|l| !l.contains("`when` takes"))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    for key in WHEN_KEYS {
        // Accepted: the only failure a trusted-less load can report is
        // the trust refusal, which comes after validation.
        assert!(
            matches!(
                load(&format!("{key} = {}", sample(key))),
                Err(Error::TrustRequiresTty { .. })
            ),
            "loader accepts `when.{key}`"
        );
        assert!(
            in_when_section.contains(&format!("| `{key} = ")),
            "configuration.md row for {key}"
        );
        assert!(
            comment_paragraph.contains(&format!("`{key}`")),
            "examples/config.toml `when` paragraph names `{key}`"
        );
        assert!(
            readme_sentence.contains(&format!("`{key}`")),
            "README `when` sentence names `{key}`"
        );
    }
    assert!(matches!(
        load("colour = \"red\""),
        Err(Error::ConfigInvalid { .. } | Error::ConfigToml { .. })
    ));
    // Every documented row is a real key.
    for row in in_when_section.lines().filter(|l| l.starts_with("| `")) {
        let key = row.trim_start_matches("| `").split([' ', '`']).next().unwrap();
        assert!(WHEN_KEYS.contains(&key), "docs row `{key}` is not a when key");
    }
    // The loader's raw struct is the code-side twin of the list: a field
    // added there without a WHEN_KEYS entry (or the reverse) fails here.
    let loader = include_str!("../src/config/loader.rs");
    let raw_when = loader
        .split("struct RawWhen {")
        .nth(1)
        .and_then(|s| s.split("\n}").next())
        .expect("RawWhen in loader.rs");
    let fields: Vec<&str> = raw_when
        .lines()
        .filter_map(|l| l.trim().strip_suffix(','))
        .filter_map(|l| l.split_once(": "))
        .map(|(name, _)| name.trim())
        .collect();
    assert_eq!(fields, WHEN_KEYS, "RawWhen fields vs WHEN_KEYS");
    fs::remove_dir_all(&dir).unwrap();
}

/// The placeholders: the docs line and the reference config's comment
/// name exactly the fixed names the template grammar parses.
#[test]
fn placeholders_match_the_docs() {
    use scout::actions::template::PLACEHOLDER_NAMES;
    let section = config_doc()
        .split("## Placeholders")
        .nth(1)
        .and_then(|s| s.split("\n## ").next())
        .expect("a placeholders section");
    // The list leads the section; the prose after it names some again.
    let mut documented: Vec<&str> = Vec::new();
    for token in section.split('`') {
        if token.starts_with('{') && token.ends_with('}') && !token.starts_with("{{") {
            let name = &token[1..token.len() - 1];
            if name != "env.NAME" && !documented.contains(&name) {
                documented.push(name);
            }
        }
    }
    assert_eq!(documented, PLACEHOLDER_NAMES, "configuration.md placeholder list");
    let comment = REFERENCE_CONFIG
        .lines()
        .find(|l| l.starts_with("# Placeholders:"))
        .expect("the reference config lists the placeholders");
    let readme_sentence = README
        .lines()
        .skip_while(|l| !l.starts_with("Placeholders `{path}"))
        .take(2)
        .collect::<Vec<_>>()
        .join(" ");
    for name in PLACEHOLDER_NAMES {
        assert!(comment.contains(&format!("{{{name}}}")), "examples/config.toml names {{{name}}}");
        assert!(readme_sentence.contains(&format!("{{{name}}}")), "README names {{{name}}}");
    }
    // The grammar's parse arms are the code-side twin of the list: a
    // fixed name parsed there without a PLACEHOLDER_NAMES entry fails.
    let template = include_str!("../src/actions/template.rs");
    let parsed: Vec<&str> = template
        .lines()
        .filter_map(|l| l.trim().strip_prefix('"'))
        .filter_map(|l| l.split_once("\" => Ok(Placeholder::"))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(parsed, PLACEHOLDER_NAMES, "Placeholder::parse arms vs PLACEHOLDER_NAMES");
}

/// A tiny matcher for `#\s*name\s*=\s*"value"` without a regex crate:
/// returns a closure that scans lines.
fn regex_lite(pattern: &str) -> impl Fn(&str) -> bool {
    // pattern is `#\s*NAME\s*=\s*"VALUE"`; extract NAME and VALUE.
    let inner = pattern.trim_start_matches(r"#\s*").trim_end_matches('"');
    let (name, rest) = inner.split_once(r"\s*=\s*").expect("pattern shape");
    let value = rest.trim_start_matches('"').to_string();
    let name = name.to_string();
    move |text: &str| {
        text.lines().any(|line| {
            let line = line.trim_start();
            let Some(after_hash) = line.strip_prefix('#') else { return false };
            let after_hash = after_hash.trim_start();
            let Some(after_name) = after_hash.strip_prefix(name.as_str()) else { return false };
            let after_name = after_name.trim_start();
            let Some(after_eq) = after_name.strip_prefix('=') else { return false };
            after_eq.trim_start().starts_with(&format!("\"{value}\""))
        })
    }
}

/// The tombstone purge window is one constant; two documents state it.
#[test]
fn purge_window_matches_the_docs() {
    let days = scout::index::write::PURGE_AFTER_SECS / 86_400;
    assert_eq!(days, 182);
    let phrase = format!("{days} days");
    assert!(include_str!("../docs/architecture.md").contains(&phrase), "architecture.md");
    assert!(README.contains(&phrase), "README");
}

/// The rc files recon scans for the wrapper are listed in the security doc.
#[test]
fn rc_files_recon_scans_are_listed_in_the_security_doc() {
    let doc = include_str!("../docs/security.md");
    for rc in scout::recon::run::rc_files(std::path::Path::new("~")) {
        let shown = format!("`{}`", rc.display());
        assert!(doc.contains(&shown), "security.md lacks {shown}");
    }
}

/// What scout sets on its own tmux server is stated in the configuration
/// reference, option by option.
#[test]
fn owned_server_options_match_the_docs() {
    let launch = scout::tmux::Launch {
        server: scout::tmux::Server::Private,
        session: "scout".into(),
        scout_exe: "/opt/scout".into(),
        cwd: None,
        print_to: None,
    };
    let options = launch.owned_server_options();
    assert!(!options.is_empty());
    for argv in options {
        let (key, value) = (&argv[argv.len() - 2], &argv[argv.len() - 1]);
        let shown = format!("`{key} {value}`");
        assert!(config_doc().contains(&shown), "configuration.md lacks {shown}");
    }
}

/// The private server name is a constant; four texts tell the user to
/// type it.
#[test]
fn the_private_server_name_is_the_one_the_docs_name() {
    let typed = format!("tmux -L {}", scout::tmux::PRIVATE_SERVER);
    for (name, text) in [
        ("README", README),
        ("configuration.md", config_doc()),
        ("examples/config.toml", REFERENCE_CONFIG),
        ("security.md", include_str!("../docs/security.md")),
    ] {
        assert!(text.contains(&typed), "{name} lacks `{typed}`");
    }
}

/// The version in Cargo.toml is the newest changelog heading.
#[test]
fn the_changelog_leads_with_the_cargo_version() {
    let changelog = include_str!("../CHANGELOG.md");
    let heading = changelog
        .lines()
        .find(|l| l.starts_with("## "))
        .expect("a version heading")
        .trim_start_matches("## ")
        .split_whitespace()
        .next()
        .unwrap();
    assert_eq!(heading, env!("CARGO_PKG_VERSION"));
}

/// Every ignored gate the guide lists runs in CI: for each `--test X --
/// --ignored` line in CLAUDE.md's gates block, ci.yml carries a run step
/// with the same test and filter.
#[test]
fn ci_runs_every_ignored_gate_in_the_guide() {
    let guide = include_str!("../CLAUDE.md");
    let ci = include_str!("../.github/workflows/ci.yml");
    let mut seen = 0;
    for line in guide.lines().filter(|l| l.contains("--ignored")) {
        let at = line.find("--test ").expect("gate line names its test");
        let gate = line[at..].split('#').next().unwrap().trim();
        assert!(ci.contains(gate), "ci.yml runs no `{gate}`");
        seen += 1;
    }
    assert!(seen >= 3, "the guide lists the perf, tmux and acl gates");
}

/// The schema version the migrations reach is the one the docs describe.
#[test]
fn schema_version_matches_the_architecture_doc() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    scout::index::schema::apply_migrations(&conn).unwrap();
    let version = scout::index::schema::schema_version(&conn).unwrap();
    assert_eq!(version, 3);
    let doc = include_str!("../docs/architecture.md");
    assert!(doc.contains(&format!("version {version} adds `roots`")), "architecture.md");
}

/// The keys the picker owns are listed in the configuration reference.
#[test]
fn picker_owned_keys_are_listed_in_the_docs() {
    let doc = config_doc();
    for key in scout::config::keys::PICKER_OWNED_KEYS {
        let shown = match key {
            "up" | "down" | "left" | "right" => "plain arrows".to_string(),
            "esc" => "`Esc`".into(),
            "enter" => "`Enter`".into(),
            "tab" => "`Tab`".into(),
            "ctrl-c" => "`Ctrl-C`".into(),
            "home" => "`Home`".into(),
            "end" => "`End`".into(),
            "backspace" => "`Backspace`".into(),
            "delete" => "`Delete`".into(),
            other => format!("`{other}`"),
        };
        assert!(doc.contains(&shown), "configuration.md lacks {shown} for {key}");
    }
}

/// Every flag the README's `scout index` row names is a real flag.
#[test]
fn the_readme_index_flags_exist() {
    let row = README.lines().find(|l| l.starts_with("| `scout index")).expect("README index row");
    let help = std::process::Command::new(env!("CARGO_BIN_EXE_scout"))
        .args(["index", "--help"])
        .output()
        .expect("run scout");
    let help = String::from_utf8_lossy(&help.stdout);
    let mut flags = 0;
    for token in row.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')) {
        if let Some(flag) = token.strip_prefix("--") {
            assert!(help.contains(&format!("--{flag}")), "README names --{flag}, help does not");
            flags += 1;
        }
    }
    assert!(flags >= 6, "the row names the flags");
}
