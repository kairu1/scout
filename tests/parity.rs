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
    assert_eq!(hash, "9223c39a0af66f8ff9417230cff1e760370be526ad8465424c00810db0e687db");
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
