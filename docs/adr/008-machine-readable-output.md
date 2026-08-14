# ADR-008 — Machine-Readable Output

- **Status:** Accepted
- **Authored:** chief-of-staff
- **Date authored:** 2026-08-14
- **Reviewers:** none convened (see §Reviews)
- **Signed by commander:** 2026-08-14 (v2 objective 4, AAR §6)

## Context

`scout query` prints bare paths, one per line, and `scout doctor` prints
a formatted human report. Neither can be consumed reliably by another
program, and both have the same shape of gap, which is why the AAR filed
them as one decision rather than two.

Three specific defects.

**Nothing but the path escapes.** `Ranked` carries `rank`, `s_now` and
`visits_total`; a caller that wants to sort, threshold, or show why a
result won cannot get at any of it. This is the same information ADR-007
§Decision 5 took *off* the picker row — correctly, because a human
reading a list does not need it — but taking it off the row is only
defensible if it remains reachable somewhere.

**Exit status carries no signal.** `scout query nonexistent` exits 0,
identically to a query that matched. A script cannot branch without
capturing stdout and testing for emptiness, which is exactly the
awkwardness an exit code exists to remove.

**A separator that the data can contain is not a format.** The obvious
answer is TSV, and it is unsafe here: the indexer refuses paths
containing NUL or newline (ADR-003 §6) but permits tabs. A tab in a
directory name would silently shift every field after it.

## Decision

**Two output modes on `scout query`, and one on `scout doctor`, using
delimiters the data cannot contain — and where it can, a field order
that makes it harmless.**

`scout query`:

- `--format paths` (default, unchanged): one path per line.
- `--format tsv`: `rank`, `visits`, `path`, tab-separated, **path last**.
- `--print0`: paths separated by NUL, for `xargs -0`.

`scout doctor`:

- `--format human` (default, unchanged).
- `--format tsv`: `level`, `section`, `name`, `detail`, **detail last**.

**Field order is the safety mechanism, not a style choice.** Paths may
contain tabs; ranks and counts cannot. Putting the unconstrained field
last means a consumer splits on the first N tabs and takes the remainder
whole — `cut -f3-`, `awk '{...}'` with a field limit, `split('\t', 3)` —
and an embedded tab cannot corrupt a field boundary, because there are
no boundaries after it. The same reasoning puts `detail` last in the
doctor rows.

**`scout query` exits 1 when nothing matched.** This changes existing
behaviour, deliberately and once, in the version where such changes
belong. A caller that wants the old behaviour writes `|| true`.

**No JSON, and therefore no `serde_json`.** ADR-002 leaves the crate off
the roster until a JSON mode ships, so the question is whether a JSON
mode is worth a roster slot. It is not: the data here is flat — three
scalars and a string — and a flat record with a safe field order is
completely served by a delimiter. JSON would buy nesting we do not have
and escaping we do not need, at the cost of a dependency, and would
still need this ADR's field-order reasoning for anyone using `cut`. If a
future feature produces genuinely nested output, this decision reopens.

## Rationale

The AAR filed this as a single objective because deciding it twice would
have produced two formats. `doctor` and `query` are read by the same
scripts in the same session; a user who learns `--format tsv` on one
should not discover a different spelling on the other.

Choosing safety-by-field-order over escaping is the substantive call.
The alternative — quoting or escaping paths that contain tabs — makes
every consumer implement an unescaper before it can use the first field,
which in practice means most consumers will not, and will work fine
until the day someone has a tab in a directory name. A format that is
correct only for well-behaved data is a format that fails in exactly the
situation the user is already confused by. Ordering fields so that the
dangerous one cannot damage anything is a property of the format rather
than a rule consumers must obey.

Against commander's intent: *composable* is the word this serves. The
print seam already lets scout compose into a shell; this lets it compose
into a program.

Against decade-longevity: no new dependency, no new file format to
version, and the default output of both commands is unchanged, so
nothing that works today stops working — except the exit code, which is
called out above and is the one deliberate break.

## Alternatives considered

1. **JSON via `serde_json`.** Rejected — see §Decision. Reconsider only
   when the output is genuinely nested.
2. **NUL-separated fields as well as records.** Rejected as
   over-engineering: NUL cannot appear in a path (ADR-003 refuses it at
   the index boundary), so it is the right *record* separator for
   `--print0`, but using it between fields makes the output unreadable
   to a human debugging their own pipeline for no gain over ordered TSV.
3. **A `--fields` selector.** Rejected for v2. It is a configuration
   surface on top of a format that has four fields, and every consumer
   would have to pin it defensively to be robust.
4. **Escaping tabs in paths.** Rejected — see §Rationale.
5. **Leave the exit code alone.** Rejected. An exit code that is
   constant carries no information, and the cost of the change is one
   documented line.

## Consequences

**Binds the CLI surface.** Two flags on `query`, one on `doctor`. Both
default to today's behaviour.

**Binds `doctor` (ADR-006).** Its `Report` already separates gathering
from rendering, so the TSV renderer is a second `render` and no
restructuring is needed — the structure ADR-006 chose for testability
paid for itself here.

**Exit-code change is a documented break.** README and `--help` say so.

**Test obligation.** The tab hazard is the thing to test: a path
containing a tab must still yield a parseable record, and the test must
assert that splitting on the first N tabs recovers the path *whole*.
Asserting only that the output "contains the path" would pass while the
format was broken.

## Revision 2026-08-14 — Terminal escapes

A path may contain ESC (the index refuses only NUL and newline), so the
default human-facing output could rewrite a terminal.

Output is stripped **only when stdout is a terminal**. Piped output stays
byte-exact in every format — `paths`, `tsv` and `--print0` alike —
because this ADR's whole purpose is that a consumer can act on what it
reads, and a stripped path does not exist on disk. Guarded by
`piped_output_is_byte_exact_in_every_format`.

## Reviews

_Appended by peer reviewers._

None convened. Commander-directed as v2 objective 4.

## Revision history

- 2026-08-14 — drafted and signed under the v2 opening objectives.
