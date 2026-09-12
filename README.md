# scout

A terminal project finder and action launcher, and a scout in the plain
sense: it also looks at the ground it indexes and tells you who else can
edit it.

Type a few characters, get your projects ranked by fuzzy match blended
with how often and how recently you use them, press a key, and something
happens: your shell lands in the project, your editor opens, the tests
run. Offline, one binary, no network. The index and the frecency store are
one SQLite file; the config is portable TOML meant to live in your
dotfiles.

## Install

```sh
git clone <this-repo> && cd scout
./install.sh                                       # builds, installs to ~/.local/bin
printf '\n# >>> scout shell integration >>>\nsource %s\n# <<< scout shell integration <<<\n' \
  "$PWD/shell/scout.bash" >> ~/.bashrc              # guarded eval wrapper, marked so recon can find it
```

Or take a musl release tarball (x86_64 / aarch64): binary, shell snippet,
reference config.

Then index a tree and run the picker:

```sh
scout index ~/projects
scout
```

`scout index <path>` adds a tree; index as many as you like. A path
inside a tree you already indexed walks that tree again; a path that
would contain one is refused (forget the inner tree first, or keep it).
`scout index` alone walks every tree again the way it was walked before,
which is how you refresh; paths that have gone away are tombstoned when
the walk completes and purged after 182 days. `scout index --forget
<path>` drops a tree.

## The picker

The frame is gold for a one-shot `scout` and burgundy for a session, so
the mode is visible before you read the title; matched characters are
sea green and the selected row ivory.

Results lead with the **name**. Searching `photo-store` returns the
directories called `photo-store`, not the hundred files inside one; a name
that *is* your query outranks a longer name that merely contains it.
Location appears only as far as it disambiguates: two projects both
called `api` show as `api  billing` and `api  storefront`. A marker
column separates a git repository (`⑂`) from a directory (`‣`) from a
file, and shows `❢` on a path where recon found something (below).
Matched characters are highlighted; rank is the order.

| key | does |
|---|---|
| type | filter; `Left`/`Right`, `Home`/`End`, `Backspace`, `Delete` edit the text |
| `Up`/`Down` | move the selection |
| `Enter` | run the default action |
| `Tab` | open the action pane: a filterable column of the actions that apply here |
| `alt-<letter>`, `ctrl-<letter>` | run the action bound to that chord |
| `?` (empty search) | show the keys, including your own bindings |
| `Esc`, `Ctrl-C` | quit |

The picker draws on stderr; stdout is reserved for commands printed for
your shell.

### Sessions

By default an action ends scout. `scout --session` (or `-s`, or
`[scout] session = true` in the config) keeps you in the picker: a
spawned program gets the terminal for its lifetime, scout holds the screen
with one status line until you press a key, and the picker returns with
your query and selection intact. The title reads `scout: session` so you
always know which mode you are in. Actions that print a command for your
shell (`cd`) can only work after scout is gone, so they end the session
and are marked `⏎`; a session wants its own actions instead, and the
reference config gives it a set on the same keys (`when = { mode =
"session" }`): Enter opens a shell pane at the selection, `alt-c` is the
one action that leaves and moves your shell.

A session brings its own panes. With tmux installed, `scout -s` from a
plain shell starts a tmux session on scout's own server (`tmux -L scout`;
your `~/.tmux.conf` still applies) and lands you in the picker inside it.
A spawn step with `pane = "split-right"` runs in a new pane and scout
stays in its own; the `[keys]` table binds `split-right`, `split-down`,
`new-window`, `focus-left/right/up/down`, `close-pane`, `zoom`,
`focus-picker`, `kill-pane` and `reindex` to keys you choose (defaults:
`alt-right`, `alt-down`, `alt-w`, `alt-shift-<arrow>`, `alt-x`, `alt-z`,
`alt-h`, `alt-q`, `ctrl-r`); on scout's own server the focus and zoom
keys work from every pane, `focus-picker` brings you back, and
`kill-pane` closes the pane you are in (never the picker). When you `cd` into a
project, or press Esc, scout detaches and your shell gets its prompt
back; everything you started in a pane keeps running, and the next
`scout -s` re-attaches to it with a fresh picker. Scout never kills a
session or a server, and never a pane on its own: `close-pane` closes the
last pane scout opened and `kill-pane` the pane you press it in, both at
your keystroke. `tmux -L scout kill-server` ends it all. Already inside tmux,
scout uses the session you are in. Without tmux the session still works,
minus panes, and `?` says so. `[scout] tmux = "never"` (or `--no-tmux`
for one run) keeps tmux out of it; `"require"` refuses to run without it.
The `?` overlay names every key your terminal delivers while it is open,
so a binding that does not fire can be diagnosed.

## Shell integration

