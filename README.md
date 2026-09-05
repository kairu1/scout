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
| `scout` | Interactive picker. Type to filter, `Left`/`Right` to edit the search text, `Up`/`Down` to move the selection, `Enter` runs the default action, `Tab` opens the searchable action pane, `?` on an empty search shows the keys, `Esc` or `Ctrl-C` quits. |
| `scout index <path>` | Walk a tree into the index. Streaming and gitignore-aware; safe to re-run. |
| `scout query <query>` | Print ranked results, best first — non-interactive, for scripts and pipes. |
| `scout doctor` | Print the state scout resolves at startup — config, trust, index, environment — each line marked `ok`, `warn` or `FAIL`. Read-only. |
| `scout open-db <path>` | Open an index database, print its vitals, and recover it if it needs recovering. |

Flags:

- `scout index --hidden` — include dotfiles and dot-directories (excluded by default).
- `scout index --follow` — follow symlinks while walking (off by default).
- `scout query --limit <n>` — how many results to print (default 20).
- `scout query --format tsv` — `rank`, `visits`, `path`, tab-separated.
  The path comes last, so a tab inside a path cannot shift a field:
  split on the first two tabs and take the rest whole.
- `scout query --print0` — NUL-separated paths, for `xargs -0`.
- `scout doctor --format tsv` — `level`, `section`, `name`, `detail`.
- `scout --version`, `scout --help`, `scout <command> --help`.

`scout query` **exits 1 when nothing matched**, so a script can branch
without inspecting the output. Piped output is byte-exact; when stdout
is a terminal, terminal escape sequences in a path are stripped so a
directory name cannot rewrite your terminal.

Re-running `scout index` on the same tree is the normal way to refresh:
it starts a new scan generation and tombstones paths that have gone
away. There is no filesystem watcher; the index is a snapshot.

## How the picker behaves

Results are ranked by the **name** first. Searching `photo-store` returns
the directories called `photo-store` — not the hundred files that happen to
live inside one. A name that *is* your query outranks a longer name that
merely contains it, so `photo-store` beats `photo-store-admin`.

A result leads with its **name**, not its path. Location appears only as
far as it needs to: two projects both called `api` show as `api
billing` and `api storefront`, while a name that is already unique on
screen shows no path at all. Name and location are separate columns, so
the eye reads down the names. A marker distinguishes a git repository
(`⑂`) from a plain directory (`‣`) from a file.

Results are ranked by fuzzy-match quality blended with frecency on a
7-day half-life, so a project you opened this morning outranks an
equally good match you last touched in March. Rank is expressed by
order; matched characters are highlighted so you can see why a row
matched. A visit is credited only when an action actually executes —
appearing in a result list is not a visit.

The picker shows what fits the pane rather than a scrollable window. If
what you want is not there, type another character.

The search field is editable: `Left`/`Right` move the cursor through
what you have typed, `Home`/`End` jump to either end, `Backspace`
deletes before the cursor and `Delete` under it, and typing or pasting
inserts at the cursor. `Up`/`Down` stay with the result list.

