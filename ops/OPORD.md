# OPORD — Active Operation Order

**Phase:** 2 — First Engagement: The DB Takes the Hill
**Status:** ISSUED. Execute `ops/phase-2-db.md`.
**Issued:** 2026-05-01
**Signed:** Chief of Staff
**Previous phase:** 1 — War Council I — CLOSED 2026-04-24 (four ADRs signed; three doctrinal revisions filed; non-signing).

---

## 1. Situation

Phase 1 is closed. Four ADRs are signed doctrine: ADR-001 Ranking, ADR-002 Dependency roster, ADR-003 Threat model, ADR-004 Action & config schema. Three non-blocking revisions have landed in ADR-001 and ADR-003; status remains Accepted on both. `main` has eleven Phase 1 commits.

Council stands down. No line officer has engaged. `/worktrees/` is ignored at `.gitignore` per AGENTS.md §Worktree Discipline; no worktree has been created yet.

`pathexplorer` remains read-only reference terrain.

## 2. Mission

2nd Rifles engages. Sector: `src/index/**`, `migrations/**`. Branch: `sector/index`. Worktree: `worktrees/sector-index/`.

2nd Rifles ships the frecency-capable index foundation that ADR-001 binds:

- **Schema + forward migration** — every column ADR-001 §Consequences names: `S REAL`, `last_update INTEGER`, `visits_total INTEGER`, `scan_generation INTEGER`, `tombstoned_at INTEGER`, plus `path TEXT` with a `UNIQUE` index on canonical form.
- **WAL configuration** — `journal_mode = WAL`, `synchronous = NORMAL`, `wal_autocheckpoint = 0`, per ADR-001 §Index-writer pacing.
- **Streaming indexer** — walker populates rows at `S = 0`, batched inserts of ≈1 000 per `BEGIN IMMEDIATE` per ADR-001 + Surgeon §2. No in-memory buffering of the full tree.
- **Visit-tracking insert path** — `record_visit(candidate_id)` as single-statement upsert inside `BEGIN IMMEDIATE`, with the 10-second per-path rate limit enforced in the caller (action executor in Phase 3 — Phase 2 ships the DB primitive + unit tests).
- **Crash-recovery scaffolding** — clean-shutdown sentinel + `PRAGMA integrity_check` on open when sentinel is absent, rename-and-rebuild on corruption, per Surgeon §4 and ADR-001 §Degradation.

Not shipped in Phase 2: the search pipeline, the matcher integration, the TUI. Those are Phase 3.

## 3. Execution

### Commander's intent for this phase

Build the DB that every downstream engagement queries. It must survive SIGINT, power loss, and a 100 k-path tree without memory blowout. The correctness bar is higher than the feature bar — a broken index that looks fast is worse than a slow index that is honest.

### Doctrinal note — ADR-002 Phase 2 admission list expansion

ADR-002 §Consequences reads "Only `rusqlite` (with `bundled`) and `tracing` (+ `tracing-subscriber` for init) are admitted in the index/DB sector. `signal-hook` enters at Phase 2 for SIGINT discipline during indexing."

`CAMPAIGN.md` Phase 2 milestone explicitly names "streaming indexer (no in-memory buffering)", which requires the `ignore` crate (ADR-002 slot 3, BurntSushi / ripgrep-family) for `.gitignore`-aware parallel walking. Re-implementing gitignore parsing violates ADR-002's own ~200-line rule and gate E. The admission list's omission of `ignore` is a Phase 2 oversight in ADR-002 drafting, not a deliberate exclusion.

**This OPORD extends the ADR-002 Phase 2 admission list** to include `ignore` (slot 3) in addition to `rusqlite`, `tracing`, `tracing-subscriber`, and `signal-hook`. Commander's signature on this OPORD constitutes approval; a one-line revision to ADR-002 §Consequences §Binds-2nd-Rifles captures the expansion in the ADR's Revision history after OPORD sign-off.

Nothing else expands. Any crate outside {`rusqlite`, `tracing`, `tracing-subscriber`, `signal-hook`, `ignore`} is rejected in review.

### Wave 3 non-blocking items that apply to Phase 2

- **Surgeon on ADR-001 — integrity_check exempt from cold-start budget.** ADR-001's p99 cold-start → first-paint ≤ 100 ms budget is the steady-state number. On a missing clean-shutdown sentinel, a full `PRAGMA integrity_check` on a 100 k-row index is not 100 ms-class work. 2nd Rifles implements integrity_check on the recovery path with its own budget (target ≤ 2 s; hard fail at 10 s) and a `tracing` span (`index.recovery.integrity_check`) so the budget miss is visible. The UI surface is a one-line banner on the recovery path, not an error popup.

### Phases of execution (four engagements)

