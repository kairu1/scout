//! Drift guards for facts that deliberately live in two places.

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
