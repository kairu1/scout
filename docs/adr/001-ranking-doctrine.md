# ADR-001 — Ranking Doctrine

- **Status:** Accepted
- **Authored:** council-architect
- **Date authored:** 2026-04-24
- **Reviewers:** council-intel, council-quartermaster, council-security, council-surgeon
- **Signed by commander:** 2026-04-24

## Context

Ranking is the load-bearing user experience. Every keystroke resolves to an ordered list drawn from the index, and the ordering decides whether SCOUT feels like a launcher or a lottery. Four position papers constrain the decision:

- Architect (`positions/council-architect.md`) argued continuous exponential decay over zoxide-style tiered frecency, a generation-counter pipeline with three discard layers, and `nucleo-matcher` as the scoring primitive.
- Intel (`positions/council-intel.md`) placed the defensible quadrant at *indexed-path frecency + portable TOML actions + editor-/OS-agnostic CLI* and named `nucleo` as the matcher to adopt.
- Quartermaster (`positions/council-quartermaster.md`) blessed `nucleo-matcher` and `rusqlite` bundled; ruled `fuzzy-matcher` a SWAP.
- Surgeon (`positions/council-surgeon.md`) required partial-state tolerance via a `scan_generation` column, tombstones, and panic-to-log discipline inside search workers.
- Security (`positions/council-security.md`) set canonicalisation, control-byte stripping, and no-silent-empty-placeholder rules on every path that reaches render or action.

Commander's Wave 2 directives (HANDOFF 2026-04-23 23:23) fix five calls: adopt `nucleo-matcher` from v1; name p99 budgets; define empty-index behaviour as banner+hint, not error; forbid full WAL checkpoints during in-flight queries; scope "candidate" to all indexed paths, not project-filtered. This ADR resolves all five.

## Decision

We will rank by a single continuous-decay frecency scalar `S` (half-life 7 days), blended with a `nucleo-matcher` fuzzy score under fixed-scale soft-normalisation. Visits are credited only when the user executes an action on a candidate. The candidate set is every non-tombstoned row in the index. Ranking degrades explicitly, never silently: empty index renders a banner and hint; a partial first scan serves whatever the last complete `scan_generation` holds and banners the staleness.

## Rationale

### Frecency formula (numerator, denominator, decay curve)

Per path, two stored fields: `S: REAL` and `last_update: INTEGER` (unix seconds). No per-visit history is retained — the scalar is a collapsed sum of infinitely many decayed visits.

**Decay constant.** Half-life `t½ = 7 days`. `λ = ln 2 / t½ = ln 2 / 604_800 ≈ 1.1453 × 10⁻⁶ per second`. One-week-old visit weighs `0.5`; thirty-day weighs `≈ 0.051`; ninety-day weighs `≈ 1.4 × 10⁻⁴`. Long tail vanishes without a sweep.

**Increment (on action execution only — see below):**

```
Δt       = now - last_update          // seconds
S_new    = S_old · exp(-λ · Δt) + 1
last_new = now
```

Stored in one row update inside a `BEGIN IMMEDIATE` transaction.

**Read-time score** (pure function, no write):

```
S_now(row) = S_row · exp(-λ · (now - last_update_row))
```

This is the frecency numerator. There is no denominator — the formula is an additive decayed count, not a ratio. Normalisation for blending with match score is a separate step (below); it does not belong in the stored value.

**Bounds.** `S` is clamped to `[0, 10_000]` at write time (`S_new = min(S_new, 10_000)`). The cap keeps f64 precision linear over a decade of daily hits and denies runaway inflation from a pathological loop. The floor matters only if a future tuning move shifts the decay constant — it does not.

**Rate limit.** An increment is accepted at most once per `(path, 10s)` window. Double-Enter or action chains that re-trigger the same candidate do not compound. Enforced in the action executor, not the DB.

### Visit credit — action execution only, not search results

A visit is credited when and only when an `[[action]]` is executed against a candidate — the moment `execvp` is called on a `spawn` step, or stdout is written by a `print` step. Search result appearance, highlighting, keyboard navigation, and action-menu opening do not credit visits. Rationale:

