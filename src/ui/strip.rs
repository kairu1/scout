//! Render-boundary strip filter (ADR-003 §6). Every string that reaches
//! ratatui passes through here: C0 stripped (tab preserved as plain
//! whitespace), C1 stripped. Lossy and deliberate — the DB keeps the
//! canonical bytes; display re-derives per render.

pub fn clean(value: &str) -> String {
    value
        .chars()
        .filter_map(|c| {
            let code = c as u32;
            match code {
                0x09 => Some(' '),
                0x00..=0x1f => None,
                0x80..=0x9f => None,
                _ => Some(c),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn strips_c0_c1_preserves_tab_as_space() {
        assert_eq!(clean("plain/path"), "plain/path");
        assert_eq!(clean("a\tb"), "a b");
        assert_eq!(clean("\x1b]0;owned\x07title"), "]0;ownedtitle");
        assert_eq!(clean("nul\0newline\n"), "nulnewline");
        assert_eq!(clean("c1\u{85}gone"), "c1gone");
        assert_eq!(clean("unicode ü中 stays"), "unicode ü中 stays");
    }
}
