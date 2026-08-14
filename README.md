# SCOUT

A fast terminal project finder and action launcher. Type a few
characters, get your projects ranked by fuzzy-match quality blended with
frecency (how often and how recently you have used them), press Enter,
and something happens — your shell cds there, your editor opens, a
command runs.

Offline, single-binary, no network. The index and the frecency store are
one SQLite file; the config is portable TOML meant to live in your
dotfiles.

## Install

```sh
git clone <this-repo> && cd scout
./install.sh                                       # builds, installs to ~/.local/bin
echo "source $PWD/shell/scout.bash" >> ~/.bashrc    # guarded eval wrapper
```

Or take a musl release tarball (x86_64 / aarch64), which carries the
binary, the shell snippet, and the example config.

Then populate the index and run the picker:

```sh
scout index ~/projects
scout
```

## Commands

| Command | What it does |
|---|---|
| `scout` | Interactive picker. Type to filter, `Up`/`Down` to move, `Enter` runs the default action, `Tab` opens the action menu, `?` shows the keys, `Esc` or `Ctrl-C` quits. |
| `scout index <path>` | Walk a tree into the index. Streaming and gitignore-aware; safe to re-run. |
| `scout query <query>` | Print ranked results, best first — non-interactive, for scripts and pipes. |
| `scout doctor` | Print the state scout resolves at startup — config, trust, index, environment — each line marked `ok`, `warn` or `FAIL`. Read-only. |
| `scout open-db <path>` | Open an index database, print its vitals, and recover it if it needs recovering. |

Flags:

- `scout index --hidden` — include dotfiles and dot-directories (excluded by default).
- `scout index --follow` — follow symlinks while walking (off by default).
- `scout query --limit <n>` — how many results to print (default 20).
- `scout --version`, `scout --help`, `scout <command> --help`.

Re-running `scout index` on the same tree is the normal way to refresh:
it starts a new scan generation and tombstones paths that have gone
away. There is no filesystem watcher; the index is a snapshot.

## How the picker behaves

A result leads with its **name**, not its path. Location appears only as
far as it needs to: two projects both called `api` show as `api
service-hub` and `api wraptious`, while a name that is already unique on
screen shows no path at all. A marker distinguishes a git repository
(`⑂`) from a plain directory (`‣`) from a file.

Results are ranked by fuzzy-match quality blended with frecency on a
7-day half-life, so a project you opened this morning outranks an
equally good match you last touched in March. Rank is expressed by
order; matched characters are highlighted so you can see why a row
matched. A visit is credited only when an action actually executes —
appearing in a result list is not a visit.

The picker shows the best eight matches rather than a scrollable window.
If what you want is not there, type another character.

The picker draws on **stderr**. Stdout is reserved for `print` steps, so
`scout` composes inside command substitution without the UI polluting
the output.

## Shell integration

A child process cannot `cd` its parent shell. So scout *prints* commands
on stdout and a shell function evals them in your shell — behind an
allowlist, so only `cd`, `printf`, and `$EDITOR`/`$VISUAL` line shapes
ever execute. A bare path, a value-printing action, or corrupted output
is shown to you, never run.

The canonical copy ships as [`shell/scout.bash`](shell/scout.bash).
Source it from your rc, or paste the function directly:

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
# Bare `scout` runs the picker; subcommands (index, query, doctor,
# open-db) pass through to the binary untouched.
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

## Configuration

Config lives at `$XDG_CONFIG_HOME/scout/config.toml`. Start from
[`examples/config.toml`](examples/config.toml) — the installer
deliberately does not copy it for you, because your first config goes
through the trust prompt.

Actions are declarative TOML: a name, a keybinding, and a list of steps
that either spawn a process or print a command for your shell to run.
Placeholders (`{path}`, `{parent}`, `{name}`, `{query}`, and friends)
are quoted at the print seam, so a directory called `proj $(rm -rf ~)`
is a filename, not an instruction.

On first run — and on every subsequent change to the file — scout shows
you the actions it is about to trust and asks for confirmation. It needs
a TTY to ask; if there is no terminal it refuses rather than trusting
silently, and tells you to use `scout query` instead.

Without any config, compiled-in defaults apply: Enter opens the
selection in `$EDITOR` **when you run the binary directly**
(`command scout`). Under the shell wrapper — the recommended setup — use
a print-based config like [`examples/config.toml`](examples/config.toml)
instead, because an editor spawned inside the wrapper's command
substitution cannot own the terminal. The wrapped `edit` action instead
prints an `$EDITOR` command that your shell runs after scout exits.

## Files scout owns

| Path | Contents |
|---|---|
| `$XDG_CONFIG_HOME/scout/config.toml` | Your actions. Portable; commit it to your dotfiles. |
| `$XDG_DATA_HOME/scout/index.db` | The index and frecency store (SQLite, WAL). |
| `$XDG_STATE_HOME/scout/trusted-config.sha256` | Hash of the config you approved. |
| `$XDG_STATE_HOME/scout/scout.log` | Picker log. |

Unset `XDG_*` variables fall back to `~/.config`, `~/.local/share`, and
`~/.local/state`. The config is searched in that order:
`$XDG_CONFIG_HOME/scout/config.toml`, then `~/.config/scout/config.toml`,
then `/etc/scout/config.toml` — the last being an operator-provided
default for a shared machine.

## When something goes wrong

Start with `scout doctor`. It prints what scout actually resolved —
which config won the discovery chain, whether it is trusted, what the
index holds, how your environment reads — rather than what you expect it
to have resolved:

```sh
scout doctor
```

It exits 0 when nothing failed and 1 when something did, so it works in
a script. It never prompts and never writes: it will not create a
database, re-trust a config, or repair a corrupt index, because a
diagnostic that fixes things destroys the evidence you called it to see.
The environment section prints a fixed short list of variables and never
your whole environment, so the output is safe to paste into a bug
report.

Most failures are one of four things, and `doctor` names all four: the
config you edited is not the one that loaded (a symlinked config is
skipped — it says so), the trust hash is stale, the index predates the
thing you are searching for, or `$EDITOR` is unset.

For anything deeper, raise the log level with `SCOUT_LOG`, which takes a
level or a per-module filter:

```sh
SCOUT_LOG=debug scout index ~/projects     # why was a path skipped?
SCOUT_LOG=scout=debug scout query hub      # scout's own logs only
SCOUT_LOG=scout::index=trace scout index ~/projects
```

Levels are `off`, `error`, `warn`, `info` (the default), `debug`,
`trace`. A bare word that is not a level is read as a *module name*, so
`SCOUT_LOG=dbug` would hide everything instead of showing more — scout
warns when you do that.

In the picker, logs go to `$XDG_STATE_HOME/scout/scout.log` rather than
the screen, because the picker owns the terminal.

`scout query <term>` is also useful on its own: it skips the TUI
entirely, so errors go to stderr where you can read them. And
`scout open-db <path>` opens a database directly — unlike `doctor`, it
*will* recover a damaged one.

If a path you expect is missing, the usual causes are that it is
gitignored, hidden (re-run `scout index --hidden`), behind a symlink
(`--follow`), or simply indexed before it existed — `scout index` is a
snapshot, so re-run it.

## Licence

MIT.