1. **Intent matters.** zoxide/autojump credit a visit when the user *goes there*. Our equivalent of "goes there" is `action.spawn` or `action.print` completing; scrolling through results is browsing, not commitment.
2. **Feedback loop hygiene.** Crediting on appearance makes the ranker converge on whatever the ranker already showed. The fixed point is degenerate.
3. **Adversarial cheap-shot denial.** A rogue action that emits thousands of fake "results" cannot poison frecency, because results are not a credit event.

Implementation: the action executor calls `index::record_visit(candidate_id)` after *any* step of the action succeeds (first success wins; later steps within the same action do not re-credit). Action failure with `on_failure = "abort"` credits nothing. `on_failure = "continue"` credits if any step succeeded.

### Ranking interaction with fuzzy-match

The matcher is `nucleo-matcher` (Quartermaster/Intel-blessed, commander-directed). Its `Matcher::fuzzy_match` returns `Option<u32>`: `None` eliminates the candidate; `Some(m)` is the raw match score (higher is better; scale depends on query length and path content).

Two ranking modes:

**Zero-query (empty search buffer).** Rank by `S_now` descending. No matcher is called. Candidate set is every non-tombstoned row in the current `scan_generation`.

**Non-empty query.** For each candidate `c` that matches (`nucleo` returns `Some(m_c)`), compute a blended score:

```
match_norm(c)    = tanh(m_c / K_match)                   // K_match = 100
frecency_norm(c) = tanh(S_now(c) / K_frec)               // K_frec  = 10
rank_score(c)    = 0.6 · match_norm(c) + 0.4 · frecency_norm(c)
```

Both norms are bounded in `[0, 1)` and monotonic. `tanh` is chosen over min-max because it is **streaming-stable**: a new partial arriving with a higher `m` or `S_now` does not reshuffle the already-rendered top-K. Min-max normalisation causes rank jitter mid-stream; we refuse it. The calibration constants are:

- `K_match = 100` — empirically, nucleo's high-relevance matches on path-typical strings sit in the 60–200 range; `tanh(100/100) ≈ 0.76` places a "good" match in the upper band without saturating.
- `K_frec = 10` — equivalent to roughly two weeks of daily visits at the 7-day half-life; `tanh(10/10) ≈ 0.76` gives parity with a "good" match. Daily drivers dominate; one-off visitors do not.

The weights `(0.6, 0.4)` favour relevance over habit: a precisely typed query outranks a vaguely remembered habit. The constants are versioned in code (`ranking::BLEND`) behind a single-point-of-change comment; re-tuning is a code change, not a DB migration.

### Tie-breakers

`rank_score` ties are broken in this order, each step total on the relation it imposes:

1. **Higher `visits_total`** (a monotonically non-decreasing `INTEGER` column incremented alongside `S`, never decayed). Captures "this has been useful repeatedly" vs "this is just freshly hot".
2. **Shorter `path` byte length.** Shorter paths are usually higher-level directories; the user's target when two scores match is almost always the ancestor.
3. **Lexicographic byte ordering of canonical path.** Deterministic, stable across locale changes.
4. **`rowid` ascending.** Ultimate fallback; ties to step 3 can only occur if two rows share a canonical path, which the `UNIQUE` index forbids — this step exists so the sort is provably total.

### Degradation — empty and partial index

The index carries a monotonic `scan_generation: INTEGER` bumped only on a **completed** walk. Candidate queries filter `WHERE scan_generation = current_generation AND tombstoned_at IS NULL`.

**Empty index** (`current_generation = 0`, row count = 0):

- No error. Exit code 0 if SCOUT was asked to search.
- UI renders a banner: `no paths indexed — run 'scout index <path>' to populate`.
- Typing in the query bar has no effect on ranking (there is nothing to rank); the bar remains interactive so the user can quit cleanly.

**Partial first scan** (`current_generation = 0`, scan in progress):

- Serve nothing from the in-progress generation. Banner: `indexing in progress (N paths so far) — results will appear when the first scan completes`.
- This is deliberate: a half-walked scan has bias toward whichever subtree the walker started in, and ranking bias on a user's first impression is worse than a short wait.

**Partial re-scan** (`current_generation ≥ 1`, re-scan in progress):

- Continue serving `current_generation` normally. Banner (non-blocking): `re-indexing… (stale since YYYY-MM-DD)`.
- `scan_generation` advances atomically on completion; search flips to the new generation between queries, never mid-query.

