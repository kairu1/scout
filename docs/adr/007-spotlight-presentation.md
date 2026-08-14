# ADR-007 — Spotlight Presentation

- **Status:** Accepted
- **Authored:** chief-of-staff
- **Date authored:** 2026-08-14
- **Reviewers:** none convened (see §Reviews)
- **Signed by commander:** 2026-08-14
- **Depends on:** ADR-005 (display width) — landed
- **Partially executed ahead of signature:** the preview pane was removed by direct commander order on 2026-08-14, before this ADR was drafted. Recorded in §Decision 1 rather than left implicit.

## Context

The picker works and reads badly. Here is a real frame, captured at 90
columns over a directory of four projects:

```
❯ █                                                                    5/5
──────────────────────────────────────────────────────────────────────────
▌ …001/-workspace/82e5f1ad-53a8-4f4a-9a78-fe6f86b0ca45/scratchpad/tree
  …ace/82e5f1ad-53a8-4f4a-9a78-fe6f86b0ca45/scratchpad/tree/plainname
  …82e5f1ad-53a8-4f4a-9a78-fe6f86b0ca45/scratchpad/tree/asciiproj-aaaa
  …e/82e5f1ad-53a8-4f4a-9a78-fe6f86b0ca45/scratchpad/tree/中文目録名前
  …ad-53a8-4f4a-9a78-fe6f86b0ca45/scratchpad/tree/日本語版プロジェクト
```

Roughly eighty columns per row carry a path, and the part the user is
choosing between is the last word of each. The other seventy characters
are identical on every row: shared ancestry, repeated five times,
truncated from the left so the *most* redundant part is what survives
longest. The eye has to travel to the end of each line to find the only
thing that differs.

This is what the commander meant by *not having to see loads of file
paths*. The row is not presenting a project; it is presenting a
filesystem string that happens to end in a project.

The stated inspiration is **macOS Spotlight (Tahoe)**: a surface that
appears, takes a query, offers a small number of well-formed results,
and gets out of the way. Its rows lead with a name. Location appears as
quiet secondary context, if at all. Nothing about a Spotlight result
asks you to read a path.

Three further observations about the current surface:

- **It is a full-screen application.** It takes the alternate screen and
  fills it, with the result list padded out by blank rows. Spotlight is
  a small thing that appears over your work.
- **It shows metadata the user cannot act on.** Every row carries a
  frecency signal meter and a visit count. Those are diagnostics of the
  ranking, shown to a user who has no decision to make about ranking.
- **It had a second panel.** The preview pane occupied 42% of the width
  whenever the terminal was at least 70 columns wide.

The last of these is already resolved: the commander ordered the preview
pane removed, and it is gone. The rest is what this ADR proposes.

## Decision

**Present a result as a *thing*, not as a path, on a single compact
surface.**

**1. One surface, no panels.** The preview pane is removed (done). No
second column is added in its place. If a result needs to be explained
by something next to it, the row is not doing its job.

**2. Name first; location only as far as it disambiguates.** A row leads
with the basename, styled as the primary element. Location follows as
dimmed secondary context, and carries *the shortest suffix of the parent
path that distinguishes this result from the others currently shown* —
not the full path, and not a fixed number of segments. Two results named
`api` in different repos show as `api — service-hub` and `api —
wraptious`; a result with a unique name shows its name and, at most, a
brief home-relative hint.

This is the substance of the ADR. Everything else follows from it.

**3. A short list, not a scrollable window.** The surface offers the
best few results (target: 8, adaptive to terminal height), not 200. If
the answer is not in the first few, the correct move is to type another
character, not to scroll. `RESULT_LIMIT = 200` remains the *ranking*
depth; it stops being the *display* depth.

**4. Compact and centred, not full-screen.** The surface occupies the
space it needs — query row, results, a single hint line — centred in the
terminal rather than stretched to its corners. The alternate screen is
still used (it is how the terminal is left clean on exit), but what is
drawn on it is a card, not an application.

**5. Ranking metadata leaves the row.** The frecency meter and the visit
count come off the result line. Ranking is expressed by *order*, which
is what order is for. The information remains available — `scout query`
and `scout doctor` can surface it — but it stops competing with the name
for the user's attention.

**6. Matched characters stay highlighted.** This is the one piece of
per-row ornament that earns its place: it tells the user *why* a result
matched, which directly informs the next keystroke.

**7. A `?` help overlay.** The only borrowing from lazygit that survives
the reframing. Today `Tab` is discoverable solely by reading the README.

**8. Three commander rulings, 2026-08-14.** The questions this draft
left open are answered.

**8a. Eight results.** Display depth is 8, adapting downward when the
terminal is too short. `RESULT_LIMIT = 200` remains ranking depth.

**8b. The query row keeps its counter.** `5/5` stays. It is ranking
metadata by decision 5's logic, but decision 5's target is *per-row*
ornament repeated once per line; the counter appears once, in the row
the user is already looking at while typing, and it is the only feedback
that a query is narrowing. One counter is information; eight meters are
decoration.

