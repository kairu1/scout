//! Drift guards for facts that deliberately live in more than one place.

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
