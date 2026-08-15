# OPORD — Active Operation Order

**Phase:** none active — the campaign is closed.
**Status:** v1 complete (Phases 0–5, `docs/aar/v1.md`), released `v0.1.0`
2026-07-06. v2 complete 2026-08-14 (AAR §8), released `v0.2.0` — tag
re-cut 2026-08-15 onto the CI-hang fix. `v0.2.1` tagged 2026-08-15: the
documentation sortie plus a rebuilt reference config (14 actions, chords
throughout). Neither 0.2.x tag has been pushed yet, so no release
artifacts exist for either. No phase is open. The next body of work is
v3, which is unassigned and needs a commander's directive before an
officer engages.
**Last revised:** 2026-08-15

---

## 1. Situation

The tool is built, released twice, and daily-driven. Nine ADRs bind the
code. Every gate the campaign specified is wired and has been observed
failing at least once: 116 tests, `cargo fmt --check`, `clippy -D
warnings`, `cargo deny`, `cargo audit` on push *and* on a weekly
schedule, the 100k-path perf budget as its own release-profile job, the
transitive ceiling (119 of 150), and a musl cross-compile smoke on both
architectures. Every CI job carries `timeout-minutes`.

Local `main` carries the whole of v2 plus the CI-hang fix and the
documentation sweep below. Pushing is the commander's act; this box
holds no credentials.

## 2. Mission

None assigned. The force is on standby.

Work that arrives without a phase — a commander-reported defect, a
documentation sweep — is executed on `main` under the standing orders
and filed in `ops/HANDOFF.md`. Worktree and sector-branch discipline
(`ops/AGENTS.md`) binds again the moment a phase opens and two or more
officers run in parallel.

## 3. What is actually open

Carried to v3 by AAR §8, none of it assigned:

- council-security's review of the ADR-003 display-integrity revision.
- council-security's review of the ADR-006 environment allowlist —
  specifically whether `TERM` belongs. Evidence gathered 2026-08-14:
  scout never reads `TERM`, but `crossterm` does (`ansi_support.rs`
  gates ANSI on `TERM != "dumb"`), so it does change scout's observable
  rendering. The recommendation on record is to keep it and restate the
  *principle* as "variables that change scout's observable behaviour".
- `toml` 1.x. Nothing forces it; it moves when something does.
- Homoglyph and confusable-script display integrity — deliberately out
  of ADR-003's scope, since unlike bidi it does not make the display
  disagree with itself.
- A JSON output mode, which reopens only if scout's output ever becomes
  genuinely nested. It is three scalars and a string today.

## 4. Closed phases

| Phase | Closed | Carried into |
|---|---|---|
| 0 Mobilization | 2026-04-21 | `ops/phase-0-mobilize.md` |
| 1 War Council | 2026-04-24 | ADR-001 … ADR-004 |
| 2 DB takes hill | 2026-07-05 | schema, streaming indexer, 100k budget |
| 3 Main assault | 2026-07-05 | search, TUI, actions — five deviations recorded and later ratified |
| 4 Consolidation | 2026-07-05 | `install.sh`, `shell/scout.bash`, `examples/config.toml`, CI, release workflow, MSRV pin, `v0.1.0` |
| 5 AAR & promotions | 2026-08-14 | `docs/aar/v1.md` |
| v2 (post-campaign) | 2026-08-14 | ADR-005 … ADR-009, dependency refresh, two shadow-review rounds, `v0.2.0` |

Phase 4's success criteria are recorded as met in the AAR with one
correction it made itself: the fresh-machine drill passed by running
`sh install.sh`, which is not what the README tells a reader to type,
and `install.sh` was committed non-executable. Fixed in `da803b1`. That
is AAR §3.6, and it is why this document no longer carries a checklist
of its own — an unticked box under a header that says CLOSED is worse
than no box at all.
