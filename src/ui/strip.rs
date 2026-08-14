//! Render-boundary strip filter (ADR-003 §6, revised 2026-08-14). Every
//! string that reaches ratatui passes through here. Lossy and deliberate
//! — the DB keeps the canonical bytes; display re-derives per render.
//!
//! Removed: C0 (tab preserved as plain whitespace), C1, and the
//! bidirectional / zero-width formatting characters that let two
//! different paths render identically (the Trojan Source class,
//! CVE-2021-42574). `keep` is the single rule; every strip in the
//! codebase calls it rather than re-deriving it.

/// Bidi and zero-width formatting characters. A path containing
/// `U+202E` displays its components in an order that does not match the
/// path an action will receive — the user approves one string and
/// executes another. Zero-width characters are the quieter half of the
/// same problem: they make two distinct paths indistinguishable on
/// screen, so "is this the project I meant?" stops being answerable by
/// looking.
const BIDI_AND_ZERO_WIDTH: [u32; 14] = [
    0x061C, // ARABIC LETTER MARK
    0x200B, // ZERO WIDTH SPACE
    0x200C, // ZERO WIDTH NON-JOINER
    0x200D, // ZERO WIDTH JOINER
    0x200E, // LEFT-TO-RIGHT MARK
    0x200F, // RIGHT-TO-LEFT MARK
    0x202A, // LEFT-TO-RIGHT EMBEDDING
    0x202B, // RIGHT-TO-LEFT EMBEDDING
    0x202C, // POP DIRECTIONAL FORMATTING
    0x202D, // LEFT-TO-RIGHT OVERRIDE
    0x202E, // RIGHT-TO-LEFT OVERRIDE
    0x2066, // LEFT-TO-RIGHT ISOLATE
    0x2067, // RIGHT-TO-LEFT ISOLATE
    0x2068, // FIRST STRONG ISOLATE
            // 0x2069 POP DIRECTIONAL ISOLATE is appended below to keep this
            // array's length honest against the list above.
];

/// The single strip rule (ADR-003 §6). `None` drops the character.
///
/// One function rather than a rule repeated at each render site: the
/// duplicate previously lived in the row builder and was kept honest by
/// a parity test, which is a weaker arrangement than not duplicating it.
pub fn keep(c: char) -> Option<char> {
    let code = c as u32;
    match code {
        0x09 => Some(' '),
        0x00..=0x1f => None,
        0x80..=0x9f => None,
        0x7F => None,            // DELETE - a C0-class control the header claims
        0x2069 => None,          // POP DIRECTIONAL ISOLATE
        0x180E => None,          // MONGOLIAN VOWEL SEPARATOR
        0x2060..=0x2064 => None, // WORD JOINER and invisible operators
        0xFEFF => None,          // ZERO WIDTH NO-BREAK SPACE / BOM
        // Tag characters: a whole invisible alphabet.
        0xE0000..=0xE007F => None,
        _ if BIDI_AND_ZERO_WIDTH.contains(&code) => None,
        _ => Some(c),
    }
}

pub fn clean(value: &str) -> String {
    value.chars().filter_map(keep).collect()
}

#[cfg(test)]
mod tests {
    use super::{clean, keep};

    #[test]
    fn strips_c0_c1_preserves_tab_as_space() {
        assert_eq!(clean("plain/path"), "plain/path");
        assert_eq!(clean("a\tb"), "a b");
        assert_eq!(clean("\x1b]0;owned\x07title"), "]0;ownedtitle");
        assert_eq!(clean("nul\0newline\n"), "nulnewline");
        assert_eq!(clean("c1\u{85}gone"), "c1gone");
        assert_eq!(clean("unicode ü中 stays"), "unicode ü中 stays");
    }

    /// ADR-003 §6 revision. The attack: a directory whose name contains
    /// U+202E renders its tail reversed, so what the user reads is not
    /// what the action receives. Stripping the control returns the
    /// display to logical order, which is the order that will execute.
    #[test]
    fn strips_bidi_overrides_so_display_matches_execution() {
        let spoofed = "invoice\u{202E}gpj.exe";
        assert_eq!(clean(spoofed), "invoicegpj.exe");
        assert!(!clean(spoofed).chars().any(|c| c as u32 == 0x202E));
    }

    /// Zero-width characters make two distinct paths look identical.
    #[test]
    fn strips_zero_width_so_distinct_paths_look_distinct() {
        let a = "pay\u{200B}ments";
        let b = "payments";
        assert_ne!(a, b, "the inputs really are different");
        assert_eq!(clean(a), clean(b), "and they rendered identically before");
        assert_eq!(clean(a), "payments");
        // U+2060 is U+FEFF's functional twin; stripping one and not the
        // other left the hole open.
        assert_eq!(clean("pay\u{2060}ments"), "payments");
    }

    #[test]
    fn strips_every_named_formatting_control() {
        for code in [
            0x7Fu32, 0x061C, 0x180E, 0x200B, 0x200C, 0x200D, 0x200E, 0x200F, 0x202A, 0x202B,
            0x202C, 0x202D, 0x202E, 0x2060, 0x2061, 0x2062, 0x2063, 0x2064, 0x2066, 0x2067, 0x2068,
            0x2069, 0xFEFF, 0xE0001, 0xE0041,
        ] {
            let c = char::from_u32(code).unwrap();
            assert_eq!(keep(c), None, "U+{code:04X} must not reach the terminal");
        }
    }

    /// The strip must not become a general Unicode filter: ordinary
    /// non-ASCII filenames are the normal case, not the threat.
    #[test]
    fn leaves_ordinary_text_alone() {
        for s in ["日本語版プロジェクト", "café", "Ωmega", "проект", "emoji-\u{1F600}"]
        {
            assert_eq!(clean(s), s, "{s} must survive");
        }
    }
}
