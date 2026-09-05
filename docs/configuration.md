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
| `spawn` | `argv` (list of strings, required), `wait` (default true), `cwd` (default `{home}`), `pause` (default true), `pane` (`split-right`, `split-down`, `new-window`) | runs the argv directly, no shell. `wait = false` detaches it with null stdio. In a session, `pause = false` skips the "press any key" hold after the child exits (set it on editors). `pane` runs the command in a tmux pane when scout is inside tmux and in-process otherwise; it cannot be combined with `wait = true` or `pause`. |
| `print` | `format` (required) | writes a line for the shell wrapper to eval after scout exits. Always ends a session. |
| `env` | `set` (table, at least one entry) | binds values that later steps in the same action can use as `{env.NAME}` and that later children inherit. All-or-nothing: if one value fails to expand, none land. |

One to 32 steps per action, run in order. Under `abort` the first failure
stops the chain; under `continue` every step runs and the exit code is 2
if none succeeded. A visit is credited to the path on the first
successful step.

## Placeholders

`{path}` `{name}` `{parent}` `{ext}` `{repo_root}` `{home}` `{query}`
`{env.NAME}`. `{{` and `}}` write literal braces. An unknown placeholder
refuses the file at load. `{ext}` is undefined on a directory and
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
`["grep -rn {query}"]`.

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

The action pane lists only actions that apply to the selection. A chord
bound to an action that does not apply does nothing and the footer says
why. Two actions may not share a chord even if their clauses exclude each
other; give each ecosystem its own letter.

## `[scout]`

`session = true` makes every run a session (as `scout --session` does).
It is the only key the table accepts.

## `[keys]`

Named operations bound to keys, in the grammar `<mods>-<key>`: modifiers
from `ctrl`, `alt`, `shift`; key a letter, `f1`-`f12`, `left`, `right`,
`up`, `down`, `home`, `end`, `pageup`, `pagedown`. A bare letter cannot be
a binding (it types into the search).

| operation | default | does (inside tmux, in a session) |
|---|---|---|
| `split-right` | `alt-right` | open a pane to the right at the selection |
| `split-down` | `alt-down` | open a pane below at the selection |
| `new-window` | `alt-w` | open a window at the selection |
| `focus-left/right/up/down` | `alt-shift-<arrow>` | move focus |
| `close-pane` | `alt-x` | close the last pane scout opened |
| `zoom` | `alt-z` | zoom the current pane |
| `reindex` | `ctrl-r` | walk the last indexed tree again without leaving (works outside tmux too) |

A `[keys]` entry may not use a key the picker owns (`Esc`, `Enter`,
`Tab`, `Ctrl-C`, plain arrows, `Home`, `End`, `Backspace`, `Delete`, `?`,
`a`) or a key an action binds; two operations may not share a key. A
default that collides with an action chord yields to the action with a
warning at load. Outside tmux the pane operations are absent from the help
overlay. The `?` overlay shows your bindings and the name of the last key
your terminal delivered, for terminals that eat some combinations.

## Trust

Every field above that changes what runs, whether it runs, or which key
runs it is part of the trust hash: name, keybinding, `on_failure`,
`unsafe_shell_template`, every step and its fields, `when`, `[keys]`.
`description` and `[scout] session` are not. On first sight and on every
change scout shows the actions and asks; without a terminal it refuses.
The trust store is `$XDG_STATE_HOME/scout/trusted-config.sha256`, one
`v2 <hash> <config path>` per line, 0600.

## Migrating from schema 1

Set `schema_version = 2`. Every v1 action is a valid v2 action unchanged.
New and optional: `when`, `pause`, `pane`, `[keys]`, `[scout] session`.
The trust prompt appears once more because the hash format changed.
