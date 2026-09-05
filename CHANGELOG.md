# Changelog

## 0.3.0 (unreleased)

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
