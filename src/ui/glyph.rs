//! Every ornament glyph scout draws, in one place, with a guard.
//!
//! A glyph whose East Asian Width class is **Ambiguous** occupies one
//! column on most terminals and two on a terminal configured for a CJK
//! locale. `unicode-width` reports Ambiguous as narrow (`width()`) or
//! wide (`width_cjk()`), and the caller picks — which means *we* are
//! choosing a number the reader's terminal may disagree with. Every
//! layout calculation downstream of that choice is then wrong for that
//! reader, and only for that reader.
//!
//! ADR-005 made display width the unit of layout. This module closes the
//! matching hole on the other side: the characters scout itself emits
//! must have exactly one width, in every terminal, so the arithmetic is
//! not a bet. `ornaments_are_width_unambiguous` is the guard — it fails
//! the build if a glyph is admitted whose width depends on the reader.
//!
//! **Exempt, deliberately:** the hairline rule and the action-menu
//! border. Both are box-drawing characters, both are Ambiguous, and
//! neither feeds any arithmetic: the hairline is a solid fill that
//! ratatui clips to the area, so a double-width render still spans the
//! width, and the border is drawn by ratatui's own widget from a
//! `BorderType` we merely select. Replacing them would cost legibility
//! and buy nothing. The exemption is narrow and stated rather than
//! silent.

/// Marks a path truncated on the left. Two ASCII dots rather than `…`
/// (U+2026), which is Ambiguous: `truncate_left` spends an exact column
/// budget on this marker, so its width cannot be allowed to vary.
pub const ELLIPSIS: &str = "..";

/// Selected-row marker. Replaces `▌` (U+258C, Ambiguous), which sat in
/// a fixed-width row prefix and shifted the whole line when it rendered
/// wide.
pub const SELECTED: &str = "\u{2759}"; // ❙ MEDIUM VERTICAL BAR
/// Unselected rows pay the same width, so rows align.
pub const UNSELECTED: &str = " ";

/// Query prompt.
pub const PROMPT: &str = "\u{276F}"; // ❯

/// Query-row cursor. Replaces `█` (U+2588, Ambiguous).
pub const CURSOR: &str = "\u{275A}"; // ❚ HEAVY VERTICAL BAR

/// Separator between hints in the footer. Replaces `·` (U+00B7,
/// Ambiguous).
pub const SEPARATOR: &str = "\u{2758}"; // ❘ LIGHT VERTICAL BAR

/// Dash inside prose banners. Replaces `—` (U+2014, Ambiguous). Banner
/// text is wrapped and clipped by ratatui rather than measured by us,
/// but it shares a line with nothing, so the cheap ASCII form costs
/// nothing and keeps the rule uniform.
pub const DASH: &str = "-";

/// The hairline rule. Ambiguous by nature — every box-drawing character
/// is — and exempt for the reason in this module's header: it is a solid
/// fill that ratatui clips to the area, so a double-width render still
/// spans the width and no arithmetic depends on it. Named here so the
/// exemption is visible rather than buried in a call site.
pub const HAIRLINE: &str = "\u{2500}"; // ─

/// Kind markers (ADR-007 §Decision 8c).
pub const KIND_REPO: char = '\u{2442}'; // ⑂ OCR FORK — a git repository
pub const KIND_DIR: char = '\u{2023}'; // ‣ TRIANGULAR BULLET — a directory
pub const KIND_FILE: char = ' ';

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthChar;

    /// The guard. `width()` reads Ambiguous as narrow and `width_cjk()`
    /// reads it as wide, so a character on which they disagree is one
    /// whose column count depends on the reader's locale. Scout must not
    /// emit such a character anywhere its width is load-bearing.
    ///
    /// If this fails, the fix is to choose a different glyph — not to
    /// relax the test. The whole point is that the alternative is
    /// invisible in local testing and only breaks for someone else.
    #[test]
    fn ornaments_are_width_unambiguous() {
        let sources: Vec<(&str, String)> = vec![
            ("ELLIPSIS", ELLIPSIS.to_string()),
            ("SELECTED", SELECTED.to_string()),
            ("UNSELECTED", UNSELECTED.to_string()),
            ("PROMPT", PROMPT.to_string()),
            ("CURSOR", CURSOR.to_string()),
            ("SEPARATOR", SEPARATOR.to_string()),
            ("DASH", DASH.to_string()),
            ("KIND_REPO", KIND_REPO.to_string()),
            ("KIND_DIR", KIND_DIR.to_string()),
            ("KIND_FILE", KIND_FILE.to_string()),
        ];
        for (name, text) in &sources {
            for c in text.chars() {
                let narrow = c.width().unwrap_or(0);
                let wide = c.width_cjk().unwrap_or(0);
                assert_eq!(
                    narrow, wide,
                    "{name}: U+{:04X} is East Asian Width Ambiguous — {narrow} column(s) on a \
                     western terminal, {wide} on a CJK one. Pick a glyph with one width.",
                    c as u32
                );
            }
        }
    }

    /// The meter is positioned by padding computed elsewhere, so every
    /// The guard above only sees the constants in this module. It saw
    /// nothing for weeks while five Ambiguous glyphs sat as inline
    /// literals in `ui/mod.rs` — the query cursor, the menu marker, the
    /// footer separator and two em dashes. A gate that covers the
    /// declarations and not the call sites covers the easy half.
    ///
    /// This closes the class: no ornament may be written inline. Every
    /// glyph scout emits is declared here, where the width guard can
    /// see it.
    #[test]
    fn no_glyph_literals_outside_this_module() {
        // Scope is the point. This guard used to read `mod.rs` alone
        // while its own docstring claimed to "close the class" — and
        // Ambiguous-width literals sat unguarded in `main.rs` and
        // `doctor.rs` the whole time. It now walks every source file.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders: Vec<String> = Vec::new();
        let mut checked = 0usize;
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("read src").flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                // This module is where glyphs are declared.
                if path.file_name().and_then(|f| f.to_str()) == Some("glyph.rs") {
                    continue;
                }
                checked += 1;
                let source = std::fs::read_to_string(&path).expect("read source");
                for (n, line) in source.lines().enumerate() {
                    // Tests carry deliberate CJK fixtures — the width
                    // rules are exactly what they exercise. In this
                    // codebase test modules sit at the end of the file,
                    // so stop there.
                    if line.trim_start().starts_with("#[cfg(test)]") {
                        break;
                    }
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    for c in line.chars().filter(|c| !c.is_ascii()) {
                        offenders.push(format!(
                            "{}:{} U+{:04X} {c}",
                            path.file_name().unwrap().to_string_lossy(),
                            n + 1,
                            c as u32
                        ));
                    }
                }
            }
        }
        assert!(checked > 5, "guard walked only {checked} files; the walk is broken");
        assert!(
            offenders.is_empty(),
            "inline non-ASCII glyphs outside glyph.rs: {offenders:#?} — declare them in \
             glyph.rs so the width guard covers them"
        );
    }

    /// Selected and unselected rows must start at the same column, or
    /// the list shifts by a column as the cursor moves.
    #[test]
    fn selection_marker_matches_its_blank() {
        let w = |s: &str| -> usize { s.chars().map(|c| c.width().unwrap_or(0)).sum() };
        assert_eq!(w(SELECTED), w(UNSELECTED));
    }
}
