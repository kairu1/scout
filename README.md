# SCOUT

Fast project finder and action launcher. Heir to `pathexplorer`.

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

Config lives at `$XDG_CONFIG_HOME/scout/config.toml` (see ADR-004 for the schema; ADR-003 for the first-run trust prompt). Without a config, compiled-in defaults apply: Enter opens the selection in `$EDITOR` **when you run the binary directly** (`command scout`). Under the shell wrapper — the recommended setup — use a print-based config like [`examples/config.toml`](examples/config.toml) instead: an editor spawned inside the wrapper's command substitution can't own the terminal, so the wrapped `edit` opens your editor by printing an `$EDITOR` command your shell runs.

Ranking blends fuzzy match quality with frecency (7-day half-life); visits are credited only when an action executes (ADR-001).

## Shell integration

The wrapper below is what makes Enter *do* things in your shell — a
child process cannot `cd` its parent, so scout prints commands and this
function evals them, behind an allowlist so nothing unexpected ever
executes. Canonical copy: [`shell/scout.bash`](shell/scout.bash)
(source it from your rc, or paste the function directly):

```bash
# scout shell integration (canonical copy — ships with the product).
#
# Why this exists: a child process cannot cd its parent shell, so scout
# actions PRINT commands on stdout and this function evals them in your
# shell. The eval is guarded: only allowlisted line shapes run (cd /
# printf / $EDITOR-$VISUAL invocations); anything else — a bare path, a
# value-printing action, corrupted output — is shown, never executed.
#
# Install: source this file from your shell rc, e.g.
#   source /path/to/scout/shell/scout.bash
#
# Bare `scout` runs the picker; subcommands (index, query, open-db)
# pass through to the binary untouched.
scout() {
  if [ $# -eq 0 ]; then
    local out line
    out="$(command scout)" || return $?
    [ -z "$out" ] && return 0
    while IFS= read -r line; do
      case "$line" in
        'cd '*|'printf '*|'${EDITOR'*|'${VISUAL'*) ;;
        *) printf 'scout: refusing to eval unexpected output: %s\n' "$line" >&2; return 1 ;;
      esac
    done <<< "$out"
    eval "$out"
  else
    command scout "$@"
  fi
}
```

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
