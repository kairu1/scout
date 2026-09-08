# scout

A Rust terminal project finder and action launcher. One binary, offline,
no network. `scout index <root>` walks a tree into a SQLite index;
`scout` opens a picker that ranks paths by fuzzy match blended with
frecency; a keypress runs a declarative action from a TOML config. Read
`README.md`, then `docs/architecture.md`, `docs/security.md` and
`docs/configuration.md`; they are short and written for a human.

## Fixed ground

These decisions are settled. Refine how they are implemented; do not
reopen them.

- Rust, single binary, no network surface. SQLite (bundled, WAL) is the
  index and the frecency store; `scan_generation` advances only on a
  completed walk and readers filter to the current generation.
- Ranking: continuous-decay frecency (7-day half-life) blended with a
  `nucleo-matcher` score under `tanh`, `k_match = 25 × query_chars`, a
  coverage-scaled basename term. Visits are credited only on action
  execution, one per (path, 10 s). The calibration test and the three
  named-directory search tests must keep passing.
- Config is portable TOML. Declarative actions; no interpreter, ever.
- Trust: a config is hashed on a canonical projection of its action set;
  prompt on first sight and every change; no TTY means refuse.
- Execution is argv-only. A shell parses a scout-templated string at
  exactly two seams: the `print` step (every placeholder single-quoted,
  NUL and newline refused) and an attested `sh -c`.
- Refuse at the boundary: `O_NOFOLLOW` config, 256 KiB cap, parse errors
  halt; the index refuses NUL, newline, `/proc`, `/sys`, `/dev`, paths
  over 4 KiB; the DB and trust store are 0600 under 0700, owner-checked.
- Display: every string reaching a terminal passes one strip rule; layout
  is measured in columns; every glyph scout emits is declared in
  `src/ui/glyph.rs` and width-guarded.
- `doctor` and `recon` report and never repair.
- The dependency roster is closed: adding a crate is a recorded decision.
  `cargo deny` and `cargo audit` are hard failures; fewer than 150
  shipped-target crates.
- Machine-readable output puts the unconstrained field last; `scout
  query` exits 1 on no match; piped output is byte-exact.
- No real project name anywhere in the repository. Anonymise to shapes
  (`photo-store` beside `photo-store-admin`).

## Conventions

- `src/` is a system, not an org chart. Each module's top comment says
  what it owns, what it refuses to know about, and what it exposes.
  `platform/` is the only place for `std::os::unix`, `signal_hook` or
  `extern "C"`; a test enforces it.
- One error type, `scout::Error`, with context-carrying variants;
  `main.rs` is the one place an error becomes a stderr line and an exit
  code. Tests assert variants, never error prose.
- Tests live beside the behaviour they guard, named for the behaviour.
  Every new guard is observed failing once before it is trusted.
- Doc comments explain in plain English; they cite no decision numbers.
- No emoji in code or docs. Ornament glyphs go in `glyph.rs`.

## Gates

Run all of these before calling anything done; CI runs the same set.

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --locked --all-targets
cargo deny check
cargo audit
cargo tree --prefix none | sed 's/ .*//' | sort -u | wc -l   # fewer than 150
cargo test --locked --release --test index -- --ignored      # 100k-path gate
cargo test --test tmux -- --ignored                          # tmux end-to-end, needs tmux
```

The reference config's trust hash is pinned in `tests/parity.rs`; a
change to `examples/config.toml` or to the canonical projection is a
deliberate edit to that literal, and a projection change also bumps the
header in `src/config/canonical.rs` so every user re-approves once.

The musl cross-compile smoke for x86_64 and aarch64 runs in CI. Every
CI job carries `timeout-minutes`.

## Working method

Deliberation before code: a short council paper per portfolio, then one
decision record per decision (context, decision, rejected alternatives,
what it binds), then one builder implementing against the records. All of
it lives in `.findings/` at the repo root, which is gitignored and never
committed: `council/`, `decisions/`, `verification/`, and `log.md`, a
dated, terse, append-only engagement log that records mistakes as well as
progress. Read `.findings/decisions/` before changing anything a decision
binds; append to `log.md` as you go.

Work outside the current directive is proposed in `.findings/questions.md`
first and built after a ruling. A small, well-reasoned exception is still
an exception, and is logged as one in `log.md`.

Git: you never write to the remote. Commit locally on `main`; the user
pushes. The old history is on `legacy/v0.2` and must never be rewritten.
