# ADR-005 — Display Width

- **Status:** Accepted
- **Authored:** chief-of-staff
- **Date authored:** 2026-08-14
- **Reviewers:** none (single-crate admission of a crate already in the graph; commander-directed)
- **Signed by commander:** 2026-08-14 (direct order, "do unicode-width")
- **Extends:** ADR-002 (dependency roster) by one slot
- **Supersedes:** the `unicode-width` deferral recorded in the KNOWN LIMITATION comment at `src/ui/render.rs` and in the HANDOFF entry of 2026-07-06. ADR-002 never named `unicode-width` in either its roster or its Rejected table — the crate was ruled out by the roster's closed-set discipline rather than by a considered refusal, which is why admitting it needs an ADR at all.

## Context

Every width calculation in the picker counts `char`s and assumes each
one occupies one terminal column. That assumption holds for ASCII and
fails for everything else:

- **East Asian Wide and Fullwidth** characters (CJK ideographs, kana,
  fullwidth Latin) occupy two columns.
- **Combining marks** and zero-width characters occupy none.
- **Emoji** in the Wide category occupy two.

Three call sites depend on the assumption. `render::truncate_left`
left-truncates to a `width` it treats as a column count while draining a
per-`char` vector. `ui::result_line` computes `filled = cells.len()` and
pads with `path_width - filled` spaces to reach the meta column. The
signal meter and visit count are positioned by that padding.

The failure is therefore not confined to the offending row. A single
path containing CJK characters consumes more columns than the padding
arithmetic believes, the meter is pushed right past the pane edge, and —
because the pad is computed per row — **every row's meter stops lining
up with every other row's**. The column stops reading as a column.

Shadow-review recorded this on 2026-07-06 as a known limitation and
deferred it to v2 on the grounds that the fix required a crate off the
ADR-002 roster. That reasoning was sound at the time and is now
obsolete, for two reasons.

**First: the crate is already here.** `unicode-width` is in the
dependency graph today, pulled in twice by `ratatui` — directly, and
again through `unicode-truncate`. The transitive ceiling (ADR-002, 105
of 120) does not move by one crate, because there is no new crate. What
was deferred as a supply-chain cost has no supply-chain cost. The
deferral was priced against a bill that was never going to arrive.

**Second: the commander has reclassified the defect.** The v1 UI is a
single-column list, where the misalignment is cosmetic — which is what
the deferral assumed. The intended v2 direction is a Spotlight-style
presentation. Any layout that positions anything to the right of a
variable-width path is structurally dependent on knowing that path's
true column count. Display width stops being a polish item and becomes
load-bearing for the UI engagement that follows it. It must therefore
land *before* that engagement opens, not alongside it.

## Decision

**`unicode-width` joins the ADR-002 roster as a direct dependency,
pinned to `0.1` to unify with the version `ratatui` already resolves.**

The pin is not stylistic. `deny.toml` sets `multiple-versions = "deny"`
with an explicit skip list; requesting a major that resolves separately
from ratatui's would introduce a duplicate and fail `cargo deny check`.
The version requirement is therefore a constraint imposed by our own
supply-chain gate, and it must be revisited in lockstep with any future
ratatui bump — that bump is the moment `unicode-width` 0.2 becomes
available to us, and not before.

**Display width, not character count, is the unit of every layout
calculation that reaches the terminal.** Concretely:

1. `render::display_width(&[(char, CellKind)]) -> usize` sums
   `UnicodeWidthChar::width(c).unwrap_or(0)` over the cells, and is the
   single source of truth for "how many columns will this occupy".
2. `render::truncate_left` truncates to a **column** budget. It drops
   leading cells until the remainder plus the one-column ellipsis fits.
   Because a dropped cell may be two columns wide, the result is
   guaranteed to be *at most* the budget, never exactly it — undershoot
   by one column is correct and overshoot is not.
3. `ui::result_line` pads by `path_width - display_width(&cells)`.

`unwrap_or(0)` is the deliberate reading for the `None` case.
`UnicodeWidthChar::width` returns `None` for C0 and C1 control
characters, which ADR-003 §6 has already stripped before any cell
reaches this code. A control character surviving to the width
calculation is a bug in the strip filter, and treating it as zero
columns is the behaviour that keeps the layout closest to correct while
that bug is found — the alternative, treating unknown input as one
column, would bake the strip filter's failure into the arithmetic.

## Consequences

**Binds `ui` sector (3rd Rifles).** `render.rs` and `ui/mod.rs` as
above. The KNOWN LIMITATION comment at `render.rs` is removed rather
than amended; it documented a deferral that no longer exists.

**Binds `ops` sector (Pioneers).** No CI change. The existing transitive
ceiling job already covers the (unchanged) count.

**Test obligation.** The width contract is only meaningful if the tests
speak in columns. A wide-character truncation test must assert on the
resulting *column* count, not the character count — asserting
`cells.len()` would pass while the bug is fully present, which is
precisely how the defect survived the original suite.

**Not closed by this ADR — bidirectional and zero-width spoofing.**
`unicode-width` correctly reports zero columns for zero-width and
format characters, so the arithmetic in this ADR is right in their
presence. But a path containing U+202E RIGHT-TO-LEFT OVERRIDE displays
its components in an order that does not match the path an action will
receive, which is a display-integrity concern of exactly the kind
ADR-003 §6 exists to address for C0/C1. That is a threat-model question,
not a layout question, and it belongs to a revision of ADR-003 rather
than to this ADR. Recorded here because the width work is what surfaced
it, and because the parity guard between `strip::clean` and
`path_cells` is the seam any such fix must pass through.

## Alternatives considered

1. **Hand-roll the width tables.** Rejected. The East Asian Width
   property is a large Unicode table that changes with each Unicode
   release; hand-rolling it means owning a data-maintenance obligation
   forever to avoid a dependency we already ship.
2. **Keep counting characters and accept the misalignment.** Rejected by
   the commander's reclassification. Defensible for a single-column
   list; indefensible for the UI direction that follows.
3. **Take `unicode-segmentation` as well and count grapheme clusters.**
   Deferred, not rejected. Grapheme clustering is the correct unit for
   *cursor movement and editing*; column width is the correct unit for
   *layout*, and this ADR is about layout. A future editable-query or
   selection feature reopens it. Unlike `unicode-width`, that crate is
   not already in the graph and would cost a real ceiling slot.
4. **Wait and take it with the ratatui bump.** Rejected on sequencing.
   The bump is a larger, riskier change; this is a self-contained
   correctness fix whose cost is already sunk.

## Revision history

- 2026-08-14 — drafted by chief-of-staff under direct commander order; admitted `unicode-width` to the roster and made display width the layout unit. Supersedes the ADR-002 deferral.