| Engagement | Scope | Milestone |
|---|---|---|
| 1 — Schema + migration | `migrations/0001_initial.sql`, `src/index/schema.rs` | Schema matches ADR-001 §Consequences exactly; forward migration runs cleanly; `sqlite3 index.db .schema` inspection passes |
| 2 — WAL + PRAGMA | `src/index/pragma.rs` | WAL mode active; `synchronous = NORMAL`; `wal_autocheckpoint = 0`; `QUERY_ACTIVE` atomic allocated (used by search in Phase 3) |
| 3 — Walker + batched insert | `src/index/walk.rs`, `src/index/insert.rs` | 100 k-path tree walks to completion; memory stays under 100 MB; batched 1 000-row `BEGIN IMMEDIATE` commits; `scan_generation` advances only on complete walk |
| 4 — Visit path + recovery | `src/index/visit.rs`, `src/index/recovery.rs` | `record_visit(id)` runs inside `BEGIN IMMEDIATE` in ≤ 5 ms median; missing-sentinel triggers `integrity_check`; corrupt DB renames to `index.db.corrupt-<epoch>` and rebuilds |

Each engagement closes with state-file update and a commit to `sector/index` inside the worktree.

### Forbidden this phase

- Any `src/` path outside `src/index/**` and `src/main.rs` wiring needed to call indexer code.
- Any crate outside the expanded Phase 2 admission list above (`rusqlite`, `tracing`, `tracing-subscriber`, `signal-hook`, `ignore`).
- Any work on `sector/search`, `sector/tui`, `sector/actions`, `sector/ops`. Those officers remain unmobilized.
- Any commit to `main` — Phase 2 commits land on `sector/index` inside the worktree. Merge to `main` at Phase 2 close, via HANDOFF, commander-authorised.
- Any `git worktree` command outside the one Chief of Staff runs at preflight. Officers never `add`, `remove`, or `move` worktrees.
- Any modification of pathexplorer.

## 4. Service & support

- Agent runtime: Claude Code inside the scout container, entered via `podman exec` into `/workspace/worktrees/sector-index/` (not `/workspace` — the worktree is where 2nd Rifles operates).
- Artifacts: `src/index/**`, `migrations/**`, `tests/index/**` (unit + integration), updates to `ops/HANDOFF.md` and `ops/state/rifles-2.json`.
- State: `rifles-2` updates `ops/state/rifles-2.json` at engagement, checkpoint, stand-down — same three-moment discipline as Council.
- Worktree: Chief of Staff creates `worktrees/sector-index/` at preflight (see §Preflight of `ops/phase-2-db.md`); 2nd Rifles operates only inside.

## 5. Command & signal

- Standing interrupt remains active: `stand down`, `redirect`, `promote`, `AAR now`.
- Two scheduled commander check-ins in Phase 2:
  - **Check-in #1 — After engagement 2** (schema + WAL visible on disk). Commander inspects `sqlite3 index.db .schema` output.
  - **Check-in #2 — After engagement 4** (Phase 2 close). Commander gut-checks the 100 k-path walk on real data and approves move to Phase 3.

---

## Success criteria (Phase 2)

- [ ] `sector/index` worktree exists at `worktrees/sector-index/` and passes `git worktree list` inspection.
- [ ] `migrations/0001_initial.sql` exists and matches ADR-001 §Consequences schema.
- [ ] `sqlite3 index.db .schema` on a freshly-created index shows every column ADR-001 names, with the `UNIQUE` index on canonical path.
- [ ] `cargo test -p scout --test index` passes all unit + integration tests inside the container.
- [ ] A smoke-test walk over a 100 k-path synthetic tree (fixture provided in the runbook) completes in < 30 s with RSS < 100 MB on the scout container.
- [ ] `record_visit` commits in ≤ 5 ms median over 1 000 repeated calls.
- [ ] Missing clean-shutdown sentinel triggers `integrity_check`; a corrupt DB fixture gets renamed to `index.db.corrupt-<epoch>` and rebuilt.
- [ ] SIGINT mid-indexing rolls back the in-flight batch cleanly; `scan_generation` stays at the prior value; next launch serves the prior generation.
- [ ] `rifles-2` state file shows `status: "standby"` at close with notes summarising engagements 1–4.
- [ ] `ops/HANDOFF.md` contains a `Phase 2 green` closing entry from 2nd Rifles.
- [ ] ADR-002 §Consequences §Binds-2nd-Rifles carries a Revision history entry noting the Phase 2 admission list expansion for `ignore` (filed by commander or Chief of Staff after OPORD sign-off).

When green, signal **"Phase 2 green"**. Chief of Staff drafts Phase 3 OPORD and `ops/phase-3-assault.md` for the parallel engagement of 1st Rifles + 3rd Rifles + Engineers.
