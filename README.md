# SCOUT

Fast project finder and action launcher. Heir to `pathexplorer` — same commander's intent, proper force structure.

> **Commander's intent:** Make finding and acting on my projects fast, composable, and portable across my machines.

## Status

**Phase 4 complete — portable.** Index, ranked search, TUI (match
highlighting, frecency signal meter, preview pane), action executor,
installer, shell integration, CI tripwires, release machinery.

## Install

```sh
git clone <this-repo> && cd scout
./install.sh                                  # builds, installs to ~/.local/bin
echo "source $PWD/shell/scout.bash" >> ~/.bashrc   # guarded eval wrapper
```

Or grab a musl release tarball (x86_64 / aarch64) once a release is
tagged; it carries the binary, the shell snippet, and the example
config.

## Use

```sh

scout index ~/projects        # walk a tree into the index (gitignore-aware)
scout                         # TUI picker: type to filter, Enter acts, Tab opens the action menu
scout query hub               # non-interactive ranked results, best first
scout open-db <path>          # inspect (and if needed recover) an index DB
```

The TUI draws on stderr; stdout is reserved for `print` steps. The
shipped wrapper (`shell/scout.bash`) evals that stdout under an
allowlist — only `cd`/`printf`/`$EDITOR` line shapes ever execute — so
actions that print commands make Enter cd your shell or open your
editor, and anything unexpected is shown, never run. Start from
[`examples/config.toml`](examples/config.toml) (the installer never
copies it for you: your first config always goes through the trust
prompt).

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

Phase 5 — AAR & promotions (`docs/aar/v1.md`), then v2 objectives. Release itself is a commander act: push a `v*` tag and the release workflow attaches musl artifacts.