`Tab` opens the action pane beside the results — a column, not a popup,
so it never covers what you are choosing an action for. It has its own
filter: with a dozen actions configured, type `git` to narrow to the git
ones. Actions can also carry their own key, `alt-<letter>` or
`ctrl-<letter>`, and fire straight from the picker — add
`keybinding = "alt-s"` to an action and `Alt-S` runs it without opening
the pane — the reference config binds most of its actions this way, so
`Alt-S` is status, `Alt-G` greps, `Alt-E` edits. `Tab` itself is
reserved for the pane and `Ctrl-C` belongs to the picker; claiming
either, or the same key twice, is refused at load rather than silently
resolved.

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
# scout shell integration (canonical copy; ships with the product).
#
# Why this exists: a child process cannot cd its parent shell, so scout
# actions PRINT commands and this function evals them in your shell. The
# eval is guarded: only allowlisted line shapes run (cd / printf /
# $EDITOR-$VISUAL invocations); anything else is shown, never executed.
#
# The printed command goes through a temporary file (`--print-to`) rather
# than a captured stdout, so that in session mode a test runner or REPL
# started by an action keeps stdout for itself. A one-shot run and a
# session both write at most one command; a session that ends with Esc
# writes nothing and nothing is evaluated.
#
# Install: source this file from your shell rc, e.g.
#   source /path/to/scout/shell/scout.bash
#
# Bare `scout`, `scout --session` and `scout -s` run the picker;
# subcommands (index, query, doctor, recon, open-db) pass through untouched.
scout() {
  case "${1:-}" in
    ''|--session|-s)
      local f out line rc
      f="$(mktemp "${TMPDIR:-/tmp}/scout.XXXXXX")" || return 1
      command scout "$@" --print-to "$f"
      rc=$?
      out="$(cat "$f" 2>/dev/null)"
      rm -f "$f"
      [ "$rc" -ne 0 ] && return "$rc"
      [ -z "$out" ] && return 0
      while IFS= read -r line; do
        case "$line" in
          'cd '*|'printf '*|'${EDITOR'*|'${VISUAL'*) ;;
          *) printf 'scout: refusing to eval unexpected output: %s\n' "$line" >&2; return 1 ;;
        esac
      done <<< "$out"
      eval "$out"
      ;;
    *)
      command scout "$@"
      ;;
  esac
}
```

## Configuration

Config lives at `$XDG_CONFIG_HOME/scout/config.toml`. Start from
[`examples/config.toml`](examples/config.toml) — the installer
deliberately does not copy it for you, because your first config goes
through the trust prompt.

### Loading the example actions

The reference config ships 14 actions, most of them on an `Alt-`
chord. To adopt them:

```sh
mkdir -p ~/.config/scout
cp examples/config.toml ~/.config/scout/config.toml
scout            # shows the trust prompt; read the actions, answer y
```

If you already have a config, that copy replaces it — diff first if you
have edits worth keeping. Either way the **next interactive launch
prompts again**, because the file is hashed and any change re-prompts;
that is the design, not a nuisance. There is no reload command and none
is needed: scout reads the config at startup, so the next run has your
edits.

`scout doctor` tells you which config actually loaded, whether it is
trusted, and how many actions are active — start there if an edit does
not seem to have taken.

### Writing your own

Actions are declarative TOML: a name, a keybinding, and a list of steps
that either spawn a process or print a command for your shell to run.

`{env.NAME}` resolves **only** against variables an earlier `env` step in
the same action set — never against your shell's environment. A
reference to something unset fails the step rather than quietly
expanding to whatever you happened to export. To use a shell variable,
reference it in a `print` template and let your own shell expand it.
Placeholders (`{path}`, `{parent}`, `{name}`, `{query}`, and friends)
are quoted at the print seam, so a directory called `proj $(rm -rf ~)`
is a filename, not an instruction. A template that needs a placeholder
expanded *unquoted* is refused unless the action sets
`unsafe_shell_template = true` — which means you have read it and
accepted that a hostile filename becomes shell syntax.

`{repo_root}` is the one placeholder that can be undefined for an
otherwise valid selection: it walks up from the selection looking for a
`.git` entry, and if there is none it fails the step rather than
guessing. An action built on it — `root`, `edit-repo`, `status`, `log`
and `diff` in the reference config — therefore does nothing outside
any repository, and says what to do instead. That is the intended
behaviour: falling back to the plain directory would run `git status`
somewhere you did not ask for. Give such actions a description that
names the requirement, since the action pane is where you pick one.

On first run — and on every subsequent change to the file — scout shows
you the actions it is about to trust and asks for confirmation. It needs
a TTY to ask; if there is no terminal it refuses rather than trusting
silently, and tells you to use `scout query` instead.

Without any config, compiled-in defaults apply: Enter opens the
selection in `$VISUAL`, else `$EDITOR`, else the first of `vi`, `vim`,
`nano` found on your `PATH` — **when you run the binary directly**
(`command scout`). If your editor is none of those three, set `$EDITOR`
— the compiled-in fallback list is deliberately short rather than a
survey of every editor.

Under the shell wrapper — the recommended setup — use
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
a script. It never prompts, and it never modifies, migrates or repairs:
it will not create a database, re-trust a config, or rebuild a corrupt
index, because a diagnostic that fixes things destroys the evidence you
called it to see. (SQLite may create transient `-shm`/`-wal` sidecars
while it *reads* a WAL database — any reader does, and refusing that
would mean reporting on a database scout cannot fully see.) The
environment section prints a fixed short list of variables and never
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
