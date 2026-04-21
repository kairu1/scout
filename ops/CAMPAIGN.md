# CAMPAIGN PLAN — Operation SCOUT

> **Commander's intent:** Make finding and acting on my projects fast, composable, and portable across my machines.

Every decision below is subordinate to the intent above. When a proposal conflicts with it, we refuse the proposal — not the intent.

---

## Terrain & Objectives

- **Core objective:** a path-finder that doesn't just find — it *acts* (open in editor, spawn terminal, compose commands), ranks results by frecency (visit count + time decay), and carries config across machines.
- **Fixed standing orders (non-negotiable):**
  - SQLite is permanent. The index and frecency store live there.
  - Config is portable TOML, intended to be committed to dotfiles.
  - Rust is the language of the realm.
  - The predecessor `pathexplorer` is read-only reference terrain.
- **Ground we hold:** working fuzzy search, basic TUI, first-pass indexer, tmux/podman multi-agent experience from the agentic_vm_guide.
- **Enemy positions (risks):** ranking correctness, stale-query races, dependency rot across a decade, agent merge conflicts, scope creep away from the intent.

---

## Force Structure

Three echelons. **Deliberators decide. Line officers execute. Staff support.**

### War Council (deliberators — convene BEFORE every major engagement)

Do not write production code. Produce written findings (ADRs) and disband at phase close.

| Officer | Portfolio | Primary output |
|---|---|---|
| Architect | System design | ADR-001 Ranking doctrine, ADR-004 Action/config schema |
| Quartermaster | Dependencies & supply lines | ADR-002 Dependency roster |
| Security Officer | Threat model | ADR-003 Threat model |
| Intelligence (S2) | Ecosystem recon | Inputs into all four ADRs |
| Surgeon | Reliability & triage | Inputs into all four ADRs |

**War Council rule:** no line officer moves until the council's OPORD is signed by the commander.

### Line Officers (executors)

Each owns a sector. Commits land only on their branch.

| Officer | Sector | Branch | Files owned |
|---|---|---|---|
| 1st Rifles | Core/Search | `sector/search` | `src/search/**` |
| 2nd Rifles | Index/DB | `sector/index` | `src/index/**`, `migrations/**` |
| 3rd Rifles | TUI | `sector/tui` | `src/ui/**` |
| Engineers | Actions/Config | `sector/actions` | `src/actions/**`, `src/config/**` |
| Pioneers | Packaging/Ops | `sector/ops` | `Containerfile`, `.github/**`, `install.sh`, `ops/**` |

### Staff

| Role | Location | Function |
|---|---|---|
| Chief of Staff | Host (Mac) | Tempo, HANDOFF traffic, escalations, OPORD authorship |
| Commander | Host (you) | Intent, approvals, promotions, standing interrupt |
| Adjutant | Rotating line officer per phase | README/docs discipline |
| Medic | Shared by all officers | Tests; CI owned by Pioneers |

---

## Decade-Longevity Doctrine

No dependency joins the force without passing these gates. The Quartermaster applies these in ADR-002.

1. **Age ≥ 3 years with active commits in the last 6 months** — proof of life.
2. **Bus factor ≥ 3, or org-backed** (rust-lang, tokio-rs, BurntSushi, etc.).
3. **1.0+ semver, or explicit API-stability commitment** — no perpetual 0.x without justification.
4. **RustSec advisory history** reviewed — track record of responding to advisories.
5. **Replaceability** — a credible alternative exists if the crate is abandoned.
6. **Proximity to stdlib / de facto standard** — the nearer to the trunk, the better.

### Provisional roster (Quartermaster must reconfirm in ADR-002)