**8c. A kind marker earns its column.** One leading character
distinguishing a git repository from a plain directory from a file —
the terminal's answer to Spotlight's icons, and the fastest way to tell
"the project" from "a file inside the project".

Marker set, chosen for width rather than looks: `⑂` (U+2442) for a git
repository, `‣` (U+2023) for a directory, and a space for a file. All
are East Asian Width **Neutral**, so they occupy exactly one column in
every terminal. The obvious candidates — `◆`, `▪`, `·` — are
**Ambiguous**, which means one column or two depending on the reader's
locale, and an ornament that changes width defeats ADR-005 in the exact
place ADR-005 was written to protect.

Repository detection is a `.git` stat, performed **only for the rows
being displayed** — at most eight per frame. It must not enter the
indexer: it would add a stat per path to a walk with a 100k budget, to
answer a question about eight rows.

## Rationale

**The row's job is discrimination, not description.** A picker exists to
help a user choose between candidates. Screen space should therefore go
to what *differs* between candidates and be denied to what they share.
The current layout does exactly the opposite: it spends its width on
common ancestry and truncates from the left, so the shared prefix is the
last thing to be cut. Decision 2 is that principle applied literally —
show the discriminating suffix, computed against the results actually on
screen.

**Minimal disambiguation is honest in a way that a fixed rule is not.**
"Show the last two path segments" is easier to implement and wrong at
both ends: noisy when names are already unique, insufficient when three
candidates share both name and parent. Computing the context against the
current result set means the display adapts to the ambiguity that
actually exists, which is the only ambiguity the user is experiencing.

**Removing metadata is a feature, not an omission.** The frecency meter
was added to make ranking visible, and that was the right instinct at
the time — it proved the ranking worked. But it answers a question the
user asks once, during evaluation, and then never again. Ordering
already communicates rank; a meter that restates it is redundant
decoration on every row forever.

**Compactness serves the intent directly.** Commander's intent is *fast*
— and a surface that fills the screen reads as a context switch, while a
card reads as an interruption you are already recovering from. This is
the difference between "I opened a tool" and "I typed and it happened".

Against decade-longevity: this ADR removes code and adds no dependency.
Decisions 1, 3, 4 and 5 are net deletions. Only decisions 2 and 7 add
anything, and both are pure functions over data already held.

Against the ADR-005 dependency: name-first rows need *more* accurate
width measurement than path rows did, not less. The name is now the
aligned element and the thing whose truncation is visible, so a
character-counted width would misplace the very element this ADR makes
primary. ADR-005 landing first was the correct sequence.

## Alternatives considered

1. **Keep the full path, shorten it algorithmically** (`~/p/s/api`, in
   the style of a shell prompt). Rejected. It compresses the noise
   rather than removing it, and it makes the path *harder* to read while
   still asking the user to read one.
2. **Keep the preview pane but make it narrower.** Rejected by the
   commander's ruling, and by decision 1's reasoning: a preview is a
   second thing to look at, and the design goal is fewer things.
3. **Keep the frecency meter behind a flag.** Rejected. A flag is a
   decision deferred onto the user, and a second layout to maintain
   forever. `scout query` can carry the numbers for anyone who wants
   them.
4. **Adopt lazygit's multi-pane layout with numbered panel jumping.**
   Rejected — this was the roadmap's original reading of the commander's
   lazygit reference and it was wrong. The reference was to presentation
   quality, not to panel structure.
5. **Render inline (below the prompt) rather than on the alternate
   screen**, as some pickers do. Deferred, not rejected. It is closer
   still to the Spotlight feel, but it interacts with the print seam
   (ADR-003 §2) and with scrollback in ways that need their own
   analysis. Decision 4 gets most of the benefit at none of that risk.

## Consequences

**Binds `ui` sector (3rd Rifles).** `render.rs` gains the disambiguating
-context computation — a pure function over the visible result set, and
therefore unit-testable, which is where the correctness of this ADR
mostly lives. `ui/mod.rs` loses the preview branch (done), the meter and
visit-count spans, and gains the help overlay and centred layout.

**`RESULT_LIMIT` splits in two.** One constant for ranking depth, one
for display depth. They are different questions and have been
conflated.

**Test obligation.** The disambiguation function is the risky part and
must be tested against: unique names (no context), shared basename
(context appears), shared basename *and* parent (context deepens), and
a result set of one. A row that is correct in isolation and ambiguous in
a set is the failure this ADR is trying to prevent, so the tests must
operate on sets, never on single rows.

**The frecency signal meter and `SIGNAL_GLYPHS` become dead code** and
should be removed rather than left dormant — an unused visual grammar is
a trap for the next reader.

