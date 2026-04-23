# Force Structure — Operation SCOUT

Authoritative record of who owns what. Read this at deployment. Cross-sector touches are forbidden unless authorised via `HANDOFF.md`.

---

## War Council (deliberators)

Convened per phase. Produces ADRs under `docs/adr/`. Does not edit production code.

| Officer | Callsign | Portfolio | State file |
|---|---|---|---|
| Architect | `council-architect` | System design, data flow, ranking | `state/council-architect.json` |
| Quartermaster | `council-quartermaster` | Dependencies, decade-longevity scoring | `state/council-quartermaster.json` |
| Security Officer | `council-security` | Threat model, sanitization | `state/council-security.json` |
| Intelligence (S2) | `council-intel` | Ecosystem recon (fzf, zoxide, broot, nucleo, etc.) | `state/council-intel.json` |
| Surgeon | `council-surgeon` | Reliability, panic discipline, recovery | `state/council-surgeon.json` |

---

## Line Officers (executors)

Each owns one sector and one branch. Commits cross neither.

| Officer | Callsign | Sector | Branch | Files owned |
|---|---|---|---|---|
| 1st Rifles | `rifles-1` | Core/Search | `sector/search` | `src/search/**` |
| 2nd Rifles | `rifles-2` | Index/DB | `sector/index` | `src/index/**`, `migrations/**` |
| 3rd Rifles | `rifles-3` | TUI | `sector/tui` | `src/ui/**` |
| Engineers | `engineers` | Actions/Config | `sector/actions` | `src/actions/**`, `src/config/**` |
| Pioneers | `pioneers` | Packaging/Ops | `sector/ops` | `Containerfile`, `install.sh`, `.github/**`, release scripts, `ops/playbook.md`, phase runbooks |

---

## Shared territory (touch rules)

| File / path | Rule |
|---|---|
| `src/main.rs`, `src/lib.rs` | Any officer may propose changes via HANDOFF; commits only after `@cos` signs the HANDOFF thread. |
| `Cargo.toml` | Quartermaster proposes additions in ADRs; Pioneers executes approved additions on a `chore/deps-<topic>` branch. |
| `ops/state/<self>.json` | The named officer only. |
| `ops/HANDOFF.md` | Append-only. Never edit another officer's entry. |
| `ops/OPORD.md`, `ops/CAMPAIGN.md` | Chief of Staff writes; commander approves. Agents read only. |
| `docs/adr/**` | Written by the authoring War Council officer; reviewed by at least one other; finalised by commander signature. |
| `tests/**` | All officers write tests for their own sector. Integration tests live under `tests/integration/` and are shared; Pioneers owns CI. |
| `~/@kairu/@projects/@shell/pathexplorer` | **Read-only reference.** No edits. Study the code; do not modify. |

---

## Staff (non-combatant, always-on)

| Role | Location | Function |
|---|---|---|
| Chief of Staff | Host (Mac, outside containers) | Orchestration, OPORD authorship, HANDOFF routing, escalation to commander |
| Commander | Host (you) | Intent, approvals, promotions, standing interrupt |
| Adjutant | Rotating line officer | README, developer docs (assigned per phase in OPORD) |
| Medic | All officers | Tests. CI belongs to Pioneers. |

---

## Activation schedule

Officers are **off-duty** until their phase. Do not launch agents whose phase has not started — token cost matters.

| Phase | Active officers |
|---|---|
| 0 Mobilization | (None — commander alone + Chief of Staff from host) |
| 1 War Council I | All five Council officers |
| 2 DB takes hill | 2nd Rifles only |
| 3 Main assault | 1st Rifles, 3rd Rifles, Engineers (parallel) |
| 4 Consolidation | Pioneers |
| 5 AAR | Recalled roster — each officer files a memo |

---

## Worktree Discipline

Standing order. Activated at Phase 2; binding on every officer that commits code. Prior to Phase 2 no worktrees exist.

### Why

The host mounts one project directory to `/workspace`. One working tree holds one `.git/HEAD`. An officer that runs `git checkout <branch>` flips HEAD for every concurrent officer, corrupting their working state and risking commits to the wrong branch. Observed in `agentic_vm_guide`. Fixed there with worktrees. Codified here before Phase 3, where three officers run in parallel.

### The map — exact paths, no alternatives

Every sector branch has exactly one worktree path. Officers do not invent alternatives. All paths are relative to the repo root (`/workspace/` inside the container).

| Officer | Sector branch | Worktree path |
|---|---|---|
| 1st Rifles | `sector/search` | `worktrees/sector-search/` |
| 2nd Rifles | `sector/index` | `worktrees/sector-index/` |
| 3rd Rifles | `sector/tui` | `worktrees/sector-tui/` |
| Engineers | `sector/actions` | `worktrees/sector-actions/` |
| Pioneers | `sector/ops` | `worktrees/sector-ops/` |

### Rules

1. **One officer → one worktree → one branch.** No exceptions.
2. **Only Chief of Staff creates or destroys worktrees.** At phase preflight Chief of Staff runs `git worktree add <path> <branch>`. At phase close, after merge to `main`, Chief of Staff runs `git worktree remove <path>`. Officers never run `git worktree add`, `git worktree remove`, or `git worktree move`.
3. **An officer operates only inside their assigned worktree.** `cd` into it at engagement; do not leave it until standdown.
4. **Permitted git commands inside your worktree:** `git status`, `git diff`, `git log`, `git add`, `git commit`, `git stash`.
5. **Forbidden git commands for every officer, in every worktree:** `git checkout <branch>`, `git switch <branch>`, `git reset --hard`, `git branch -D`, `git push --force`, `git worktree add`, `git worktree remove`, `git worktree move`. Attempting any of these is a standing-order violation.
6. **Cross-sector work routes through `HANDOFF.md` only.** An officer never `cd`s into another officer's worktree, never edits files there, never commits to another officer's branch.
7. **Merge path.** Sector branch → `main` only via HANDOFF request to `@cos` with commander authorization. No direct push-to-main from any worktree.
8. **Gitignore.** Chief of Staff adds `/worktrees/` to `.gitignore` at Phase 2 preflight, ahead of the first `git worktree add`.

### Phase activation

| Phase | Worktrees live | Owned by |
|---|---|---|
| 0 Mobilization | None | — |
| 1 War Council | None — Council writes docs to `main` | — |
| 2 DB | `worktrees/sector-index/` | 2nd Rifles |
| 3 Main Assault | `worktrees/sector-search/`, `worktrees/sector-tui/`, `worktrees/sector-actions/` | 1st Rifles, 3rd Rifles, Engineers |
| 4 Consolidation | `worktrees/sector-ops/` | Pioneers |
| 5 AAR | None — memos to `main` | — |

### Enforcement

At every phase preflight Chief of Staff runs `git worktree list` and compares against the phase-activation table. Mismatch halts the phase. Any officer observed issuing a forbidden git command — in state files, HANDOFF entries, or commit trails — is benched; Chief of Staff tears down their worktree, recreates it clean, and restarts the engagement.
