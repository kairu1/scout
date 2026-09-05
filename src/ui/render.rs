//! Pure render helpers, kept ratatui-free so the visual grammar is
//! unit-testable: path cell classification (dim dir / bold basename /
//! accent match), home shortening, terminal-column measurement and
//! truncation, and the name/context split rows are built from.

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

/// Terminal columns these cells occupy. The single source of
/// truth for every layout calculation downstream.
///
/// `None` — which `unicode-width` returns for C0/C1 — is read as zero
/// columns. Those characters are stripped by `strip::keep` before they
/// can reach here, so a `None` means the strip filter has a hole; zero
/// keeps the arithmetic closest to correct until it is found.
pub fn display_width(cells: &[(char, CellKind)]) -> usize {
    use unicode_width::UnicodeWidthChar;
    cells.iter().map(|(c, _)| c.width().unwrap_or(0)).sum()
}

/// Right-truncate to `width` terminal columns, keeping the HEAD and
/// marking the cut. Names are truncated from the end because a name's
/// distinguishing part is usually its start — the opposite of a path,
/// where the tail carries the basename.
pub fn truncate_right(cells: &mut Vec<(char, CellKind)>, width: usize) {
    use unicode_width::UnicodeWidthChar;

    if width == 0 {
        cells.clear();
        return;
    }
    if display_width(cells) <= width {
        return;
    }
    let marker: Vec<char> = super::glyph::ELLIPSIS.chars().collect();
    let marker_width: usize = marker.iter().map(|c| c.width().unwrap_or(0)).sum();
    if width <= marker_width {
        cells.clear();
        return;
    }
    let budget = width - marker_width;
    let mut kept = 0usize;
    let mut split = 0usize;
    for (i, (c, _)) in cells.iter().enumerate() {
        let w = c.width().unwrap_or(0);
        if kept + w > budget {
            break;
        }
        kept += w;
        split = i + 1;
    }
    cells.truncate(split);
    for c in marker {
        cells.push((c, CellKind::Dir));
    }
}