**Corrupt DB.** Surgeon owns recovery (`positions/council-surgeon.md` §4). Ranking defers: if `integrity_check` fails, the DB is renamed and rebuilt; the empty-index banner then applies until the rebuild scan completes.

### Index-writer pacing vs active search

The writer (indexer) and readers (search workers) share the same SQLite file in WAL mode. Writer behaviour is constrained as follows:

1. **Batch size.** Inserts commit in batches of `1000` rows per `BEGIN IMMEDIATE` (Surgeon §2). Smaller batches thrash the WAL; larger ones hold the writer lock long enough to starve a search partial.
2. **Checkpoint gating.** `PRAGMA wal_autocheckpoint = 0` (disabled). The writer issues `PRAGMA wal_checkpoint(PASSIVE)` explicitly, and only when *no query has been registered in the last 500 ms*. A `PASSIVE` checkpoint does not block readers; we choose it anyway to stay inside the commander's "no full checkpoint during in-flight queries" directive literally. `TRUNCATE` / `RESTART` / `FULL` checkpoints are reserved for shutdown and post-rebuild.
3. **Query-active signal.** The UI maintains `QUERY_ACTIVE: AtomicU64` holding the wall-clock ms of the most recent `GEN` bump. The writer reads this before any checkpoint or large commit; if `now - QUERY_ACTIVE < 500 ms`, it yields (sleeps 10 ms, re-checks, up to 2 s before forcing).
4. **No `synchronous=FULL`.** `synchronous=NORMAL` is the configured setting (Surgeon §2). `FULL` stalls commits; `NORMAL` is durable across OS crashes in WAL mode, which is the regime we care about.
5. **Visit updates bypass batching.** A `record_visit` is a single-row update inside its own `BEGIN IMMEDIATE`; it takes the writer lock for microseconds. It is not gated by the query-active signal because latency-sensitive credit > latency-sensitive search when both fire simultaneously (the user just pressed Enter).

### Candidate scope (v1)

Per commander's directive: the candidate set for ranking is **every non-tombstoned row in the current `scan_generation` of the index**. No project-root filter, no heuristic classification of "project" vs "non-project" paths, no path-kind gate. A future ADR may introduce scoped modes (in-project, repo-only, type-filtered); v1 does not.

### Performance budgets

Commander directive: name numbers. Target `p99` at a 100 000-path index on a 2020-era laptop-class machine (four cores, NVMe SSD). These are not aspirational; they are review criteria for Phase 4.

| Event | Definition | p99 target |
|---|---|---|
| First-paint | keystroke received → first partial rendered | **≤ 50 ms** |
| Next-partial | second and subsequent partials for same query | ≤ 30 ms |
| Enter → `execvp` | Enter keystroke → `execvp` returns | **≤ 30 ms** |
| Enter → `print` flush | Enter → stdout flushed for `print` step | ≤ 10 ms |
| Visit credit | `record_visit` commit | ≤ 5 ms |
| Cold start → first-paint | binary invocation → first partial on zero query | ≤ 100 ms |

Miss of any budget at Phase 4 is a blocker, not a warning. `tracing` spans record each event (Surgeon §5); `queries_total` and a p99 histogram are dumped on SIGUSR1.

### Gate alignment

- **Portability.** Single scalar per path plus a SQLite row; no machine-local heuristics. Commits clean between hosts.
- **Composability.** Ranking is a pure function given the row; matcher is a swappable `Matcher` trait (Architect §4) with `nucleo-matcher` as the v1 impl.
- **Decade-longevity.** No tiered thresholds to migrate. Two columns, one formula. Constants live in code, not schema.
- **Empty-quadrant defensibility** (Intel §2). Frecency over indexed paths, not just visited — satisfied because the walker populates rows at S=0 and the first action lifts them into the habit band within one use.

## Alternatives considered

1. **zoxide-style tiered frecency** (`score = visits × age_weight(age)` with stepped weights). Rejected. The tier cliffs produce visible rank jitter at the hour/day/week boundaries; `visits` is unbounded; changing the tier constants requires data migration. Our formula is the continuous limit of this scheme — same intuition, no cliffs.

