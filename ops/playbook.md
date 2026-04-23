# SCOUT Playbook — Master Runbook Index

Per-phase runbooks. Execute in order. Commander's check-in required at each phase boundary.

| Phase | Runbook | Milestone |
|---|---|---|
| 0 — Mobilization | [`phase-0-mobilize.md`](phase-0-mobilize.md) | Command post live |
| 1 — War Council I | [`phase-1-council.md`](phase-1-council.md) | Four ADRs signed |
| 2 — DB Takes Hill | `phase-2-db.md` *(drafted at Phase 1 close)* | Frecency index operational |
| 3 — Main Assault | `phase-3-assault.md` *(drafted at Phase 2 close)* | End-to-end flow |
| 4 — Consolidation | `phase-4-consolidate.md` *(drafted at Phase 3 close)* | Portable install |
| 5 — AAR | `phase-5-aar.md` *(drafted at Phase 4 close)* | Lessons captured |

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
