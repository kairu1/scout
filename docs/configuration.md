# Configuration

One TOML file, searched in this order and taken at the first regular file
(a symlink is skipped; a file that fails to parse halts rather than
falling through):

1. `$XDG_CONFIG_HOME/scout/config.toml` (when the variable is set)
2. `~/.config/scout/config.toml`
3. `/etc/scout/config.toml`

Without a config, two compiled-in actions apply: `edit` on `Enter` (uses
`$VISUAL`, then `$EDITOR`, then `vi`, `vim` or `nano` on `PATH`) and
`print-path`. A user action with the same name replaces a default wholly.

```toml
schema_version = 2          # required; 1 is refused with a note on what to change

[scout]
session = true              # optional: stay in the picker after each action
tmux = "auto"               # optional: "auto" | "never" | "require"
tmux_session = "scout"      # optional: the tmux session a session lives in
tmux_server = "private"     # optional: "private" (tmux -L scout) | "shared"

[keys]                      # optional: pane operations (tmux) and re-index
split-right = "alt-right"
reindex     = "ctrl-r"

[[action]]
name = "cargo-test"                         # ASCII [A-Za-z0-9_-], 1-64
description = "cargo test at the repo root" # shown, never hashed
keybinding = "alt-t"                        # alt-<letter> | ctrl-<letter> | enter
on_failure = "abort"                        # abort (default) | continue
when = { marker = "Cargo.toml" }            # optional
steps = [ { kind = "spawn", argv = ["cargo", "test"], cwd = "{repo_root}" } ]
```

## Steps

| kind | fields | does |
|---|---|---|
| `spawn` | `argv` (list of strings, required; may be empty only with `pane`), `wait` (default true), `cwd` (default `{home}`), `pause` (default true), `pane` (`split-right`, `split-down`, `new-window`) | runs the argv directly, no shell. `wait = false` detaches it with null stdio. In a session, `pause = false` skips the "press any key" hold after the child exits (set it on editors). `pane` runs the command in a tmux pane when the session has tmux (scout starts one when it can) and in-process otherwise; it cannot be combined with `wait = true` or `pause`. With `pane`, `argv = []` opens a shell at `cwd` and nothing else; outside tmux that shell runs on this terminal instead (`$SHELL`, else `sh`), and a session returns to the picker when it exits. |
| `print` | `format` (required) | writes a line for the shell wrapper to eval after scout exits. Always ends a session. |
| `env` | `set` (table, at least one entry) | binds values that later steps in the same action can use as `{env.NAME}` and that later children inherit. All-or-nothing: if one value fails to expand, none land. |

One to 32 steps per action, run in order. Under `abort` the first failure
stops the chain; under `continue` every step runs and the exit code is 2
if none succeeded. A visit is credited to the path on the first
successful step.

## Placeholders

`{path}` `{name}` `{parent}` `{dir}` `{ext}` `{repo_root}` `{home}`
`{query}` `{env.NAME}`. `{{` and `}}` write literal braces. An unknown
placeholder refuses the file at load. `{dir}` is the selection when it is
a directory and its parent otherwise (the `cwd` for a command about the
selection). `{ext}` is undefined on a directory and
`{repo_root}` is undefined outside a git repository; a step that needs
one fails and says what to do instead. `{env.NAME}` resolves only
against an `env` step earlier in the same action, never your shell's
environment; to use a shell variable, write `$VAR` in a `print` template
and let your shell expand it.

In a `print` format every placeholder is single-quoted, so a directory
called `proj $(rm -rf ~)` is a filename. A template that needs a
placeholder expanded unquoted, inside `sh -c`, is refused unless the
action sets `unsafe_shell_template = true`, which means you have read it
and accepted that a hostile filename becomes shell syntax. In a `spawn`
argv, an element holding a placeholder may hold nothing else (no spaces,
no shell characters): `["grep", "-rn", "{query}", "."]`, not
`["grep -rn {query}"]`. When a spawn needs a shell (a pipe, an editor
fallback), hand the placeholder to `sh` as a positional parameter:
`["sh", "-c", "grep -rn \"$1\" . | head -60", "sh", "{query}"]`. The
text after `-c` is fixed and is all the shell parses; `$1` is data, so no
attestation is needed. A placeholder inside the `-c` text is the attested
shape.

## `when`

Where an action is offered. Keys are ANDed; an action without `when` is
offered everywhere; an empty table is refused (omit it instead).

| key | meaning |
|---|---|
| `kind = "repo" \| "dir" \| "file"` | the selection is a directory with a `.git` entry, another directory, or a file |
| `marker = "Cargo.toml"` or `["Makefile", "GNUmakefile"]` | one of these files exists at the selection (its parent, for a file) or at the repository root |
| `ext = ["rs", "toml"]` | the selection is a file with one of these extensions (no dot); false on a directory |
| `glob = "~/work/**"` | the canonical path matches; `*` does not cross `/`, `**` does; a leading `~/` is your home |
| `finding = "world-writable-dir"` | `scout recon` has an unaccepted finding with that check name on the row |
| `mode = "session" \| "one-shot"` | the run is a session (`--session`, `-s`, `[scout] session = true`) or a one-shot `scout` |

The action pane lists only actions that apply to the selection. A chord
bound to an action that does not apply does nothing and the footer says
why. Two actions may not share a chord even if their clauses exclude each
other (`mode` excepted, below); give each ecosystem its own letter.

