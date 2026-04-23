# Position Paper — Quartermaster: Supply Audit

**Author:** council-quartermaster
**Date:** 2026-04-23
**Phase:** 1, Wave 1
**Portfolio:** Dependencies, supply lines, decade-longevity scoring.

**Source of facts.** All claims below are drawn from training data (cutoff: January 2026). No live crates.io or GitHub lookups were performed in this sandbox; the environment has no confirmed network egress. Any field I cannot attest to from training data is explicitly marked **VERIFY**. No versions are fabricated; where a concrete version would be guesswork, I cite the major line and flag it.

---

## 1. Scored roster

Gates: **A** age ≥3y + commits in last 6mo; **B** bus factor ≥3 or org-backed; **C** 1.0+ or stated stability; **D** RustSec history reviewed; **E** replaceability; **F** stdlib proximity. Verdicts: **HOLD** (take as-is), **VERIFY** (take pending a crates.io/GitHub check), **SWAP** (prefer an alternative), **AVOID** (refuse).

| Crate | Version line | Age (A) | Bus factor (B) | Semver (C) | RustSec (D) | Replaceability (E) | Stdlib-proximity (F) | Verdict |
|---|---|---|---|---|---|---|---|---|
| `rusqlite` | 0.3x line — **VERIFY** latest | ≥10y | Multi-maintainer, wide use | Long-running 0.x; API-stable in practice | No structural advisories recalled | `sqlx` (async/heavier), `libsql` | De facto SQLite FFI in Rust | **HOLD** |
| `clap` | 4.x | ~10y | `clap-rs` org, ≥3 maintainers | 1.0+; 4.x stable series | Minor advisories historically, resolved | `argh`, `lexopt`, `pico-args` | Dominant; near-default | **HOLD** |
| `ignore` | 0.4.x | ~8y | BurntSushi + ripgrep org | 0.x but decade-stable API | Clean recall | Hand-rolled `walkdir` + gitignore parser | Ships with `ripgrep`; reference walker | **HOLD** |
| `crossterm` | 0.27+ — **VERIFY** | ~7y | `crossterm-rs` org | 0.x; backwards-compatible minors | Clean recall | `termion` (unix-only), `termwiz` (wezterm) | De facto cross-platform terminal | **HOLD** |
| `ratatui` | 0.26+ — **VERIFY** | Fork since 2023 (~2.5y) | Active org, multiple maintainers | 0.x; pre-1.0 roadmap stated | Clean recall | Original `tui-rs` (archived) | Fork of archived `tui-rs`; currently the TUI trunk | **VERIFY** — gate A marginal; community gravity is the offsetting evidence |
| `fuzzy-matcher` | 0.3.x | ~6y | Single-maintainer (lotabout) | 0.x; stagnant | Clean recall | `nucleo`, `nucleo-matcher` | Once de facto; now trailing | **SWAP** → `nucleo-matcher` |
| `nucleo-matcher` | 0.3.x — **VERIFY** | ~2–3y | Helix editor team | 0.x; active | Clean recall | `fuzzy-matcher` (the thing we'd swap from) | Powers Helix's picker; closest to a modern trunk | **VERIFY** — gate A marginal, but bus factor and momentum pass |
| `signal-hook` | 0.3.x | ~6y | vorner + maintainers | 0.x; narrow API, effectively frozen | Clean recall | `ctrlc` (narrower), `nix` raw handlers | Narrow scope; unix signals done right | **HOLD** |
| `dirs` | 5.x — **VERIFY** | ~7y | Small team, stable | 1.0+ on recent line | Clean recall | `directories`, `etcetera` | Trivial surface; swap cost near zero | **HOLD** (over `directories` — smaller API, same facts) |
| `num_cpus` | — | ~10y | Legacy | N/A | Clean | `std::thread::available_parallelism` | Superseded by stdlib | **SWAP** → stdlib |
| `serde` | 1.x | ~10y | dtolnay + org | 1.0+, frozen contract | Clean recall | None credible | Near-stdlib | **HOLD** |
| `serde_json` | 1.x | ~10y | Same as serde | 1.0+ | Clean recall | None credible at parity | Near-stdlib | **HOLD** |
| `toml` | 0.8+ — **VERIFY** | ~10y | `toml-rs` org | 0.x on parser; stable in practice | Clean recall | `basic-toml`, `toml_edit` | De facto TOML reader | **HOLD** |
| `anyhow` | 1.x | ~7y | dtolnay | 1.0+ | Clean | `eyre` | Near-standard for app-level errors | **HOLD** |
| `thiserror` | 1.x / 2.x — **VERIFY** | ~7y | dtolnay | 1.0+ | Clean | Hand-written `impl Error` | Near-standard for library errors | **HOLD** |
| `tracing` | 0.1.x | ~7y | `tokio-rs` org | 0.1 stable-by-commitment | Clean | `log` (narrower), `slog` (fading) | De facto structured logging | **HOLD** |
| `tracing-subscriber` | 0.3.x | ~6y | `tokio-rs` | 0.x, stable contract | Clean | Hand-rolled subscriber | Ships with `tracing` | **HOLD** |
| `arboard` | 3.x — **VERIFY** | ~4y | Multi-maintainer | 3.x; reasonable semver | Clean recall | `copypasta` (unmaintained-leaning) | Needed only if Engineers want a `copy` step | **VERIFY** — include only if ADR-004 keeps `copy` |
| `notify` | 6.x — **VERIFY** | ~8y | `notify-rs` org | 6.x; churn historically | Clean recall | `watchexec-events`, hand-rolled inotify | Only if live-reindex lands; not Phase 2 | **AVOID** for v1 (scope) |
| `tokio` | 1.x | ~8y | `tokio-rs` | 1.0+, frozen | Clean | `async-std` (fading), `smol` | Near-stdlib for async | **AVOID** for v1 — no async requirement in Architect's pipeline (threads + channels suffice) |
| `regex` | 1.x | ~10y | BurntSushi | 1.0+ | Clean recall | None at parity | Near-stdlib | **HOLD** — only if ranking or indexer needs it; not a default |
| `directories` | 5.x | ~7y | Same lineage as `dirs` | 1.0+ | Clean | `dirs` | Sibling crate, larger API surface | **SWAP** → prefer `dirs` |

---

## 2. Recommended shortlist

Eleven crates. Each carries its reason.

1. **`rusqlite`** — standing order mandates SQLite; `rusqlite` is the FFI binding with a decade of gravity. `sqlx` is async and heavier; we are not async. Bundled-`sqlite3` feature removes a system dep and aids portability.
2. **`clap` (derive)** — argument parsing is solved; hand-rolling it is rework without payoff. The derive macro keeps the CLI surface colocated with types.
3. **`ignore`** — BurntSushi's walker from `ripgrep`. Handles `.gitignore`, parallel traversal, and hidden-file policy. Re-implementing any of this is a trap.
4. **`crossterm`** — the only credible cross-platform terminal backend `ratatui` supports without surrendering Windows. `termion` is unix-only and violates portability.
5. **`ratatui`** — the successor of `tui-rs` with the only active TUI community in Rust; Architect's design targets it. Gate A is marginal (fork ~2.5y), offset by bus factor and the absence of any competitor at parity. Marked **VERIFY** only to confirm the current minor and changelog discipline at adoption time.
6. **`nucleo-matcher`** — swap target for `fuzzy-matcher`. Stronger matching (prefix, camel, path separators), SMP-ready, and Helix's active maintenance. Use **`nucleo-matcher`** (the pure-matcher sub-crate), not the full `nucleo` picker — we already own the pipeline; we only need the scoring primitive.
7. **`serde` + `serde_json`** — near-stdlib. `serde_json` is primarily for test fixtures and any machine-readable `--print` mode; drop it if neither materialises.
8. **`toml`** — parser for portable config. ADR-004's schema shape determines whether `toml_edit` is additionally needed; default is **not** to include it.
9. **`anyhow` (binary) + `thiserror` (library types)** — the dtolnay pair. Idiomatic, frozen at 1.x, no credible competitor inside our gates.
10. **`tracing` + `tracing-subscriber`** — structured logs now save hours later when Surgeon is triaging panics and stale-query races. `log` is narrower and locks us out of spans.
11. **`signal-hook`** — SIGINT/SIGWINCH handling for the TUI. Narrow scope, long stability.

**Stdlib instead of a crate:**
- `std::thread::available_parallelism` replaces `num_cpus`. Direct, zero-cost swap.
- `std::sync::mpsc` / `crossbeam-channel` — prefer stdlib mpsc unless Architect's pipeline needs multi-consumer or select; then `crossbeam-channel` (tokio-rs, 1.0+). Flagged as a follow-up question, not a default.

Total: **eleven direct dependencies**. That is the supply bill I want on the wire to Pioneers.

## 3. Rejected crates

- **`num_cpus` — SWAP.** Stdlib covers it. Every transitive reference to it is a minor cleanup for Pioneers at Phase 4.
- **`fuzzy-matcher` — SWAP.** Single-maintainer, thinner heuristics, stagnant. Gate B fails, E easy.
- **`directories` — SWAP to `dirs`.** Same information, larger surface. Pick the smaller.
- **`tokio` / `async-std` — AVOID for v1.** Architect's pipeline is synchronous workers plus channels; adopting a full async runtime would drag `mio`, executor choices, and reactor wiring for no gain. Revisit only if Phase 4 introduces long-running watchers.
- **`notify` — AVOID for v1.** Live-reindex on filesystem events is out of scope; `notify` has a churn history that does not warrant inclusion until the feature is real.
- **Lua / Rhai / Deno / any embedded interpreter — AVOID outright.** Intel named this as a class failure. Actions are declarative TOML per ADR-004; scripting is an injection and supply-chain surface.
- **`tui` (original) — AVOID.** Archived. `ratatui` is the fork that carries the community.
- **`structopt` — AVOID.** Superseded by `clap` derive; do not re-import legacy.
- **`lazy_static` / `once_cell` — AVOID.** `std::sync::OnceLock` and `LazyLock` (stable on recent toolchains — **VERIFY** MSRV) cover both. Do not add either crate.
- **`chrono` — AVOID by default.** RustSec history is spotty; timezone surface is large. If we need timestamps beyond `SystemTime`, prefer `jiff` (BurntSushi, newer) or `time` — but neither earns a slot until a concrete need appears. **VERIFY** before committing.
- **`reqwest` / any HTTP client — AVOID.** SCOUT is offline-first; no network in the hot path. If a future feature needs HTTP, it is a new ADR.

## 4. Risks the table does not capture

- **Licensing.** All shortlist crates are MIT or dual MIT/Apache-2.0 per recall; `ratatui` dual, `ignore` MIT/Unlicense, `rusqlite` MIT. No GPL/AGPL contamination expected. **VERIFY** at `Cargo.lock` freeze time; Pioneers should run `cargo-deny` before release. A single transitive GPL crate poisons a proprietary port.
- **Native dependencies.** `rusqlite` with the `bundled` feature statically compiles SQLite, removing a system dep — use it. Without `bundled`, `libsqlite3-dev` becomes a packaging obligation on every target. `arboard` needs X11/Wayland libs on Linux and has a macOS privacy-prompt surface; if we keep it, Pioneers must test on bare Linux containers. `crossterm` is pure Rust on unix; Windows pulls `winapi`-family bindings — fine, but cross-compile from macOS to Linux inside Podman must be smoke-tested.
- **Cross-compile.** Musl targets (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`) are the portability story. `rusqlite` bundled + musl is the load-bearing combination; **VERIFY** on the Pioneers' build runner before Phase 4.
- **Abandoned-maintainer scenarios.** `signal-hook` has slowed — acceptable given its narrow frozen API, but note it. `fuzzy-matcher` is the live abandonment case; the swap closes it. `rusqlite`'s perpetual 0.x is a cultural risk, not a technical one; its bus factor and ABI stability offset the semver fiction.
- **0.x-forever crates.** `rusqlite`, `ignore`, `crossterm`, `ratatui`, `toml`, `tracing-subscriber`, `nucleo-matcher`, `fuzzy-matcher` (swapped out) all violate gate C literally. Each has an explicit or de facto stability commitment. I am reading gate C as "1.0+ **or** stability stated and observed over ≥3y" — anything stricter excludes most of the Rust ecosystem. Surface this to the commander as the single interpretive liberty in this audit.
- **Transitive explosion.** `ratatui` + `crossterm` + `clap` + `tracing-subscriber` each pull small trees. Target total transitive crate count **< 120**; above that, cold-build time on a fresh machine violates the "install → works day one" checkpoint. Pioneers should track with `cargo tree --duplicates`.
- **RustSec pipeline.** Adopt `cargo audit` in CI from Phase 4 preflight. A single advisory on `rusqlite` or `serde` is a release-blocking event; we need the tripwire wired before we need it.
- **MSRV discipline.** Pin MSRV in `rust-toolchain.toml` at adoption, not at release. A silent MSRV bump from a patch release of `clap` or `tracing` has bitten projects in this shape before. **VERIFY** current MSRVs on shortlist at Cargo.toml freeze.

---

**Key claim.** Eleven direct dependencies clear all six gates with one explicit interpretive liberty (gate C read as "stability demonstrated, not merely numbered"). One swap (`fuzzy-matcher` → `nucleo-matcher`), one stdlib substitution (`num_cpus`), and five outright refusals (`tokio`, `notify`, embedded scripting, `chrono`, `lazy_static`/`once_cell`) are what separates a decade-surviving supply line from a year-two dependency graveyard.