2. **Pure recency (`-age`) or pure visit-count (`visits`).** Rejected. Recency alone loses daily drivers to an accidental one-minute-ago `cd`; visit-count alone never drains abandoned projects. Intel's §3 post-mortems on `autojump` and `fasd` are the cautionary evidence.

3. **Min-max normalisation of match score over the current candidate set.** Rejected as unstable. As a search worker streams partials, the set's min/max shift, and previously-rendered rows reorder under the cursor. `tanh` with fixed calibration is streaming-stable; we pay a small calibration cost up front rather than a UX cost on every keystroke.

4. **Credit visits on every search result appearance.** Rejected. Creates a degenerate fixed point (the ranker reinforces whatever it surfaced) and opens an adversarial cheap shot (a noisy action spraying results can poison rankings).

5. **Project-root filtering of candidates in v1.** Rejected per commander's directive. Project inference is a separate design question; coupling it to ranking in v1 would leak a half-formed heuristic into the ADR that locks it.

6. **Full `nucleo` picker instead of `nucleo-matcher`.** Rejected per Quartermaster §2. We already own the pipeline (GEN counter, partial streaming, UI merge); we need only the scoring primitive.

## Consequences

**Binds `index` sector.** Schema must include `S REAL NOT NULL DEFAULT 0`, `last_update INTEGER NOT NULL DEFAULT 0`, `visits_total INTEGER NOT NULL DEFAULT 0`, `scan_generation INTEGER NOT NULL`, `tombstoned_at INTEGER`. `PRAGMA journal_mode = WAL`, `synchronous = NORMAL`, `wal_autocheckpoint = 0`. A `UNIQUE` index on canonical path. Visit upsert is a single statement inside `BEGIN IMMEDIATE`.

**Binds `search` sector.** Matcher is `nucleo-matcher`, integrated behind a `Matcher` trait (Architect §4). Ranking blend lives in `ranking::blend(match, frecency)` as the single point of change; weights and calibration constants are module-private `const`. Query workers must honour `QUERY_ACTIVE` semantics and must not block the writer's visit-credit path.

**Binds `ui` sector.** Empty-index and partial-scan banners are first-class render states, not error popups. Keyboard navigation must not credit visits; only action execution does.

**Binds `actions` sector.** The action executor calls `index::record_visit(candidate_id)` exactly once per action, on first successful step, subject to the 10 s per-path rate limit.

**Binds `ipc` sector.** `QUERY_ACTIVE: AtomicU64` timestamp is owned here; `GEN` counter from Architect §2 remains the pipeline spine.

**Dependencies.**
- ADR-002 (Quartermaster) must hold `nucleo-matcher` on the shortlist and ratify the bundled-SQLite decision this ADR assumes.
- ADR-003 (Security) must keep the closed-placeholder and canonicalisation rules; visit credit operates on canonical path.
- ADR-004 (Architect, 2nd sitting) must define the action executor semantics (success criteria per step kind) this ADR's visit-credit rule references.

**Explicitly out of scope.** Live filesystem watching (Quartermaster `notify` AVOID), project-root filtering, cross-machine index sync, per-user frecency segmentation, multi-tenant modes.

## Reviews

_Appended by peer reviewers._

> **council-intel, 2026-04-24 — non-blocking**
>
> Continuous exponential decay is the correct limit of the zoxide/fasd tiered scheme Intel surveyed — the constants in those tools are the artifact, the cliffs were a concession to integer storage. Two intel-lens concerns, both non-blocking: (1) visit-credit semantics under `on_failure = "abort"` contradict ADR-004 §5 — ADR-001 reads "Action failure with `on_failure = "abort"` credits nothing" while ADR-004 says first-success-wins even on abort; per HANDOFF 2026-04-24 12:12 request one-sentence alignment citing ADR-004 §5. (2) `K_match = 100` assumes nucleo's path-typical scores sit 60–200, and the constant is load-bearing for the streaming-stability argument; nucleo's raw score scale varies with query length, so I recommend Phase 2 calibration telemetry (a `tracing` span on raw `m_c` per query) before Phase 3 locks the UX. Neither concern threatens commander's intent or decade-longevity — calibration lives in `ranking::BLEND` as a code-const, not in the schema.

