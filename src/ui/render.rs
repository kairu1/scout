//! Pure render helpers — kept ratatui-free so the visual grammar is
//! unit-testable: path cell classification (dim dir / bold basename /
//! accent match), home shortening, and the frecency signal meter.

/// Visual class of one displayed character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// Directory portion — rendered dim.
    Dir,
    /// Final path component — rendered bold.
    Base,
    /// Matcher-hit character — rendered in the accent colour.
    Match,
}

/// Classify `path` into displayable cells: `$HOME` prefix collapses to
/// `~`, C0/C1 control chars are stripped at this boundary (ADR-003 §6;
/// tab becomes a plain space), `match_indices` (char positions in the
/// ORIGINAL path) win over dir/base classification. Matches that fall
/// inside the collapsed `~` prefix are dropped with it.
pub fn path_cells(path: &str, home: &str, match_indices: &[u32]) -> Vec<(char, CellKind)> {
    let chars: Vec<char> = path.chars().collect();
    let home_chars = home.chars().count();
    let collapse_home = !home.is_empty()
        && path.starts_with(home)
        && (chars.len() == home_chars || chars.get(home_chars) == Some(&'/'));

    let base_start = path
        .rfind('/')
        .map(|byte| path[..byte].chars().count() + 1)
        .unwrap_or(0);

    let mut cells = Vec::with_capacity(chars.len());
    let mut start = 0;
    if collapse_home {
        cells.push(('~', CellKind::Dir));
        start = home_chars;
    }
    for (i, &c) in chars.iter().enumerate().skip(start) {
        let code = c as u32;
        let c = match code {
            0x09 => ' ',
            0x00..=0x1f | 0x80..=0x9f => continue,
            _ => c,
        };
        let kind = if match_indices.contains(&(i as u32)) {
            CellKind::Match
        } else if i >= base_start {
            CellKind::Base
        } else {
            CellKind::Dir
        };
        cells.push((c, kind));
    }
    cells
}

/// Left-truncate to `width` cells, keeping the tail (where basenames
/// live) and marking the cut with a leading ellipsis.
///
/// KNOWN LIMITATION (shadow-review 2026-07-06): width is counted in
/// chars, not terminal display columns, so a path with wide (CJK/emoji)
/// glyphs mis-aligns the signal-meter column. The correct fix needs
/// `unicode-width`, which is off the ADR-002 roster — admitting it is a
/// successor-ADR decision, deferred to v2. Cosmetic; no correctness or
/// security impact (control bytes are already stripped).
pub fn truncate_left(cells: &mut Vec<(char, CellKind)>, width: usize) {
    if cells.len() > width && width > 0 {
        let drop = cells.len() - width + 1;
        cells.drain(..drop);
        cells.insert(0, ('…', CellKind::Dir));
    }
}

/// Frecency signal meter: 0-3 strength levels derived from ranking's
/// `K_FREC` (level 3 = saturation ~ a daily driver, level 2 ~ 0.3·K,
/// level 1 ~ 0.05·K ~ touched this week). Deriving from the exported
/// constant keeps the meter in step with any ranking re-tune.
pub fn signal_level(s_now: f64) -> usize {
    let k = crate::search::ranking::K_FREC;
    if s_now >= k {
        3
    } else if s_now >= 0.3 * k {
        2
    } else if s_now >= 0.05 * k {
        1
    } else {
        0
    }
}

pub const SIGNAL_GLYPHS: [&str; 4] = ["   ", "\u{2581}  ", "\u{2581}\u{2584} ", "\u{2581}\u{2584}\u{2588}"];

#[cfg(test)]
mod tests {
    use super::*;

    fn render(cells: &[(char, CellKind)]) -> String {
        cells.iter().map(|(c, _)| *c).collect()
    }

    #[test]
    fn home_collapses_and_base_is_classified() {
        let cells = path_cells("/home/agent/projects/scout", "/home/agent", &[]);
        assert_eq!(render(&cells), "~/projects/scout");
        // "scout" chars are Base, the rest Dir.
        let kinds: Vec<CellKind> = cells.iter().map(|(_, k)| *k).collect();
        assert!(kinds[..11].iter().all(|k| *k == CellKind::Dir));
        assert!(kinds[11..].iter().all(|k| *k == CellKind::Base));
        // Not a prefix match on a sibling dir: /home/agentx must not collapse.
        let cells = path_cells("/home/agentx/f", "/home/agent", &[]);
        assert_eq!(render(&cells), "/home/agentx/f");
    }

    #[test]
    fn match_indices_survive_home_collapse_shift() {
        // Match on "scout" at original char positions 21..26.
        let path = "/home/agent/projects/scout";
        let indices: Vec<u32> = (21..26).collect();
        let cells = path_cells(path, "/home/agent", &indices);
        let matched: String =
            cells.iter().filter(|(_, k)| *k == CellKind::Match).map(|(c, _)| *c).collect();
        assert_eq!(matched, "scout");
    }

    #[test]
    fn control_chars_strip_without_breaking_match_alignment() {
        // ESC at char index 4; match on "abc" at indices 5..8.
        let path = "/tmp\u{1b}abc";
        let cells = path_cells(path, "", &[5, 6, 7]);
        assert_eq!(render(&cells), "/tmpabc");
        let matched: String =
            cells.iter().filter(|(_, k)| *k == CellKind::Match).map(|(c, _)| *c).collect();
        assert_eq!(matched, "abc");
    }

    #[test]
    fn truncation_keeps_tail() {
        let mut cells = path_cells("/very/long/dir/base", "", &[]);
        truncate_left(&mut cells, 9);
        assert_eq!(render(&cells), "…dir/base");
        assert_eq!(cells.len(), 9);
    }

    #[test]
    fn signal_levels_derive_from_k_frec() {
        let k = crate::search::ranking::K_FREC;
        // Saturation is exactly K_FREC, so a ranking re-tune moves both
        // the blend and the meter together (drift guard).
        assert_eq!(signal_level(k), 3);
        assert_eq!(signal_level(k - 0.01), 2);
        assert_eq!(signal_level(0.0), 0);
        assert_eq!(signal_level(0.06 * k), 1);
        assert_eq!(SIGNAL_GLYPHS.len(), 4);
    }

    #[test]
    fn path_cells_strip_matches_strip_clean() {
        // The inline C0/C1 strip in path_cells must stay equivalent to
        // the canonical strip::clean over a control-char corpus
        // (ADR-003 §6 lives in one place, guarded here).
        let corpus = "plain\tpath\x00\x1b\x07\u{85}\u{9f}end/base\u{1b}]0;x\u{7}z";
        let rendered: String =
            path_cells(corpus, "", &[]).iter().map(|(c, _)| *c).collect();
        assert_eq!(rendered, crate::ui::strip::clean(corpus));
    }
}