A child process cannot `cd` its parent shell, so scout *prints* commands
and a shell function evals them, behind an allowlist: only `cd`,
`printf`, and `$EDITOR`/`$VISUAL` line shapes ever execute. The command
travels through a temporary file rather than a captured stdout, so
programs you start from a session keep their stdout.

The canonical copy is [`shell/scout.bash`](shell/scout.bash):

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

## Configuration in one page

Config lives at `$XDG_CONFIG_HOME/scout/config.toml`. Start from
[`examples/config.toml`](examples/config.toml); the reference config ships 41
actions across navigation, editing, git, search, plumbing, one section per
ecosystem (Rust, Node, Python, Go, Make and just) that is offered only
where that ecosystem's marker file exists, and the recon fixes. The
navigation, editing, git, search and plumbing actions come in pairs with
the same name and key: a `print` one for a one-shot `scout` and a session
one that opens a pane or runs and returns, told apart by
`when = { mode = "one-shot" }` and `{ mode = "session" }`.

```toml
schema_version = 2

[[action]]
name = "cargo-test"
keybinding = "alt-t"
when = { marker = "Cargo.toml" }                      # offered only where the file exists
steps = [ { kind = "spawn", argv = ["cargo", "test"], cwd = "{repo_root}" } ]

[[action]]
name = "go"
keybinding = "enter"
steps = [ { kind = "print", format = "cd {path} 2>/dev/null || cd {parent}" } ]
```

Steps are `spawn` (an argv, run directly), `print` (a line for your
shell) or `env` (bindings for later steps in the same action).
Placeholders `{path} {parent} {name} {ext} {repo_root} {home} {query}
{env.VAR}` are single-quoted when printed, so a directory called
`proj $(rm -rf ~)` is a filename, not an instruction. `when` takes `kind`,
`marker`, `ext`, `glob` and `finding`, ANDed. The first time you run scout
with a config, and every time the file changes, it shows you the actions
and asks before trusting them; without a terminal it refuses instead.
`schema_version = 1` files are refused with a note saying what to change.
The full reference is [`docs/configuration.md`](docs/configuration.md).

## Recon

```sh
scout recon                       # who else can edit what you indexed
scout recon ~/projects/api        # one tree
scout recon --format tsv --fail-on critical
scout recon accept ~/shared world-writable-dir --reason "team scratch dir"
scout recon baseline ~/projects/api   # record its Makefile, package.json, hooks...
scout recon --since-last          # only what appeared since the previous run; exit 1 if any
scout index ~/projects --no-recon # skip the cheap checks for this tree
```

Recon reports ownership and mode problems (world- or group-writable,
owned by someone else, setuid, setgid), credential-shaped files
(`.env`, `*.pem`, `id_*`, ...) that others can read, links leading into
system trees or out of your home, files another user changed in the last
day, ACLs that widen a mode, and entry points that changed since you
recorded a baseline. The cheap checks run on every walk, and credential
files are examined even when hidden files are not indexed or git ignores
them. It judges permission bits, never file contents. It stores what it
finds beside the index, marks the row in the picker, lets you accept an
exception that lapses when the underlying fact changes, and offers fixes
as actions you run deliberately. It never repairs anything itself. It
also looks at scout's own files, including the rc file that sources the
shell wrapper. Details in [`docs/security.md`](docs/security.md).

## Other commands

| command | does |
|---|---|
| `scout query <q> [--limit n] [--format tsv] [--print0]` | rank and print, best first; exits 1 on no match; piped output is byte-exact |
| `scout doctor [--format tsv]` | the state scout resolved: config, trust, index, environment; read-only |
| `scout open-db <path>` | open an index, print its vitals, recover it if needed |
| `scout index [<path>] [--hidden\|--no-hidden] [--follow\|--no-follow] [--no-recon\|--recon] [--forget]` | walk a tree into the index, or every indexed tree again; flags are remembered per tree |

Machine-readable output puts the unconstrained field last, so a tab in a
path cannot shift a column: split on the first N tabs and take the rest
whole.

## When something goes wrong

Start with `scout doctor`. It prints which config won, whether it is
trusted, what the index holds (each tree with its flags and the walk it
last completed) and how your environment reads, never
changes anything, and prints only a short fixed list of environment
variables so the output is safe to paste. For more, `SCOUT_LOG=debug
scout index ~/projects` (levels: `off error warn info debug trace`; the
picker logs to `$XDG_STATE_HOME/scout/scout.log`).

## Files scout owns

| path | contents |
|---|---|
| `$XDG_CONFIG_HOME/scout/config.toml` | your actions; portable |
| `$XDG_DATA_HOME/scout/index.db` | index, frecency, recon findings (SQLite, WAL, 0600) |
| `$XDG_STATE_HOME/scout/trusted-config.sha256` | hashes of the configs you approved |
| `$XDG_STATE_HOME/scout/scout.log` | picker log |

## Licence

MIT.
