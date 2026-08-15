//! Drift guards for facts that deliberately live in more than one place.

use std::fs;
use std::path::{Path, PathBuf};

use scout::config::loader::{load_file, LoadError};
use scout::config::template::ExpandCtx;
use scout::config::Step;

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
/// own copy of the thing it checks, which is how the `failure_hint`
/// parity test came to pass while guarding nothing.
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

fn load_pretrusted(dir: &Path) -> scout::config::Config {
    let config_path = dir.join("config.toml");
    fs::write(&config_path, REFERENCE_CONFIG).unwrap();
    let store = dir.join("trusted-config.sha256");
    match load_file(&config_path, store.clone(), true) {
        Err(LoadError::NonTtyUntrusted { hash, .. }) => {
            fs::write(&store, format!("{hash} {}\n", config_path.display())).unwrap();
            load_file(&config_path, store, true).expect("reference config must load once trusted")
        }
        other => other.expect("reference config must load"),
    }
}

/// `examples/config.toml` is a product artifact: it ships in the release
/// tarball and the README tells people to copy it verbatim. Nothing was
/// checking that it still parses, let alone that what it prints is a
/// shape the shipped wrapper will actually run — so a stray `{` in a
/// template, or a wrapper prefix change, would have reached a user as a
/// "refusing to eval unexpected output" they had no way to diagnose.
#[test]
fn reference_config_loads_and_every_printed_line_survives_the_wrapper() {
    let dir = temp_dir("refcfg");
    let config = load_pretrusted(&dir);

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

/// The Rust version is dual-encoded (ADR-002 §MSRV makes the pin
/// doctrine): Cargo.toml `rust-version` and rust-toolchain.toml
/// `channel`. They must agree on the major.minor.
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
    let rust_version = field(cargo, "rust-version"); // "1.96"
    let channel = field(toolchain, "channel"); // "1.96.1"

    assert!(
        channel.starts_with(&rust_version),
        "toolchain {channel} vs Cargo rust-version {rust_version}"
    );
}

/// The shell wrapper is duplicated verbatim in the README for
/// paste-ability; shell/scout.bash is canonical. Edit one, this fails
/// until the other matches.
#[test]
fn readme_carries_the_canonical_wrapper() {
    let readme = include_str!("../README.md");
    let wrapper = include_str!("../shell/scout.bash");
    assert!(readme.contains(wrapper), "README.md wrapper block has drifted from shell/scout.bash");
}

/// The transitive-crate ceiling is dual-encoded: CI enforces a number,
/// ADR-002 §Decision states one. They drifted — CI moved to 150 while
/// the ADR's normative line still said 120, and nothing noticed.
///
/// CI is the source of truth because it is the thing that actually
/// fails; this asserts the ADR quotes the same number.
#[test]
fn transitive_ceiling_matches_between_ci_and_adr() {
    let ci = include_str!("../.github/workflows/ci.yml");
    let adr = include_str!("../docs/adr/002-dependency-roster.md");

    let enforced: u32 = ci
        .lines()
        .find_map(|l| l.trim().strip_prefix("test \"$count\" -lt "))
        .expect("CI must enforce a ceiling")
        .trim()
        .parse()
        .expect("ceiling is a number");

    let stated: u32 = adr
        .lines()
        .find_map(|l| {
            let at = l.find("Target **< ")? + "Target **< ".len();
            l[at..].split_whitespace().next()?.parse().ok()
        })
        .expect("ADR-002 must state the ceiling in its Decision");

    assert_eq!(
        enforced, stated,
        "CI enforces < {enforced} but ADR-002 states < {stated}; one of them is lying to a reader"
    );
}
