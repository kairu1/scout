//! Drift guards for facts that deliberately live in more than one place.

/// The Rust version is triple-encoded (ADR-002 §MSRV makes the pin
/// doctrine): Cargo.toml `rust-version`, rust-toolchain.toml `channel`,
/// and the Containerfile base image. They must agree on the major.minor.
#[test]
fn toolchain_version_is_consistent() {
    let cargo = include_str!("../Cargo.toml");
    let toolchain = include_str!("../rust-toolchain.toml");
    let containerfile = include_str!("../Containerfile");

    let field = |src: &str, key: &str| -> String {
        src.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim().trim_start_matches('=').trim().trim_matches('"').to_string())
            .unwrap_or_else(|| panic!("`{key}` not found"))
    };
    let rust_version = field(cargo, "rust-version"); // "1.96"
    let channel = field(toolchain, "channel"); // "1.96.1"
    let base = containerfile
        .lines()
        .find_map(|l| {
            l.split_once("FROM rust:")
                .map(|(_, v)| v.split_whitespace().next().unwrap().to_string())
        })
        .expect("FROM rust: line");

    assert!(
        channel.starts_with(&rust_version),
        "toolchain {channel} vs Cargo rust-version {rust_version}"
    );
    assert!(channel.starts_with(&base), "toolchain {channel} vs Containerfile rust:{base}");
}

/// The shell wrapper is duplicated verbatim in the README for
/// paste-ability; shell/scout.bash is canonical. Edit one, this fails
/// until the other matches.
#[test]
fn readme_carries_the_canonical_wrapper() {
    let readme = include_str!("../README.md");
    let wrapper = include_str!("../shell/scout.bash");
    assert!(
        readme.contains(wrapper),
        "README.md wrapper block has drifted from shell/scout.bash"
    );
}
