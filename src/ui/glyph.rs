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

/// Frecency signal meter, 0-3. A light/medium/heavy weight ramp,
/// replacing the block-element ramp `▁▄█` (all Ambiguous).
pub const SIGNAL: [&str; 4] = [
    "   ",
    "\u{2758}  ",               // ❘
    "\u{2758}\u{2759} ",        // ❘❙
    "\u{2758}\u{2759}\u{275A}", // ❘❙❚
];

/// Query prompt.
pub const PROMPT: &str = "\u{276F}"; // ❯

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
        let mut sources: Vec<(&str, String)> = vec![
            ("ELLIPSIS", ELLIPSIS.to_string()),
            ("SELECTED", SELECTED.to_string()),
            ("UNSELECTED", UNSELECTED.to_string()),
            ("PROMPT", PROMPT.to_string()),
            ("KIND_REPO", KIND_REPO.to_string()),
            ("KIND_DIR", KIND_DIR.to_string()),
            ("KIND_FILE", KIND_FILE.to_string()),
        ];
        for (i, level) in SIGNAL.iter().enumerate() {
            sources.push(("SIGNAL", format!("[{i}] {level}")));
        }

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
    /// level must occupy the same number of columns as every other.
    #[test]
    fn signal_levels_are_equal_width() {
        let widths: Vec<usize> = SIGNAL
            .iter()
            .map(|level| level.chars().map(|c| c.width().unwrap_or(0)).sum())
            .collect();
        assert_eq!(widths, vec![3, 3, 3, 3], "signal ramp levels differ in width: {widths:?}");
    }

    /// Selected and unselected rows must start at the same column, or
    /// the list shifts by a column as the cursor moves.
    #[test]
    fn selection_marker_matches_its_blank() {
        let w = |s: &str| -> usize { s.chars().map(|c| c.width().unwrap_or(0)).sum() };
        assert_eq!(w(SELECTED), w(UNSELECTED));
    }
}
