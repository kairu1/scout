# Changelog

## 0.4.3

A session gets its own actions.

### Added
- `when = { mode = "session" | "one-shot" }`: an action offered only in
  a session, or only in a one-shot `scout`. Two actions may share a name,
  a chord or `enter` when their modes are disjoint. `scout doctor` counts
  each mode's actions. Enters the trust hash; a config without the key
  hashes as before.
- A `spawn` step with `pane` may have an empty `argv`: the pane is a
  shell at `cwd`. `{dir}`: the selection when it is a directory, else its
  parent.
- The reference config pairs every navigation, editing, git, search and
  plumbing action with a session version on the same key: Enter opens a
  shell pane at the selection, `alt-r` one at the repository root,
  `alt-k` lists there, the git views and searches run and return, the
  editor runs here (`alt-e`) or in a pane (`edit-here`), and `leave`
  (`alt-c`) is the one action that ends the session with a `cd`. The
  trust prompt appears once after copying it.

### Changed
- `sh -c` needs `unsafe_shell_template` only when a placeholder is inside
  the `-c` text. `sh -c '… "$1"' sh {path}` passes it as a positional
  parameter and loads without attestation; the parameters themselves
  are held to the single-slot rule like any other argv element.
- A `[keys]` entry clashes only with a chord a session can offer;
  `scout doctor` checks Enter for each mode and reports what each run
  offers.

## 0.4.2

A hardening release from an independent review of everything since
0.3.0, plus a palette.

### Added
- A palette: gold frame for a one-shot `scout`, burgundy for a session,
  mocha lines, ivory selected row, sea green matches; on scout's own tmux
  server the status bar is sea green, pane borders mocha, the active
  border burgundy, the current window gold. One source for both.
- `scout index` refuses a path that is missing, relative or not a
  directory; a remembered tree whose directory is gone is reported as
  not present and leaves the previous index serving; walk flags need a
  path.
- `?` names keys in every mode; `[keys]` warns when an explicit entry
  takes another operation's default.
- CI runs the tmux story; ten facts that live in code and docs have
  parity guards.

### Fixed
- A directory named like tmux format syntax (`x #(cmd)`) could run
  `cmd` when a pane opened at it: every `-c` value is escaped.
- An interrupted re-walk hid the rows it had touched until the next
  completed walk, and its generation number could be reused.
- Forgetting a tree, or a walk tombstoning a path, left their findings in
  every report.
- Re-attach failed when a window in the session was named `scout`; two
  launchers racing on a cold server left one in-process.
- An index with rows but no completed root read as ready and served
  nothing; a scoped `scout recon <path>` reset the `--since-last` clock.
- `pane-run` now uses the sanitised PATH, receives `env` bindings, and
  strips what it prints; symlinked rc files are followed; the re-index
  writer opens without running recovery on the live index; detached
  children are reaped; every way out of a launched picker unbinds its
  keys, withdraws the handed-over file and detaches.

## 0.4.1

### Added
- `[keys] focus-picker` (default `alt-h`) focuses the picker from any
  pane; `[keys] kill-pane` (default `alt-q`) closes the pane you press
  it in and refuses the picker's. On scout's own tmux server the focus
  keys, `zoom`, `focus-picker` and `kill-pane` are installed as tmux
  bindings while the picker runs, so one set of keys works everywhere.

### Fixed
- The tmux client scout execs is told the terminal takes UTF-8 (`tmux -u`),
  so a shell with no UTF-8 locale no longer gets `_` for the picker's
  corners.
- The `?` overlay describes a key it cannot name by what arrived, so a
  terminal that sends Alt+letter as an 8-bit character is diagnosed on
  the spot.

## 0.4.0