/// Left-truncate to `width` terminal columns, keeping the tail (where
/// basenames live) and marking the cut with a leading ellipsis.
///
/// Columns, not chars: a CJK or emoji glyph is two columns
/// wide, so dropping one cell can free two. The result is therefore
/// guaranteed to be *at most* `width` and may undershoot by one when a
/// wide glyph straddles the boundary. Undershooting keeps the meta
/// column aligned; overshooting would push it off the pane.
pub fn truncate_left(cells: &mut Vec<(char, CellKind)>, width: usize) {
    use unicode_width::UnicodeWidthChar;

    if width == 0 || display_width(cells) <= width {
        return;
    }
    // The marker's own columns come out of the budget. Its width is a
    // known constant because `glyph` forbids Ambiguous-width ornaments
    // (see glyph::ornaments_are_width_unambiguous); with `…` this was a
    // guess that came out wrong on a CJK-locale terminal.
    let marker: Vec<char> = super::glyph::ELLIPSIS.chars().collect();
    let marker_width: usize = marker.iter().map(|c| c.width().unwrap_or(0)).sum();
    if width <= marker_width {
        cells.clear();
        return;
    }
    let budget = width - marker_width;
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
    for c in marker.into_iter().rev() {
        cells.insert(0, (c, CellKind::Dir));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(cells: &[(char, CellKind)]) -> String {
        cells.iter().map(|(c, _)| *c).collect()
    }

    fn text(cells: &[(char, CellKind)]) -> String {
        cells.iter().map(|(c, _)| *c).collect()
    }

    /// Cells for a plain string, for the width/truncation tests.
    fn make_cells(path: &str) -> Vec<(char, CellKind)> {
        path.chars().map(|c| (c, CellKind::Dir)).collect()
    }

    #[test]
    fn control_chars_strip_without_breaking_match_alignment() {
        // ESC at char index 5; match on "abc" at indices 6..9.
        let path = "/tmp/\u{1b}abc";
        let rows = rows(&[path], &[&[6, 7, 8]], "");
        assert_eq!(render(&rows[0].name), "abc");
        let matched: String =
            rows[0].name.iter().filter(|(_, k)| *k == CellKind::Match).map(|(c, _)| *c).collect();
        assert_eq!(matched, "abc");
    }

    #[test]
    fn truncation_keeps_tail() {
        let mut cells = make_cells("/very/long/dir/base");
        truncate_left(&mut cells, 9);
        assert_eq!(render(&cells), "..ir/base");
        assert_eq!(cells.len(), 9);
        assert_eq!(display_width(&cells), 9);
    }

    #[test]
    fn width_counts_columns_not_chars() {
        // CJK ideographs are two columns each, so this path is
        // 1 + 4*2 + 1 + 3 = 13 columns across 9 chars. A char count
        // would say 9 and misplace everything to its right.
        let cells = make_cells("/日本語版/abc");
        assert_eq!(cells.len(), 9);
        assert_eq!(display_width(&cells), 13);

        // Combining marks add characters but no columns.
        let combining = make_cells("/e\u{301}");
        assert_eq!(combining.len(), 3);
        assert_eq!(display_width(&combining), 2);
    }

    #[test]
    fn truncation_budget_is_columns() {
        // Truncating a wide-glyph path to 9 columns must yield at most 9
        // COLUMNS. Asserting cells.len() here would pass even with the
        // old char-counting bug fully present, which is exactly
        // how that bug survived the original suite.
        let mut cells = make_cells("/日本語版/abc");
        truncate_left(&mut cells, 9);
        assert!(
            display_width(&cells) <= 9,
            "rendered {:?} at {} columns, budget 9",
            render(&cells),
            display_width(&cells)
        );
        // Tail preserved, cut marked.
        assert!(render(&cells).ends_with("/abc"));
        assert!(render(&cells).starts_with(crate::ui::glyph::ELLIPSIS));
    }

    #[test]
    fn truncation_undershoots_rather_than_overshoots_on_a_straddling_glyph() {
        // A two-column glyph straddling the boundary cannot be half
        // rendered. Dropping it undershoots by one column; keeping it
        // would push the meta column off the pane.
        let mut cells = make_cells("日日日"); // 6 columns, 3 chars
        truncate_left(&mut cells, 4); // ellipsis (1) + budget 3 -> one glyph fits
                                      // marker (2 cols) + one wide glyph (2) would be 4; budget is 4.
        assert_eq!(display_width(&cells), 4);
        assert_eq!(render(&cells), "..日");
    }

    #[test]
    fn narrow_and_exact_widths_are_left_alone() {
        let mut cells = make_cells("/abc");
        truncate_left(&mut cells, 4); // exactly fits: untouched
        assert_eq!(render(&cells), "/abc");
        truncate_left(&mut cells, 0); // degenerate width: untouched
        assert_eq!(render(&cells), "/abc");
    }

    #[test]
    fn unique_names_carry_no_context() {
        let paths = ["/home/u/projects/scout", "/home/u/projects/storefront"];
        assert_eq!(context_depths(&paths), vec![0, 0]);
        let rows = rows(&paths, &[&[], &[]], "/home/u");
        assert_eq!(text(&rows[0].name), "scout");
        assert!(rows[0].context.is_empty(), "unique name should show no path");
    }

    #[test]
    fn shared_basename_grows_context_until_it_distinguishes() {
        let paths = ["/home/u/billing/api", "/home/u/storefront/api"];
        assert_eq!(context_depths(&paths), vec![1, 1]);
        let rows = rows(&paths, &[&[], &[]], "/home/u");
        assert_eq!(text(&rows[0].name), "api");
        assert_eq!(text(&rows[0].context), "billing");
        assert_eq!(text(&rows[1].context), "storefront");
    }

    #[test]
    fn context_deepens_when_the_parent_also_collides() {
        // Same name AND same parent: one segment is not enough.
        let paths = ["/home/u/alpha/src/mod.rs", "/home/u/beta/src/mod.rs"];
        assert_eq!(context_depths(&paths), vec![2, 2]);
        let rows = rows(&paths, &[&[], &[]], "/home/u");
        assert_eq!(text(&rows[0].context), "alpha/src");
        assert_eq!(text(&rows[1].context), "beta/src");
    }

    #[test]
    fn depth_is_per_row_not_per_set() {
        // Only the colliding rows pay for context; the unique one does not.
        let paths = ["/home/u/a/api", "/home/u/b/api", "/home/u/c/unique"];
        assert_eq!(context_depths(&paths), vec![1, 1, 0]);
    }

    #[test]
    fn a_single_result_needs_nothing() {
        let paths = ["/home/u/projects/scout"];
        assert_eq!(context_depths(&paths), vec![0]);
        assert!(rows(&paths, &[&[]], "/home/u")[0].context.is_empty());
    }

    #[test]
    fn matches_are_classified_in_name_and_in_context() {
        // The highlight says WHY a row matched, and a query
        // can match on location alone — so context must carry hits too.
        let paths = ["/home/u/storefront/api", "/home/u/billing/api"];
        // Char positions 8..12 land inside the parent segment. Derived
        // from the fixture rather than written out, so renaming the
        // fixture cannot leave the expectation pointing at old text.
        let hits: Vec<u32> = (8..12).collect();
        let expected: String = paths[0].chars().skip(8).take(4).collect();
        let rows = rows(&paths, &[&hits, &[]], "/home/u");
        let matched: String = rows[0]
            .context
            .iter()
            .filter(|(_, k)| *k == CellKind::Match)
            .map(|(c, _)| *c)
            .collect();
        assert_eq!(matched, expected);
    }

    #[test]
    fn a_full_parent_context_collapses_home() {
        // Depth reaching the whole parent path is the one place a long
        // location can still appear; ~ keeps it from being a full path.
        // Collides at every shallower depth, so depth reaches the whole
        // parent — the only case where a long location can still appear.
        let paths = ["/home/u/api", "/x/home/u/api"];
        let rows = rows(&paths, &[&[], &[]], "/home/u");
        assert_eq!(text(&rows[0].context), "~");
        assert_eq!(text(&rows[1].context), "x/home/u");
    }

    #[test]
    fn wide_glyph_names_measure_in_columns() {
        // Column width is load-bearing here: the name is the aligned
        // element, so its width must be columns, not chars.
        let paths = ["/p/日本語", "/q/日本語"];
        let rows = rows(&paths, &[&[], &[]], "");
        assert_eq!(rows[0].name.len(), 3);
        assert_eq!(display_width(&rows[0].name), 6);
        assert_eq!(text(&rows[0].context), "p");
    }

    #[test]
    fn rows_strip_matches_strip_clean() {
        // `rows` now CALLS strip::keep rather than repeating it, so this
        // is a regression guard rather than a drift guard. The corpus
        // carries a bidi override, a zero-width space and a BOM
        // alongside the C0/C1 cases: when the rules were duplicated, a
        // corpus without them let the guard pass while the two
        // implementations genuinely disagreed.
        let corpus =
            "plain\tpath\x00\x1b\x07\u{85}\u{9f}ba\u{202E}s\u{200B}e\u{feff}\u{1b}]0;x\u{7}z";
        let path = format!("/dir/{corpus}");
        let rendered: String = rows(&[&path], &[&[]], "")[0].name.iter().map(|(c, _)| *c).collect();
        assert_eq!(rendered, crate::ui::strip::clean(corpus));
    }
}

// ---------------------------------------------------------------------
// Name-first rows
// ---------------------------------------------------------------------

/// One displayed result, split into the two things a Spotlight row shows:
/// the name, and just enough location to tell it from its neighbours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Final path component, with matcher hits classified.
    pub name: Vec<(char, CellKind)>,
    /// Shortest parent-path suffix that distinguishes this row from the
    /// others on screen. Empty when the name is already unique.
    pub context: Vec<(char, CellKind)>,
}

