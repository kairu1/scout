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
