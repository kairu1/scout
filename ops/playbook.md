# SCOUT Playbook — Master Runbook Index

Per-phase runbooks. **Every phase is closed** — see `ops/OPORD.md`. This
index is kept as the record of how the campaign was run.

| Phase | Runbook | Milestone | State |
|---|---|---|---|
| 0 — Mobilization | [`phase-0-mobilize.md`](phase-0-mobilize.md) | Command post live | closed 2026-04-21 |
| 1 — War Council I | [`phase-1-council.md`](phase-1-council.md) | Four ADRs signed | closed 2026-04-24 |
| 2 — DB Takes Hill | [`phase-2-db.md`](phase-2-db.md) | Frecency index operational | closed 2026-07-05 |
| 3 — Main Assault | *no runbook written* | End-to-end flow | closed 2026-07-05 |
| 4 — Consolidation | *no runbook written* | Portable install | closed 2026-07-05 |
| 5 — AAR | *no runbook written* | Lessons captured | closed 2026-08-14 (`docs/aar/v1.md`) |

Phases 3–5 ran from HANDOFF orders rather than from a written runbook.
The earlier version of this table listed them as *"drafted at the
previous phase's close"*, which never happened — a reader chasing
`phase-3-assault.md` was chasing a file that has never existed. Recorded
rather than quietly deleted, because the gap between a planned artifact
and a written one is exactly the kind of thing an index should show.

**Rule of the playbook:** runbooks for phases later than the active phase are drafts — not authoritative — until the Chief of Staff signs them at phase entry. This prevents stale orders from being executed.

---

## Standing interrupts (always valid)

Issue these at any time. They take precedence over any active runbook.

- `stand down` — halt all engagements, officers return to standby.
- `redirect <new objective>` — current OPORD superseded; Chief of Staff re-authors.
- `promote <officer>` — expand that officer's sector.
- `AAR now` — triggered retrospective; next phase paused.

---

## Escalation ladder

1. Line officer → peer line officer (coordination) via HANDOFF.
2. Line officer → Chief of Staff (blocker, ambiguity) via HANDOFF `@cos`.
3. Chief of Staff → Commander (intent-touching decision, deadlock, budget) via HANDOFF `@commander`.
4. Commander → all (standing interrupt, direct order).

Never skip a rung unless the situation demands it (e.g., security incident → direct to commander).