`mode` is the one key settled per run rather than per selection: scout
drops the actions the run cannot offer before the picker draws. That is
also the one case where two actions may share a name, a chord or `enter`:
one `mode = "session"`, the other `mode = "one-shot"`, so the same key
does the session thing in a session (open a pane here) and the one-shot
thing otherwise (`cd` your shell here). An action without `mode` is
offered in both and clashes with either. The trust prompt and `scout
doctor` show the whole set; `doctor` also counts each mode's.

## `[scout]`

`session = true` makes every run a session (as `scout --session` does).

`tmux` says how a session gets its panes. `"auto"` (default): outside
tmux, with tmux installed, scout starts a tmux session and runs the
picker inside it; already inside tmux, it uses that session; without
tmux, the session runs in your terminal with no panes. `"never"` keeps
tmux out of it entirely (also `--no-tmux` for one run). `"require"`
refuses to run a session without tmux.

`tmux_session` names the session scout starts or re-attaches to: letters,
digits, `_` and `-`, up to 64 characters; anything else refuses the file
because tmux would rewrite it. Default `scout`.

`tmux_server` is `"private"` (default) for scout's own server, `tmux -L
scout`, which still reads your `~/.tmux.conf`, or `"shared"` for your
default server, where scout sets no options at all. On its own server
scout sets these in memory when it creates its session, never by writing
a file:

| option | value | why |
|---|---|---|
| `extended-keys on` | | modified keys reach the picker distinctly |
| `escape-time 10` | | no half-second hold on Esc |
| `status-style bg=#3E6E58,fg=#EDE9E3` | sea green, ivory | the status bar is scout's |
| `window-status-current-style fg=#C9A44C,bold` | gold | the current window's name |
| `pane-border-style fg=#A47864` | mocha | pane borders |
| `pane-active-border-style fg=#67192E` | burgundy | the focused pane's border |
| `message-style bg=#67192E,fg=#EDE9E3` | burgundy, ivory | tmux messages |

The picker paints from the same six colours: gold frame and cursor for a
one-shot `scout`, burgundy for a session, mocha lines, ivory for the
selected row, sea green for matched characters, and the finding marker
in whichever of gold or burgundy is not the accent. A terminal without
24-bit colour shows its nearest colours.

None of these enter the trust hash: they change where the picker runs,
not what runs.

## `[keys]`

Named operations bound to keys, in the grammar `<mods>-<key>`: modifiers
from `ctrl`, `alt`, `shift`; key a letter, `f1`-`f12`, `left`, `right`,
`up`, `down`, `home`, `end`, `pageup`, `pagedown`. A bare letter cannot be
a binding (it types into the search), and `shift` with a letter is
refused too: terminals send it as the capital letter.

| operation | default | does (in a session with tmux) |
|---|---|---|
| `split-right` | `alt-right` | open a pane to the right at the selection |
| `split-down` | `alt-down` | open a pane below at the selection |
| `new-window` | `alt-w` | open a window at the selection |
| `focus-left/right/up/down` | `alt-shift-<arrow>` | move focus |
| `close-pane` | `alt-x` | close the last pane scout opened |
| `zoom` | `alt-z` | zoom the current pane |
| `focus-picker` | `alt-h` | focus the picker's pane, from wherever you are |
| `kill-pane` | `alt-q` | close the pane you press it in; refuses the picker's pane (from the picker, Esc leaves) |
| `reindex` | `ctrl-r` | walk every indexed tree again, each the way it was walked, without leaving (works without tmux too) |

On scout's own tmux server (`tmux_server = "private"`) the focus keys,
`zoom`, `focus-picker` and `kill-pane` are also installed as tmux key
bindings while the picker runs, so the same keys work from any pane,
including the way back to the picker. `kill-pane` checks the pane's role
first and never kills the picker. Splits, windows and `close-pane` stay
the picker's. Nothing is installed on a shared server.

A `[keys]` entry may not use a key the picker owns (`Esc`, `Enter`,
`Tab`, `Ctrl-C`, plain arrows, `Home`, `End`, `Backspace`, `Delete`, `?`,
`a`) or a key an action binds; two operations may not share a key. A
default that collides with an action chord yields to the action with a
warning at load. Without tmux the pane operations are absent from the help
overlay, which says why. While the `?` overlay is open every key you press
is named in its last row (Esc or `?` closes it), so a binding that does
not fire can be checked against what your terminal delivers: a key the
grammar cannot name is shown as what arrived, and `the character '÷'` for
alt-w means the terminal sends Alt as an 8-bit character; turn on its
"Alt/Option sends Escape" (or "meta sends escape") setting. On a
scout-started server the alt-shift arrows arrive as distinct keys,
`ctrl-alt-<arrow>` is the fallback for a terminal that eats them.

## Trust

Every field above that changes what runs, whether it runs, or which key
runs it is part of the trust hash: name, keybinding, `on_failure`,
`unsafe_shell_template`, every step and its fields, `when`, `[keys]`.
`description` and the `[scout]` table are not. On first sight and on every
change scout shows the actions and asks; without a terminal it refuses.
The trust store is `$XDG_STATE_HOME/scout/trusted-config.sha256`, one
`v2 <hash> <config path>` per line, 0600.

## Migrating from schema 1

Set `schema_version = 2`. Every v1 action is a valid v2 action unchanged.
New and optional: `when`, `pause`, `pane`, `[keys]`, the `[scout]` table.
The trust prompt appears once more because the hash format changed.
