# OPORD — Active Operation Order

**Phase:** 1 — War Council I: Doctrine & Supply
**Status:** ISSUED. Execute `ops/phase-1-council.md`.
**Issued:** 2026-04-21
**Signed:** Chief of Staff
**Previous phase:** 0 — Mobilization — CLOSED 2026-04-21 (three commits on `main`).

---

## 1. Situation

Command post stood up. Container running. Toolchain verified inside sandbox. Five sector branches exist but no line officer has engaged. `pathexplorer` remains read-only reference terrain.

## 2. Mission

Convene the War Council. Produce four signed ADRs that set doctrine for every downstream engagement:

- **ADR-001 Ranking doctrine** — exact frecency formula, decay curve, tie-breakers.
- **ADR-002 Dependency roster** — every candidate crate scored against the six decade-longevity gates from `ops/CAMPAIGN.md`.
- **ADR-003 Threat model** — what we trust, what we sanitise, action-execution safety.
- **ADR-004 Action & config schema** — TOML shape, template variables, composition semantics.

## 3. Execution

### Commander's intent for this phase

Get doctrine right **before** any production code is written. A wrong choice here is cheap to fix now, expensive to fix in Phase 3. This is the highest-leverage check-in of the campaign.

### Phases of execution (four waves)

| Wave | Officers active | Output |
|---|---|---|
| 1 — Position papers | All five Council officers | `docs/adr/positions/<callsign>.md` — each officer's stance from their portfolio's angle |
| 2 — ADR drafts | Architect, Quartermaster, Security | Four ADRs at `Status: Draft` under `docs/adr/` |
| 3 — Peer review | All Council officers | Review blocks appended to each ADR |
| 4 — Commander sign-off | You | `Status: Accepted` + date on each ADR |

### Forbidden this phase

- Line officers (Rifles, Engineers, Pioneers) stand down. **No implementation code in any `src/` path.**
- No `Cargo.toml` edits — dependencies are what ADR-002 is for.
- No merges to `main` except documentation commits (ADRs, positions, OPORD updates).
- No touching `pathexplorer`.

## 4. Service & support

- Agent runtime: Claude Code inside the scout container. Commander verifies first-run auth at preflight.
- Artifacts: `docs/adr/positions/*.md`, `docs/adr/NNN-*.md`, updates to `ops/HANDOFF.md`.
- State: each Council officer writes its own `ops/state/council-*.json` at engagement, checkpoint, stand-down.

## 5. Command & signal

- Standing interrupt remains active: `stand down`, `redirect`, `promote`, `AAR now`.
- Four scheduled check-ins in this phase — one after each wave. Runbook names them explicitly.

---

## Success criteria (Phase 1)

- [ ] Five position papers exist under `docs/adr/positions/`.
- [ ] Four ADRs exist under `docs/adr/` at `Status: Accepted` with your signature and date.
- [ ] Each ADR has a `## Reviews` section with at least two peer reviewers who marked no blockers.
- [ ] `ops/HANDOFF.md` contains your `Phase 1 signed` entry.
- [ ] Council officer state files show `status: "standdown"` at close.

When green, signal **"Phase 1 green"**. I will then draft Phase 2 OPORD and `phase-2-db.md` for 2nd Rifles' engagement.