/// Half-open char-index range of one path segment.
type Span = (usize, usize);
/// A path split into its parent segments and its final component.
type Split = (Vec<Span>, Span);

/// Split a path into its parent segments and its final component, by
/// char index. Trailing slashes are ignored; a root path has no parent.
fn segments(path: &str) -> Split {
    let chars: Vec<char> = path.chars().collect();
    let mut bounds = Vec::new();
    let mut start = 0usize;
    for (i, &c) in chars.iter().enumerate() {
        if c == '/' {
            if i > start {
                bounds.push((start, i));
            }
            start = i + 1;
        }
    }
    if start < chars.len() {
        bounds.push((start, chars.len()));
    }
    match bounds.pop() {
        Some(base) => (bounds, base),
        None => (Vec::new(), (0, 0)),
    }
}

/// How many trailing parent segments each row needs in order to be
/// distinguishable from every other row sharing its name.
///
/// Computed against the set actually on screen, not by a fixed rule: a
/// unique name needs nothing, and three candidates sharing both name and
/// parent need two segments. A fixed "show the last N" is noisy at one
/// end and insufficient at the other, and the user is only ever
/// experiencing the ambiguity that is actually in front of them.
pub fn context_depths(paths: &[&str]) -> Vec<usize> {
    // Key on the STRIPPED text. Uniqueness computed over raw bytes is
    // uniqueness the user cannot see: two paths differing only by a
    // zero-width or bidi character are "distinct" here and render
    // byte-identical, so both would show no context at all. The strip
    // filter would then have manufactured the collision this function
    // has already ruled out.
    let display: Vec<String> = paths.iter().map(|p| super::strip::clean(p)).collect();
    let paths: Vec<&str> = display.iter().map(|s| s.as_str()).collect();
    let paths = paths.as_slice();
    let parsed: Vec<Split> = paths.iter().map(|p| segments(p)).collect();
    let chars: Vec<Vec<char>> = paths.iter().map(|p| p.chars().collect()).collect();

    let slice = |i: usize, (a, b): Span| -> String { chars[i][a..b].iter().collect() };
    // Key at depth k: the name plus its k nearest parent segments.
    let key = |i: usize, k: usize| -> String {
        let (parents, base) = &parsed[i];
        let take = k.min(parents.len());
        let mut parts: Vec<String> =
            parents[parents.len() - take..].iter().map(|&b| slice(i, b)).collect();
        parts.push(slice(i, *base));
        parts.join("/")
    };

    let max_depth = parsed.iter().map(|(p, _)| p.len()).max().unwrap_or(0);
    let mut depths = vec![0usize; paths.len()];

    for i in 0..paths.len() {
        for k in 0..=max_depth {
            let mine = key(i, k);
            let collides = (0..paths.len()).any(|j| j != i && key(j, k) == mine);
            if !collides {
                depths[i] = k;
                break;
            }
            // Ran out of parents and still colliding: identical paths.
            if k == max_depth {
                depths[i] = parsed[i].0.len();
            }
        }
    }
    depths
}

