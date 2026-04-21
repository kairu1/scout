# OPORD — Active Operation Order

**Phase:** 0 — Mobilization
**Status:** PENDING execution. Awaiting commander to run `ops/phase-0-mobilize.md`.
**Issued:** 2026-04-21
**Signed:** Chief of Staff

---

## 1. Situation

Command post scaffolded on host. No container running. No branches cut. No agents deployed. `pathexplorer` remains the sole reference terrain — read-only.

## 2. Mission

Stand up the SCOUT command post: podman image built, container running with host volume mounted, `main` branch initialised, five `sector/*` branches cut, initial state files in place, smoke HANDOFF entry round-tripped.

## 3. Execution

### Commander's intent for this phase

Establish the battlefield so Phase 1 can convene the War Council. **No implementation code. No dependencies added. No agents launched yet.**

### Tasks

1. Commander runs `ops/phase-0-mobilize.md` in order. Every step has its own checkpoint.
2. Chief of Staff stands by on host to author Phase 1 OPORD once Phase 0's checkpoint is green.

### Forbidden this phase

- Touching `pathexplorer` in any write mode.
- Adding crates to `Cargo.toml`.
- Implementing search, indexing, TUI, or any production code.
- Launching line officers or War Council agents.

## 4. Service & support

- Logs: `ops/logs/` (git-ignored contents; `.gitkeep` committed).
- State: `ops/state/*.json`. Officers are `unmobilized` at Phase 0.
- Escalation: write to `ops/HANDOFF.md` tagged `@commander` or `@cos`.

## 5. Command & signal

- Commander's standing interrupt: `stand down`, `redirect`, `promote`, `AAR now`.
- Scheduled check-in: end of Phase 0 — verify checkpoint and approve move to Phase 1.

---

## Success criteria (Phase 0)

- [ ] `podman images` lists `scout:latest`.
- [ ] `podman ps` shows a running `scout` container with `/workspace` mounted.
- [ ] `git branch` on host lists `main` + `sector/search`, `sector/index`, `sector/tui`, `sector/actions`, `sector/ops`.
- [ ] `ops/state/*.json` files are committed to `main`.
- [ ] Chief of Staff posts a "command post live" entry in `HANDOFF.md` and commander acknowledges.
