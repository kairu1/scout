# Architecture

scout is a library with a thin binary. `src/main.rs` parses arguments and
calls one function in `commands`; everything else is usable without the
CLI. Read the module tree top to bottom and you have the system.

## Modules

| module | owns | refuses to know about |
|---|---|---|
| `platform` | every OS-specific line: XDG resolution, `O_NOFOLLOW`, private (0600/0700) files, owner and mode, the effective uid, process spawning, SHA-256, time and ISO-8601, extended-attribute presence, the interrupt flag | what a config, index, action or terminal is |
| `locations` | where scout keeps its own files | how XDG is resolved |
| `index` | the SQLite file: open discipline, migrations, the streaming walker, the batched writer, frecency and the visit credit, crash recovery, writer pacing | ranking, config, terminals |
| `search` | candidate scope, the three index states, the ranking blend | schema details, drawing |
| `recon` | the checks, their evaluation over one `lstat`, the findings and exceptions tables, the baseline, the report | repairing anything, file contents |
| `actions` | the action model (`Action`, `Step`, `when`), the template grammar and its two shell seams, compiled defaults, the executor, the closed set of failure kinds | TOML, trust, terminals |
| `config` | the staged loader, the canonical projection the trust hash covers, the trust store, the `[keys]` table | executing anything |
| `doctor` | a read-only report of the state scout resolved | repairing, prompting |
| `ui` | the picker: state, event loop, layout, glyphs, the strip rule, the terminal | executing actions, the database |
| `tmux` | detecting tmux and translating named pane operations to argv | the picker, actions |
| `commands` | one function per subcommand, returning a contract exit code or an error | clap, `exit()` |
| `error` | the one error type, its exit code and its hint | printing |

A test enforces that only `platform/` imports `std::os::unix`,
`signal_hook` or declares `extern "C"`, so the rest of the crate is
portable by construction rather than by claim.

## The index

One SQLite file in WAL mode. `paths` holds every canonical path with its
frecency (`S`, a continuously decaying score with a seven-day half-life),
its visit count and its `scan_generation`. The generation is a single
counter that advances only when a walk completes; readers filter to the
current generation, so an interrupted walk leaves the previous index
serving. When a walk completes, rows it did not visit are tombstoned and
tombstones older than 182 days are purged; a path that returns is revived
with its history. The index holds one tree at a time: indexing a second
root retires the first.

The writer commits about a thousand rows per transaction and checkpoints
only when no query has run for half a second, so a live picker is never
stalled by a re-index in a session.

Migrations are embedded in the binary. Version 2 adds `findings`,
`exceptions`, `baseline`, a `worst_finding` summary column on `paths` and
`last_root` on `run_state`.

## Ranking

Match quality from `nucleo-matcher`, normalised with `tanh` against a
constant that scales with the query length, plus a basename term scaled
by how much of the name the query covers, blended with the decayed
frecency. Every norm is a pure function of the candidate and the query,
never of the result set, so the order is stable as results stream. Ties
break on visits, path length, bytes, rowid. A visit is credited only when
an action executes, at most once per path per ten seconds, and that
window is a predicate on the row itself so it holds across processes.

## Actions and the two shell seams

An action is a list of steps. `spawn` runs an argv directly; `print`
writes a line for the shell wrapper to eval after scout exits; `env`
binds values for later steps in the same action. Exactly two places let a
shell parse anything scout templated: the printed line, where every
placeholder is single-quoted and NUL or newline is refused, and an
explicit `sh -c` whose action carries `unsafe_shell_template = true`. A
`spawn` argv element holding a placeholder may hold nothing else. tmux
panes keep the rule: the command goes to tmux as separate arguments after
`--`, through `scout pane-run`, which runs it and then becomes the shell.

`when` clauses decide where an action is offered. Their inputs (kind,
marker files, extension, glob match, finding names) are computed once per
selected path and every clause is evaluated in memory, so the picker never
stats per frame.

## Sessions

The picker is an object that can be suspended and resumed. In a session
the command layer loops: pick, then either run the action in-process with
the terminal released to the child, run it in a tmux pane, or, for an
action with a `print` step, leave for good. A re-index runs on its own
thread with its own connection; completion arrives on a channel the event
loop polls, and the candidate set is swapped atomically.

## Display

Every string that reaches the terminal passes one strip rule (C0, C1,
DEL, bidirectional and zero-width controls, tag characters). Layout is
measured in terminal columns with `unicode-width`. Every ornament glyph is
declared in `ui/glyph.rs` and a test asserts it has the same width on a
CJK-locale terminal as on any other; a second test fails on any non-ASCII
literal elsewhere in `src/`.

## Gates

`cargo fmt --check`, `clippy -D warnings`, `cargo test --all-targets`,
`cargo deny check` (licences, duplicate versions, sources), `cargo audit`
on push and weekly, fewer than 150 shipped-target crates, the 100k-path
walk under 30 s as its own release-profile job, musl builds for x86_64
and aarch64, a timeout on every job. Adding a crate is a recorded
decision with a longevity argument; the current graph has 119 crates and
scout adds no crate for recon, sessions or tmux.