/// Build the displayable rows for a result set.
///
/// `match_indices` are char positions in the ORIGINAL path, as the
/// matcher reports them, so hits are classified in both the name and the
/// context: the highlight tells the user *why* a row matched,
/// and a query can match on location alone.
pub fn rows(paths: &[&str], match_indices: &[&[u32]], home: &str) -> Vec<Row> {
    let depths = context_depths(paths);
    paths
        .iter()
        .enumerate()
        .map(|(i, path)| {
            let chars: Vec<char> = path.chars().collect();
            let (parents, base) = segments(path);
            let hits: &[u32] = match_indices.get(i).copied().unwrap_or(&[]);

            // The strip rule lives in `strip::keep` and is called, not
            // copied. It used to be duplicated here and kept honest by a
            // parity test — which passed for weeks against a corpus that
            // predated the characters the rules had come to disagree on.
            let cell = |idx: usize, plain: CellKind| -> Option<(char, CellKind)> {
                let c = super::strip::keep(chars[idx])?;
                Some((c, if hits.contains(&(idx as u32)) { CellKind::Match } else { plain }))
            };

            let name: Vec<(char, CellKind)> =
                (base.0..base.1).filter_map(|i| cell(i, CellKind::Base)).collect();

            let depth = depths[i].min(parents.len());
            let context = if depth == 0 {
                Vec::new()
            } else {
                let first = parents[parents.len() - depth];
                let last = parents[parents.len() - 1];
                let (from, to) = (first.0, last.1);
                // A context that spans the whole parent path collapses
                // $HOME to `~`, the one place a full path can still show.
                // The test is "took every parent segment", NOT `from == 0`:
                // an absolute path's first segment starts at index 1,
                // after the leading slash, so `from` is never 0.
                let home_chars = home.chars().count();
                let collapse = depth >= parents.len()
                    && !home.is_empty()
                    && path.starts_with(home)
                    && chars.get(home_chars).is_some_and(|c| *c == '/');
                let mut out: Vec<(char, CellKind)> = Vec::new();
                if collapse {
                    out.push(('~', CellKind::Dir));
                    out.extend((home_chars..to).filter_map(|i| cell(i, CellKind::Dir)));
                } else {
                    out.extend((from..to).filter_map(|i| cell(i, CellKind::Dir)));
                }
                out
            };

            Row { name, context }
        })
        .collect()
}
