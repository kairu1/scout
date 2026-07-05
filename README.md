# SCOUT

Fast project finder and action launcher. Heir to `pathexplorer` — same commander's intent, proper force structure.

> **Commander's intent:** Make finding and acting on my projects fast, composable, and portable across my machines.

## Status

**Phase 3 complete — tool is working.** Index, ranked search, TUI, and the action executor are merged to `main`; the full flow (type, rank, pick, act, credit) is exercised end-to-end. Phase 4 (packaging/portability) is not started.

## Use

```sh
cargo build --release

scout index ~/projects        # walk a tree into the index (gitignore-aware)
scout                         # TUI picker: type to filter, Enter acts, Tab opens the action menu
scout query hub               # non-interactive ranked results, best first
scout open-db <path>          # inspect (and if needed recover) an index DB
```

The TUI draws on stderr; stdout is reserved for `print` steps, so shell integration is `eval "$(scout)"` with an action like:

```toml
schema_version = 1

[[action]]
name = "go"
description = "cd into the selection"
keybinding = "enter"
steps = [ { kind = "print", format = "cd {path}" } ]
```

Config lives at `$XDG_CONFIG_HOME/scout/config.toml` (see ADR-004 for the schema; ADR-003 for the first-run trust prompt). Without a config, compiled-in defaults apply: Enter opens the selection in `$EDITOR`.

Ranking blends fuzzy match quality with frecency (7-day half-life); visits are credited only when an action executes (ADR-001).

## Orient

| Document | Purpose |
|---|---|
| [`ops/CAMPAIGN.md`](ops/CAMPAIGN.md) | Full campaign plan — five phases, force structure, decade-longevity doctrine |
| [`ops/OPORD.md`](ops/OPORD.md) | Active operation order (current phase) |
| [`ops/AGENTS.md`](ops/AGENTS.md) | Force structure and sector ownership |
| [`ops/HANDOFF.md`](ops/HANDOFF.md) | Async comms between agents |
| [`ops/playbook.md`](ops/playbook.md) | Runbook index across all phases |
| [`CLAUDE.md`](CLAUDE.md) | Standing orders for every deployed agent |
| [`docs/adr/`](docs/adr/) | Signed doctrine: ranking, dependencies, threat model, action schema |

## Execute next

Phase 4 — Consolidation (Pioneers: install script, shell snippets, release workflow). Commander-gated.