> **council-surgeon, 2026-04-24 — non-blocking**
>
> §Visit credit contradicts itself and ADR-004 §5 under `on_failure = "abort"`: the paragraph reads "Action failure with `on_failure = "abort"` credits nothing", yet the next sentence says "first success wins; later steps within the same action do not re-credit". ADR-004 §5 resolves the ambiguous case correctly — credit iff ≥1 step succeeded before the abort; suppressed only when the first step fails — so per HANDOFF 2026-04-24 12:12 item 2 request a one-sentence alignment citing ADR-004 §5. Surgeon-lens endorsements: `scan_generation` as the completion guard plus `tombstoned_at` on action-time stat failure maps position §2 and §4 directly; `PRAGMA wal_autocheckpoint = 0` paired with the `QUERY_ACTIVE` yield closes the "long checkpoint starves search partial" hazard from position §1b without touching `FULL`; carving `record_visit` out of the query-active gate is the right call — the user just pressed Enter and the ≤5 ms budget is non-negotiable. One portfolio nit: the cold-start → first-paint p99 ≤ 100 ms budget should name the exclusion for a startup `PRAGMA integrity_check` on a missing clean-shutdown sentinel (position §4). A full integrity_check on a 100 k-row index is not 100 ms-class work, and leaving the budget mute on that path invites a Phase 4 blocker on what is actually a recovery-only codepath.

> **council-quartermaster, 2026-04-24 — non-blocking**
>
> Supply alignment clean: `nucleo-matcher` (ADR-002 slot 6) and `rusqlite` with `bundled` (ADR-002 slot 1) are both on the authoritative roster and survive the gate-C re-reading; ADR-001's assumption of those two is honoured. The `Matcher` trait shape in Consequences preserves gate E (replaceability) — if the Helix team ever stalls, the swap lives at one seam and does not cascade into the index. One alignment item per HANDOFF 2026-04-24 12:12 item 2 and seconding Surgeon: §Visit credit reads self-contradictory under `on_failure = "abort"` — "Action failure with abort credits nothing" vs. "first success wins" — while ADR-004 §5 resolves it correctly (credit iff ≥1 step succeeded before the abort; suppressed only when the first step fails); request one sentence citing ADR-004 §5 so a Phase 3 Rifles implementer reading only ADR-001 cannot get the visit-credit contract wrong. Portfolio nit on the tuning-constant discipline: `ranking::BLEND` with `K_match = 100`, `K_frec = 10`, weights `(0.6, 0.4)` as module-private `const` is the correct shape — constants in code, not schema, so a Phase 3 re-tune is a recompile rather than a migration; this honours the "decade without a DB migration on tuning" invariant the Rationale claims. No blocker; commander's intent (portability, decade-longevity) served.

> **council-security, 2026-04-24 — non-blocking**
>
> Security-lens substance clean. §Visit credit operating on `canonical path` (Consequences, "visit credit operates on canonical path") aligns with ADR-003 §1's inode-vs-string asymmetry — the credit event consumes the trusted inode reference, not the display string, which is the correct seam. The 10 s per-`(path, 10s)` rate limit combined with "credit only on action execution, not on result appearance" closes the two adversarial-cheap-shot vectors I would otherwise flag: a noisy action spraying fake results cannot poison frecency, and a key-held repeat cannot compound — both are consistent with ADR-003 §1's posture that a local attacker at the user's UID is out of scope but injection-adjacent feedback loops are not. Tidying item 2 per HANDOFF 2026-04-24 12:12, thirding Intel, Surgeon, and Quartermaster: §Visit credit is self-contradictory under `on_failure = "abort"` ("Action failure with abort credits nothing" vs. "first success wins"); ADR-004 §5 resolves the ambiguous case correctly — request one sentence citing ADR-004 §5 so a Phase 3 Rifles implementer reading only ADR-001 cannot get the visit-credit contract wrong. Portfolio-lens endorsement: the `tanh` streaming-stability choice keeps the rendered top-K stable under partial arrival — this matters to ADR-003 only indirectly, but it denies the "late partial reorders the top row under the cursor and user presses Enter on the wrong candidate" class of ergonomic hazard, which is the shape an adversarial indexed path would want to exploit. No blocker; commander's intent served.

## Revision history

- 2026-04-24 — drafted by council-architect.
- 2026-04-24 — signed by commander.
