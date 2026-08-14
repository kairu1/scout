//! Pure render helpers — kept ratatui-free so the visual grammar is
//! unit-testable: path cell classification (dim dir / bold basename /
//! accent match), home shortening, terminal-column measurement and
//! truncation (ADR-005), and the frecency signal meter.

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

    let base_start = path.rfind('/').map(|byte| path[..byte].chars().count() + 1).unwrap_or(0);

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

/// Terminal columns these cells occupy (ADR-005). The single source of
/// truth for every layout calculation downstream.
///
/// `None` — which `unicode-width` returns for C0/C1 — is read as zero
/// columns. Those characters are stripped by `path_cells` before they
/// can reach here, so a `None` means the strip filter has a hole; zero
/// keeps the arithmetic closest to correct until it is found.
pub fn display_width(cells: &[(char, CellKind)]) -> usize {
    use unicode_width::UnicodeWidthChar;
    cells.iter().map(|(c, _)| c.width().unwrap_or(0)).sum()
}

/// Left-truncate to `width` terminal columns, keeping the tail (where
/// basenames live) and marking the cut with a leading ellipsis.
///
/// Columns, not chars (ADR-005): a CJK or emoji glyph is two columns
/// wide, so dropping one cell can free two. The result is therefore
/// guaranteed to be *at most* `width` and may undershoot by one when a
/// wide glyph straddles the boundary. Undershooting keeps the meta
/// column aligned; overshooting would push it off the pane.
pub fn truncate_left(cells: &mut Vec<(char, CellKind)>, width: usize) {
    use unicode_width::UnicodeWidthChar;

    if width == 0 || display_width(cells) <= width {
        return;
    }
    // One column is spent on the ellipsis itself.
    let budget = width - 1;
    let mut kept = 0usize;
    let mut split = cells.len();
    for (i, (c, _)) in cells.iter().enumerate().rev() {
        let w = c.width().unwrap_or(0);
        if kept + w > budget {
            break;
        }
        kept += w;
        split = i;
    }
    cells.drain(..split);
    cells.insert(0, ('…', CellKind::Dir));
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

pub const SIGNAL_GLYPHS: [&str; 4] =
    ["   ", "\u{2581}  ", "\u{2581}\u{2584} ", "\u{2581}\u{2584}\u{2588}"];

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
        assert_eq!(display_width(&cells), 9);
    }

    #[test]
    fn width_counts_columns_not_chars() {
        // ADR-005. CJK ideographs are two columns each, so this path is
        // 1 + 4*2 + 1 + 3 = 13 columns across 9 chars. A char count
        // would say 9 and misplace everything to its right.
        let cells = path_cells("/日本語版/abc", "", &[]);
        assert_eq!(cells.len(), 9);
        assert_eq!(display_width(&cells), 13);

        // Combining marks add characters but no columns.
        let combining = path_cells("/e\u{301}", "", &[]);
        assert_eq!(combining.len(), 3);
        assert_eq!(display_width(&combining), 2);
    }

    #[test]
    fn truncation_budget_is_columns() {
        // Truncating a wide-glyph path to 9 columns must yield at most 9
        // COLUMNS. Asserting cells.len() here would pass even with the
        // pre-ADR-005 char-counting bug fully present, which is exactly
        // how that bug survived the original suite.
        let mut cells = path_cells("/日本語版/abc", "", &[]);
        truncate_left(&mut cells, 9);
        assert!(
            display_width(&cells) <= 9,
            "rendered {:?} at {} columns, budget 9",
            render(&cells),
            display_width(&cells)
        );
        // Tail preserved, cut marked.
        assert!(render(&cells).ends_with("/abc"));
        assert!(render(&cells).starts_with('…'));
    }

    #[test]
    fn truncation_undershoots_rather_than_overshoots_on_a_straddling_glyph() {
        // A two-column glyph straddling the boundary cannot be half
        // rendered. Dropping it undershoots by one column; keeping it
        // would push the meta column off the pane.
        let mut cells = path_cells("日日日", "", &[]); // 6 columns, 3 chars
        truncate_left(&mut cells, 4); // ellipsis (1) + budget 3 -> one glyph fits
        assert_eq!(display_width(&cells), 3);
        assert_eq!(render(&cells), "…日");
    }

    #[test]
    fn narrow_and_exact_widths_are_left_alone() {
        let mut cells = path_cells("/abc", "", &[]);
        truncate_left(&mut cells, 4); // exactly fits: untouched
        assert_eq!(render(&cells), "/abc");
        truncate_left(&mut cells, 0); // degenerate width: untouched
        assert_eq!(render(&cells), "/abc");
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
        let rendered: String = path_cells(corpus, "", &[]).iter().map(|(c, _)| *c).collect();
        assert_eq!(rendered, crate::ui::strip::clean(corpus));
    }
}