| Crate | Verdict | Notes |
|---|---|---|
| `rusqlite` | Hold | De facto Rust SQLite binding. |
| `clap` | Hold | Ubiquitous, org-backed. |
| `ignore` | Hold | Part of ripgrep family; battle-tested. |
| `crossterm` | Hold | Dominant cross-platform terminal lib. |
| `ratatui` | Verify | Successor to `tui-rs`; young fork but active. |
| `fuzzy-matcher` | Verify | Compare against `nucleo` (Helix editor's matcher). |
| `signal-hook` | Hold | Stable, narrow scope. |
| `dirs` | Watch | Compare against `directories`. |
| `num_cpus` | Swap | `std::thread::available_parallelism` is now stdlib. |
| `serde`, `serde_json`, `toml` | Candidate | Required for config; confirm schema. |

---

## Phased Campaign

Every phase carries milestone / checkpoint / connection. Commander's check-in at every phase boundary.

### Phase 0 — Mobilization
- **Milestone:** Podman + tmux environment stood up. `ops/` scaffolded. Sector branches cut. Agents not yet launched.
- **Checkpoint:** `podman ps` shows the container; `git branch` shows all sector branches; a smoke HANDOFF message round-trips.
- **Connection:** The battlefield exists. War Council can convene in Phase 1.
- **🪖 Commander's check-in:** Approve force structure, branch names, HANDOFF protocol. *(Already given — 2026-04-21.)*

### Phase 1 — War Council I: Doctrine & Supply
- **Milestone:** Four ADRs signed.
  - ADR-001 Ranking doctrine (frecency formula, decay curve, tie-breakers)
  - ADR-002 Dependency roster (decade-longevity scored)
  - ADR-003 Threat model (sanitization, action-exec safety, config trust)
  - ADR-004 Action & config schema (TOML shape, templates, composition)
- **Checkpoint:** Four files in `docs/adr/`, each `Status: Accepted`, reviewed by at least two officers.
- **Connection:** Line officers have orders they can execute without re-deliberating.
- **🪖 Commander's check-in:** Read the ADRs. Push back. Approve. **Highest-leverage check-in of the campaign.**

### Phase 2 — First Engagement: The DB Takes the Hill
- **Milestone:** 2nd Rifles ships frecency-capable schema with migrations, WAL config, streaming indexer (no in-memory buffering), visit-tracking insert path.
- **Checkpoint:** Index a 100k-file tree in <30s with memory under 100MB; `sqlite3 index.db .schema` shows the new tables; migration runs forward.
- **Connection:** Search and TUI can now pull ranked results.
- **🪖 Commander's check-in:** Gut-check responsiveness on real data. Approve move to Phase 3.

### Phase 3 — Main Assault: Search, TUI, Actions in parallel
- **Milestone:** Full flow — type → ranked results → pick → action → side effect (open in editor, cd, terminal).
  - 1st Rifles: ranking-aware search, cancel tokens for stale queries, path-prefix bug fix.
  - 3rd Rifles: action picker overlay, reduced lock contention in draw.
  - Engineers: TOML config loader, action templates, `--print` flag for shell integration.
- **Checkpoint:** End-to-end manual test on real projects; config on disk drives behavior.
- **Connection:** Tool is usable daily.
- **🪖 Commander's check-in:** Daily-drive for a week before Phase 4.

### Phase 4 — Consolidation: Portability
- **Milestone:** Pioneers ship Containerfile, install script, shell integration snippets, release workflow.
- **Checkpoint:** Fresh machine → clone dotfiles → install → scout works day one.
- **Connection:** Intent ("portable across machines") fulfilled.
- **🪖 Commander's check-in:** Approve release.

### Phase 5 — AAR & Promotions
- **Milestone:** After-action review. Which officers excelled (earn expanded sector in v2)? Which ADRs need revision?
- **Checkpoint:** `docs/aar/v1.md` written. Next campaign's opening objectives drafted.
- **Connection:** Lessons carried forward.

---

## Commander's Check-in Protocol

Three channels, always open:

1. **Scheduled check-ins** — end of each phase. Mandatory. Chief of Staff halts the force pending your word.
2. **OPORD sign-off** — new ADRs or significant direction changes require your approval before execution. Line officers are blocked until signed.
3. **Standing interrupt** — at any time:
   - `stand down` — halt all engagements
   - `redirect <new objective>` — current OPORD superseded
   - `promote <officer>` — expand that officer's sector
   - `AAR now` — trigger retrospective, pause next phase

Chief of Staff proactively escalates when officers deadlock in deliberation, when an engagement breaches time/cost budget, or when any decision touches commander's intent.

---

## Standing constraints on operational tempo

- Line officers are activated **only for their phase**. Phase 1 runs War Council; Phase 2 runs 2nd Rifles; Phase 3 runs three officers in parallel. This controls token spend and cognitive load.
- Agents go dark if they stop updating `ops/state/<officer>.json`. Chief of Staff detects silence during host-side checks.
- Every ADR must name the officer who wrote it and list reviewers. No anonymous doctrine.