### Added
- A session brings its own panes: `scout -s` outside tmux, with tmux
  installed, starts a tmux session on scout's own server (`tmux -L
  scout`, your `~/.tmux.conf` still read) and runs the picker inside it;
  an exit action or Esc detaches and returns your shell; the next
  `scout -s` re-attaches with a fresh picker and hands over its own
  return file. `[scout] tmux = "auto" | "never" | "require"`,
  `tmux_session`, `tmux_server = "private" | "shared"`; `--no-tmux` for
  one run. Scout never kills a session or a server.
- The index holds many trees: `scout index <path>` adds one, `scout
  index` alone walks every tree again the way it was walked (flags are
  remembered per tree; `--no-hidden`, `--no-follow`, `--recon` undo
  them), a tree inside another is refused, `--forget` drops one. `ctrl-r`
  in a session re-walks every tree.
- Credential files are examined on every walk even when hidden entries
  are not indexed or git ignores them; such rows never reach the picker.
- `scout recon --since-last`: only findings first seen after the previous
  run; exit 1 when one reaches `--fail-on`.
- Recon finds the shell wrapper in your rc files by the installer's
  marker line and flags an rc file others can edit.
- The `?` overlay names every key pressed while it is open.
- The search caret moves by grapheme cluster.
- CI runs the ACL positive case with `setfacl`, and the tmux story test.

### Changed
- The cheap recon checks run on every walk; `--no-recon` opts a tree
  out and `--recon` opts it back in (both remembered per tree).
- Index schema 3 (`roots`, `paths.root_id`, `paths.candidate`;
  `run_state.last_root` dropped). A 0.3 index migrates on open: its one
  tree becomes the first root.
- The shell wrapper is unchanged; under tmux `command scout` returns
  when the client detaches, always with code 0, and the returned file is
  the whole contract. The installer's suggested rc line is now a marked
  block.
- Print sinks are opened without following symlinks.

## 0.3.0

A rebuild in place: same tool, new history (the prior history through
0.2.1 is on the branch `legacy/v0.2` and under its tags), reorganised
codebase, two new capabilities.

### Added
- `scout recon`: reports who else can edit the indexed paths and what is
  exposed by its permissions (ownership, modes, setuid/setgid, credential
  files, symlink escapes, ACL presence, changed entry points, scout's own
  files), grouped by severity, human or `--format tsv`, exit 1 on
  unaccepted findings at or above `--fail-on`. Findings are stored in the
  index; `scout recon accept` records an exception that lapses when the
  fact changes; `scout recon baseline` records a project's entry points;
  `scout index --recon` runs the cheap checks during the walk. The picker
  marks flagged rows and asks before running an action on one. Fix actions
  ship in the reference config, gated on the finding; recon never repairs.
- Conditional actions: `when = { kind, marker, ext, glob, finding }`; the
  action pane shows only what applies and a chord to an inapplicable
  action says why.
- Session mode: `scout --session` / `-s` / `[scout] session = true` keeps
  the picker open across actions; in-process children own the terminal
  and scout pauses after them (`pause = false` to skip); `ctrl-r`
  re-indexes the last tree without leaving; exit actions are marked.
- tmux: `pane = "split-right" | "split-down" | "new-window"` on a spawn
  step, and a `[keys]` table binding `split-right`, `split-down`,
  `new-window`, `focus-*`, `close-pane`, `zoom`, `reindex` to keys of your
  choice, translated to tmux underneath and argv-only.
- Reference config: per-ecosystem sections (Rust, Node, Python, Go, Make,
  just) gated by marker files, plus recon fixes.
- `scout doctor`-style discipline for recon; `--print-to` for the wrapper.

### Changed
- Config schema is 2; a `schema_version = 1` file is refused with a note
  on what to change. The trust hash format is v2, so every user
  re-approves their config once.
- The shell wrapper reads the printed command from a temporary file
  (`--print-to`) instead of capturing stdout, so programs started from a
  session keep their stdout. The eval allowlist is unchanged.
- The index tombstones paths a completed walk did not see (the mechanism
  the documentation had described) and purges tombstones after 182 days.
- The one-credit-per-path-per-ten-seconds rule lives in the row, so it
  holds across processes and sessions.
- The codebase is a library with a thin binary, domain modules with stated
  scope, one error type, and a `platform` module holding every OS-specific
  line; tests are organised by module. `--help` text no longer cites
  internal decision numbers.

### Fixed
- The trust store was briefly wider than 0600 between creation and chmod.
- The 100k-path performance gate is now run by CI (its own release-profile
  job) rather than only documented.

## 0.2.1 and earlier

See the `legacy/v0.2` branch.