**An ADR-005 gap this ADR surfaced, and does not close.** Checking the
marker candidates revealed that almost the entire existing visual
grammar is East Asian Width **Ambiguous**: `▌` (selection bar), `▁▄█`
(the meter), `─` (hairline), and — the one that matters —
`…`, the truncation ellipsis. `unicode-width` 0.1 reports Ambiguous as
one column, which is right for most terminals and wrong for a reader
whose locale renders them wide. `truncate_left` spends exactly one
column on that ellipsis (ADR-005 §Decision 2), so on such a terminal the
truncation overshoots by one, which is the failure ADR-005 exists to
prevent, in ADR-005's own code.

Three of the four offenders leave with decisions 5 and 8. The ellipsis
stays and needs a ruling: either accept Ambiguous-as-narrow explicitly,
or replace `…` with `..`. This is filed as an ADR-005 revision, not
fixed here.

**Glyph width is now a design constraint, not a detail.** See §Decision
8. Every ornament glyph must be checked against East Asian Width before
it is used.

## Revision 2 — 2026-08-14, commander-directed

Three corrections from driving the built surface. Each supersedes part
of §Decision above; the original text stands so the change is legible.

**1. The surface is a framed panel, not a bare card (supersedes
§Decision 4's "occupies the space it needs").** Minimal was the wrong
target. A small unbordered block adrift in an empty alternate screen
reads as unfinished, and a one-line query row reads as a terminal that
happens to echo rather than as something you type into. The panel is now
bordered and titled, sized to the terminal with a margin (capped at
120x30), the search is a bordered field, and the results are a titled
pane. The border is what makes the surrounding space read as margin
instead of void — the same pixels, differently framed.

**2. The action launcher is a searchable column, not a popup
(supersedes §Decision 1's "no second column is added in its place").**
The commander's reasoning is decisive and I had it backwards: a popup
covers the very results you are choosing an action *for*, and it has
nowhere to put a filter row except over more of them. A column sits
beside them. It carries its own prompt and filters on name *and*
description, so a user who remembers "git" finds `status` without
knowing its name. §Decision 1's principle — a result should not need a
second panel to explain it — is intact: this panel explains the
*action*, not the result, and only exists while you are choosing one.

**3. Display depth follows the pane (supersedes §Decision 8a's flat
eight).** Eight was the right answer for a minimal card and the wrong
one for a framed pane, where it left a bordered box two-thirds empty.
The pane now shows what fits, capped at 16 — the cap is what keeps
§Decision 3's "not a scrollable window" true on a tall terminal.

**Defect fixed in the same pass.** `down` clamped the selection to
`results.len()` (up to 200 ranked) while the pane drew far fewer, so
past the last drawn row the highlight stuck to the bottom while the real
selection kept moving — the cursor appeared to vanish, and `enter` would
have run against a row the user could not see. Movement is now clamped
to drawn capacity, computed by the same `regions` function the renderer
uses, so the two cannot disagree. Verified on a 24-row terminal (11
results, 20 `down` presses, cursor rests on the last row) and on a
14-row terminal where only four rows fit.

## Revision 3 — 2026-08-14 — An editable search field

The query was append-only: characters went on the end and backspace took
them off the end. Correcting a typo three characters back meant deleting
everything after it, and a pasted string could not be edited at all.

The field now carries a caret. `left`/`right` move it, `home`/`end` jump
to either edge, `backspace` deletes before it and `delete` under it, and
typed or pasted characters insert at it. The caret is drawn where it
actually sits rather than always at the end.

Two implementation notes that are part of the decision. The caret is
counted in **characters, not bytes** — a byte index would split a
multi-byte character and panic on the next edit, and paths are exactly
where non-ASCII shows up (verified: editing inside `日本語` behaves and
does not panic). And the two halves of the query are passed through
`strip::clean` **separately**, because cleaning the whole string and
then slicing at the caret would misplace the cursor whenever a paste
contained a stripped character.

`up`/`down` remain the result list, so caret movement and selection
movement never contend for the same key.

## Reviews

_Appended by peer reviewers._

None convened. Signed on the commander's direct approval after the three
open questions in §Decision 8 were answered. As with ADR-005 and
ADR-006, the reasoning here has not been read adversarially by another
officer — and unlike those two, this ADR changes what every user sees on
every invocation. 3rd Rifles holds the sector under the AAR §5
promotion; council-security's standing review (AAR §5) covers the render
boundary, which decision 8c touches by introducing a filesystem stat
into the draw path.

## Revision history

- 2026-08-14 — drafted by chief-of-staff at commander's direction. Preview-pane removal already executed under separate order.
- 2026-08-14 — the three open questions answered by the commander and folded into §Decision 8: eight results, the counter stays, the kind marker ships. Marker glyphs selected for unambiguous width, which surfaced the ellipsis gap now filed against ADR-005.
- 2026-08-14 — **signed by the commander. Status: Accepted.** 3rd Rifles cleared to implement.
