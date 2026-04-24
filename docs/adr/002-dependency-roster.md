# ADR-002 — Dependency Roster

- **Status:** Accepted
- **Authored:** council-quartermaster
- **Date authored:** 2026-04-24
- **Reviewers:** council-architect, council-security, council-surgeon, council-intel
- **Signed by commander:** 2026-04-24

## Context

Commander's intent fixes three constraints that bear directly on supply: portable across machines (no system libraries we cannot vend), fast (no runtime we do not need), and decade-longevity (no dependency we cannot defend in year ten). The six decade-longevity gates in `ops/CAMPAIGN.md` are the scoring instrument; this ADR applies them and fixes the roster.

Four position papers constrain the decision:

- Quartermaster (`positions/council-quartermaster.md`) scored twenty candidates against the six gates, proposed an eleven-entry shortlist, flagged gate-C ("1.0+ or stated stability") as the single interpretive liberty, and named five outright rejections.
- Architect (`positions/council-architect.md`) fixed the runtime shape as synchronous workers plus channels, named `nucleo-matcher` as the scoring primitive, `ratatui`+`crossterm` as the TUI backend, and declared a strict module DAG that the dependency graph must mirror.
- Security (`positions/council-security.md`) foreclosed embedded scripting runtimes, argued the supply-chain vector is what dependency gates exist to close, and deferred advisory discipline to this ADR.
- Surgeon (`positions/council-surgeon.md`) required `signal-hook` for SIGINT/SIGWINCH and `tracing` for structured observability, and pinned crash-log paths under `$XDG_STATE_HOME` that `dirs` resolves.

Commander's Wave 2 directives (HANDOFF 2026-04-23 23:23) fix three calls normative on this ADR: (1) bless the gate-C re-reading explicitly in Rationale, not in a footnote; (2) drop `arboard` from v1 and with it any `copy` step in ADR-004; (3) name the eleven-entry shortlist as the authoritative v1 roster — any addition requires its own ADR. This ADR resolves all three.

## Decision

We will ship SCOUT v1 with exactly the following direct dependencies, pinned by major (or minor, for 0.x-line crates that treat minor as major). Any addition beyond this list requires a successor ADR signed by the commander — it is not a Cargo.toml chore.

| # | Cargo crate | Pinned line | Purpose | Decade-longevity justification (one line) |
|---|---|---|---|---|
| 1 | `rusqlite` | `0.31` (with `bundled` feature) | SQLite FFI for the index and frecency store | Decade-old de facto Rust/SQLite binding; `bundled` statically links SQLite itself (also decade-stable), removing a system-library portability risk. |
| 2 | `clap` | `4` (with `derive` feature) | CLI parsing | `clap-rs` org, multi-maintainer, 1.0+ series; every credible alternative (`argh`, `lexopt`, `pico-args`) loses on ecosystem gravity. |
| 3 | `ignore` | `0.4` | Parallel path walker respecting `.gitignore` | Ships inside `ripgrep`; BurntSushi + a ripgrep-family bus factor; re-implementing gitignore semantics is a decade-of-bugs trap. |
| 4 | `crossterm` | `0.27` | Cross-platform terminal backend for `ratatui` | `crossterm-rs` org; the only backend `ratatui` supports that keeps Windows on the table without forking. |
| 5 | `ratatui` | `0.26` | TUI rendering primitives | Active successor to archived `tui-rs`; multi-maintainer org; no competing Rust TUI crate at parity. Gate A marginal (fork ~2.5y) but all other gates pass; see §Rationale. |
| 6 | `nucleo-matcher` | `0.3` | Fuzzy scoring primitive | Powers Helix's picker; active maintenance by the Helix team; swaps cleanly for `fuzzy-matcher`, which is stagnant. |
| 7 | `serde` | `1` (with `derive`) | Serialisation trait layer for config and any machine-readable output | dtolnay + org, 1.0+ frozen contract; functionally stdlib-adjacent; no credible alternative. |
| 8 | `serde_json` | `1` | JSON for test fixtures and `--print` machine-readable mode | Same author/org as `serde`, same 1.0+ contract; drop only if no `--print` JSON mode ships — tracked in ADR-004. |
| 9 | `toml` | `0.8` | Portable config parser | `toml-rs` org; decade of use; long-running 0.x with stable parsing contract. |
| 10 | `anyhow` | `1` | Application-level error type at the binary boundary | dtolnay, 1.0+ frozen; the idiomatic pairing with `thiserror` for binary/library split. |
| 11 | `thiserror` | `1` | Library-level error derive inside `scout`'s modules | dtolnay, 1.0+; narrow derive-macro surface; pin to `1` and defer any `2.x` bump to a successor ADR. |
| 12 | `tracing` | `0.1` | Structured event and span instrumentation | `tokio-rs` org; 0.1 line is stable by explicit commitment; Surgeon's observability minimum needs spans, not `log`'s flat record. |
| 13 | `tracing-subscriber` | `0.3` | Subscriber impl for stderr + `$XDG_STATE_HOME` file sink | Ships with `tracing`, same org, same cadence; hand-rolling a subscriber is wasted effort. |
| 14 | `signal-hook` | `0.3` | SIGINT / SIGWINCH / SIGUSR1 handling | vorner + maintainers; narrow API effectively frozen; Surgeon's crash-recovery and counter-dump stories depend on it. |

The roster is fourteen Cargo entries grouped into the eleven portfolio slots of the Quartermaster position paper (§2): `serde + serde_json`, `anyhow + thiserror`, and `tracing + tracing-subscriber` count as one slot each because they are co-versioned sibling crates whose adoption decision is singular. We will treat the eleven-slot shortlist as the unit of change — adding `serde_yaml` alongside `serde` is an ADR; bumping `serde_json` to match `serde` is not.

**Stdlib substitutions.** We commit, effective immediately, to using `std::thread::available_parallelism()` in every place where a worker-pool sizing decision is made; `num_cpus` does not enter the dependency graph. Rationale in §Alternatives.

**Dirs resolution.** `$XDG_CONFIG_HOME`, `$XDG_STATE_HOME`, and `$HOME` are resolved by hand in `src/config/paths.rs` using `std::env::var` with documented POSIX fallbacks. We reject the `dirs` crate (position paper marked it HOLD) in favour of a ~40-line module, because our needs are trivially small and cross-platform surface beyond Linux/macOS is not a v1 requirement. Windows support is deferred to a later ADR; at that point either `dirs` or an equivalent ships alongside.

## Rationale

### Gate-C blessing — stated up front

Commander's directive is accepted and codified here, not relegated. **Gate C in this ADR reads: "SemVer 1.0+, OR a public stability commitment observed over ≥3 years of real releases."** Strict "1.0+ only" is not workable: `rusqlite`, `ignore`, `crossterm`, `ratatui`, `nucleo-matcher`, `toml`, `tracing-subscriber`, and `signal-hook` all run perpetual 0.x lines while behaving more stably than many crates in the 1.x bucket. Reading gate C literally would either exclude these — emptying the roster of most of what Rust actually ships — or force us into worse alternatives that happen to have chosen 1.x marketing. Every entry in the table above that is 0.x has been admitted under this reading with an explicit stability track record. `toml-edit` and `notify`, by contrast, fail the same re-reading: their 0.x is load-bearing churn, not numbering accident, so they are rejected in §Alternatives.

### Why these, and not more

Each crate earns its slot by replacing work we would otherwise build and maintain ourselves. `ignore` is the single largest weight saver: we are not reimplementing gitignore parsing, parallel walk, or symlink-cycle detection. `rusqlite` + `bundled` replaces both a system-library dependency and the FFI layer around it. `nucleo-matcher` gives us the scoring primitive that Architect's ranking blend needs, with active maintenance that `fuzzy-matcher` lacks. `tracing`+`tracing-subscriber` is the smallest thing that gives Surgeon the structured logs and spans §Consequences requires. `signal-hook` gives us signal plumbing with a correctness story (reentrancy, unix-signal edge cases) we do not want to re-derive.

Every slot that does not appear is a slot we chose to pay with stdlib or hand-rolled code. The test: if re-implementing the crate's contribution would take less than ~200 lines and carry no subtle correctness hazard, we do not take the crate. That is the rule that kills `dirs`, `once_cell`, `lazy_static`, `chrono` (for our timestamp needs), and in many places `regex`.

### Transitive ceiling and advisory discipline

Target **< 120 transitive crates** at v1. Exceeding this cold-builds too slowly on a fresh machine to honour commander's "clone dotfiles → install → works day one" checkpoint in Phase 4. Pioneers owns the measurement (`cargo tree --duplicates` and `cargo tree --target all | wc -l`) and the enforcement: a dependency bump that pushes us over 120 without a Quartermaster sign-off is a release blocker.

`cargo-deny` and `cargo audit` must be wired into CI from Phase 4 preflight. `cargo-deny` enforces licence allow-list (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Unicode-DFS-2016, Zlib) and fails on duplicate versions beyond a small allow-list. `cargo audit` fails the build on any RustSec advisory against a crate in the direct roster. This is the tripwire — wired before we need it, not after.

### MSRV

Pin MSRV in `rust-toolchain.toml` at the toolchain current at Phase 2 engagement. Do not chase stable. Review on every minor bump of `clap`, `tracing`, `serde`, or `rusqlite` — these are the historical vectors for silent MSRV inflation. **VERIFY** at Cargo.toml freeze: confirm each shortlist crate's declared MSRV is ≤ the pinned toolchain. If any is higher, we downgrade that crate or raise MSRV deliberately, not by accident.

### Cross-compile story

`rusqlite` with `bundled` + musl (`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`) is the load-bearing cross-compile combination for portability. Smoke-test before Phase 4 on the Pioneers build runner. If bundled+musl fails to link, we do not drop portability — we file an incident and hold the release.

### Commitment to the `num_cpus` swap

`std::thread::available_parallelism` has been stable in the standard library since Rust 1.59 (released 2022-02) and covers every sizing decision SCOUT needs to make: search-worker pool size, indexer parallel-walk width. It returns `Result<NonZeroUsize, io::Error>`; SCOUT treats the error case by defaulting to a single worker and logging at `warn`. That is the correct failure mode anyway — a machine that cannot report its parallelism is a machine where spinning up many workers is a gamble.

We do not adopt `num_cpus`. We will audit the transitive graph at Phase 4 preflight and, where `num_cpus` appears as a transitive, accept it as irreducible — we control only direct deps. Any direct use of `num_cpus` by SCOUT code is a review-time reject.

**The strongly-justified case** (preserved for honesty): if Phase 3 discovers a need for *physical*-core counts distinct from logical (e.g., to avoid SMT oversubscription for a very specific workload), stdlib does not provide that and `num_cpus::get_physical()` does. We do not anticipate this need — ranking and walking are both memory-bound enough that logical cores is the right sizing knob — but if the case appears, it comes back as a revision to this ADR, not a quiet Cargo.toml edit.

### Rejected crates (explicit, with reason)

| Crate | Verdict | Reason |
|---|---|---|
| `num_cpus` | **SWAP → stdlib** | `std::thread::available_parallelism` covers every use case we have; one less dep, one less advisory-risk surface. See above. |
| `fuzzy-matcher` | **SWAP → `nucleo-matcher`** | Single-maintainer, stagnant, thinner scoring heuristics (no native path-separator or camelCase bonuses); gate B fails, gate E (replaceability) easy via nucleo. |
| `tui` (original) | **AVOID** | Archived. `ratatui` is the active fork. |
| `structopt` | **AVOID** | Superseded by `clap` derive; no new-code reason to adopt legacy. |
| `directories` | **AVOID (over `dirs`)** | Same information as `dirs` with larger API surface; and we reject even `dirs` in favour of a hand-rolled XDG resolver — see Decision. |
| `dirs` | **AVOID (hand-rolled)** | Our needs are ~40 lines of `std::env::var` plus documented fallbacks; adding a crate for this fails the ~200-line test. |
| `tokio` / `async-std` / `smol` | **AVOID for v1** | Architect's pipeline is synchronous workers plus channels. An async runtime drags `mio`, reactor wiring, and executor choice for zero gain. Revisit only if a long-running watcher lands post-v1. |
| `notify` | **AVOID for v1** | Live-reindex on FS events is out of scope. `notify`'s churn history (gate C literal-reading still fails under our re-reading) does not warrant inclusion until the feature is real. |
| Lua / Rhai / Deno / any embedded interpreter | **AVOID outright** | Intel named this a class failure; Security named it a supply-chain and injection surface. Actions are declarative TOML per ADR-004. Not for v1, not for v2 without a redesign. |
| `lazy_static` / `once_cell` | **AVOID** | `std::sync::OnceLock` and `std::sync::LazyLock` (stable on our MSRV — VERIFY at freeze) cover both. |
| `chrono` | **AVOID by default** | Large timezone surface, spotty advisory history. `SystemTime` covers every timestamp SCOUT stores. If a human-readable formatter becomes necessary, prefer `jiff` (BurntSushi) or `time` — but in a new ADR. |
| `reqwest` / any HTTP client | **AVOID** | SCOUT is offline-first. Any network feature is a new ADR. |
| `regex` | **AVOID unless needed** | Not a default. If ranking or the indexer develops a concrete regex need, `regex` (BurntSushi, 1.0+) is the obvious choice — but we do not pre-import it. |
| `arboard` | **AVOID — commander-dropped from v1** | HANDOFF 2026-04-23 23:23 drops the `copy` step from ADR-004; without a `copy` step, there is no clipboard surface. If clipboard returns, so does this decision. |
| `toml_edit` | **AVOID** | Round-tripping TOML with comment preservation is not a v1 requirement. The read-only `toml` crate suffices. |
| `crossbeam-channel` | **AVOID by default** | `std::sync::mpsc` covers Architect's query pipeline (single-producer-per-key, single-consumer per worker). If Phase 3 discovers a multi-consumer or `select`-style need, `crossbeam-channel` (tokio-rs, 1.0+) is the pre-approved swap candidate — but remains outside the roster until needed. |

## Alternatives considered

### Alt A — A strictly 1.0+ roster

Excise every 0.x crate and replace with 1.x alternatives. The outcome is either a dramatically smaller feature set (no `rusqlite` bundled? no `ratatui`? no `ignore`? no real TUI?) or substitution into 1.x crates that are younger or thinner than what we rejected. Rust's de facto standards are disproportionately 0.x-forever; a strict gate-C reading misunderstands the ecosystem. Rejected — commander's directive blesses the re-reading.

### Alt B — Adopt `nucleo` (full) instead of `nucleo-matcher`

The full `nucleo` crate ships a picker alongside the matcher. We already own the pipeline (`ipc` module, generation counter, partial-render protocol). Adopting the picker doubles the surface we import and creates a silent dependency on `nucleo`'s UX decisions — which may diverge from Architect's. `nucleo-matcher` is the scoring primitive alone, which is what we need. Rejected.

### Alt C — Take `dirs` / `directories` for XDG resolution

Saves ~40 lines. Costs one more direct dep, one more advisory-risk surface, and the opportunity cost of a review the first time `dirs` bumps major. Our needs are small enough that the hand-rolled module is more defensible than the dep. Rejected.

### Alt D — Take `num_cpus`

The historical, well-known crate. It has a decade of use and passes most gates. But `std::thread::available_parallelism` is stdlib and was designed specifically to replace `num_cpus::get` — that is the API contract. Gate F (proximity to stdlib) fires: when stdlib subsumes a crate's API, we prefer stdlib. Rejected. See §Rationale "Commitment to the `num_cpus` swap".

### Alt E — Defer the licence, MSRV, and audit discipline to Phase 4

Wiring `cargo-deny` and `cargo audit` later is cheap mechanically but expensive as policy: the rules bite hardest when they catch the first real violation, and "we'll put the tripwire in after the next release" is how projects ship advisories. Rejected — policy is pre-committed here; Pioneers executes in Phase 4 preflight.

## Consequences

**Binds Pioneers (Phase 4).** Cargo.toml must be written exactly once, with exactly the fourteen entries above, at the pinned lines declared. CI must run `cargo audit`, `cargo-deny check`, and `cargo tree --duplicates`; the first two as hard failures, the third as a warning that escalates past the 120-transitive ceiling. Cross-compile smoke test for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` with `rusqlite` `bundled` is a Phase 4 preflight gate.

**Binds 2nd Rifles (Phase 2).** Only `rusqlite` (with `bundled`) and `tracing` (+ `tracing-subscriber` for init) are admitted in the index/DB sector. `signal-hook` enters at Phase 2 for SIGINT discipline during indexing. Any other import is rejected in review.

**Binds 1st Rifles / 3rd Rifles / Engineers (Phase 3).** Line officers may `use` only crates from the roster. Introducing a new direct dep requires a HANDOFF to `@commander` and a successor ADR — not a Cargo.toml edit. Transitive deps are the Quartermaster's inventory, not Ritles'.

**Binds Engineers on config (Phase 3).** The hand-rolled XDG resolver lives at `src/config/paths.rs` under the Engineers sector. Engineers owns correctness on `$XDG_CONFIG_HOME` / `$XDG_STATE_HOME` fallbacks. This is a sector decision downstream of this ADR.

**Binds ADR-004.** `arboard` is out; therefore no `copy` step kind. Commander's HANDOFF is the normative source; this ADR records it so ADR-004 does not re-litigate.

**Binds this Council.** Every addition to the roster is a new ADR, not a PR comment. Every rejection here is a standing refusal until revised by successor ADR.

**Dependencies on other ADRs.** ADR-001 (Ranking) depends on `nucleo-matcher` (slot 6) and `rusqlite` (slot 1) — both confirmed here. ADR-003 (Threat Model) defers supply-chain advisory discipline to this ADR — satisfied by the `cargo audit` / `cargo-deny` commitment in §Rationale. ADR-004 (Action & Config schema) depends on `toml` and `serde` — confirmed; and is constrained to schemas the roster can parse without additional deps.

## Reviews

_Appended by peer reviewers._

> **council-intel, 2026-04-24 — endorsement**
>
> Eleven-slot shortlist matches the ecosystem map Intel filed: `ignore` from ripgrep, `nucleo-matcher` from Helix, `rusqlite`-bundled as the decade-stable SQLite seam, `tracing` and `signal-hook` for Surgeon's observability and signal discipline. The `fuzzy-matcher → nucleo-matcher` swap is the right call — Intel position §3 flagged it and Helix's active maintenance is the strongest bus-factor argument available in the matcher quadrant. Gate-C re-reading is the correct interpretive move; every Rust crate Intel named as a de facto standard runs a perpetual 0.x line and a strict reading would empty the roster. One non-blocking tidy crossing into ADR-003's text, not this ADR's: ADR-003 §4 cites `dirs` for the `$XDG_DATA_HOME` macOS fallback, but this ADR rejects `dirs` in favour of a hand-rolled resolver — Engineers should not be left chasing two answers at Phase 3.

> **council-surgeon, 2026-04-24 — endorsement**
>
> Roster serves the surgeon-lens requirements cleanly: `signal-hook` (SIGINT / SIGWINCH / SIGUSR1 counter dump per position §3 and §5), `tracing` + `tracing-subscriber` (structured events and the `$XDG_STATE_HOME/scout/panic-<epoch>.log` sink), and `rusqlite` with `bundled` (one fewer system-library failure mode on the recovery path that renames a corrupt DB and rebuilds). The hand-rolled XDG resolver over `dirs` is fine for surgeon — panic log, `scout.lock`, and the corrupt-DB rename target all resolve through `std::env::var` with documented POSIX fallbacks; the ~200-line rule cuts the right direction and keeps one fewer advisory-risk surface. Gate-C re-reading is load-bearing specifically for `tracing` 0.1 and `signal-hook` 0.3 — both are effectively frozen APIs with decade track records, and a literal reading would strip the observability and signal-handling primitives that the crash-recovery story (position §4) is built on. One non-blocking cross-ADR nit, seconding Intel: ADR-003 §4 still cites `dirs` for the macOS `$XDG_DATA_HOME` fallback — Engineers implementing the panic-log and trust-store paths must not chase two answers. No blockers; commander's intent (portability, decade-longevity) is served.

> **council-security, 2026-04-24 — endorsement**
>
> Roster is security-aligned end-to-end. `rusqlite` with `bundled` (slot 1) closes the system-`libsqlite3` supply surface ADR-003 §Dependencies names — an attacker-controlled `libsqlite3.so` on a compromised machine cannot swap into SCOUT's process. The five outright refusals — embedded interpreters (Lua / Rhai / Deno), `reqwest`, `notify` for v1, `arboard`, `chrono` — collectively close the injection, network, live-reload, clipboard, and timezone-parser vectors ADR-003 §1 and §Refused-outright depend on staying shut; specifically, dropping `arboard` removes the clipboard surface at the dep layer and dropping embedded scripting refuses the class of bug ADR-003 §Alternatives 4 argued against at the design layer. §Rationale's `cargo audit` + `cargo-deny` (MIT/Apache/BSD/ISC/Unicode/Zlib licence allow-list) + <120-transitive ceiling + MSRV pinning satisfy the supply-chain discipline that ADR-003 §Dependencies explicitly defers to this ADR — the tripwire is wired before we need it, not after, which is the posture ADR-003 requires. Gate-C re-reading is security-load-bearing: `signal-hook` 0.3 is what ADR-003 §5 and Surgeon's crash recovery depend on, and a literal gate-C would strip it. Two non-blocking cross-ADR tidies, thirding Intel and Surgeon: (1) ADR-003 §4 still cites `dirs` for the macOS `$XDG_DATA_HOME` fallback while this ADR's Decision rejects `dirs` in favour of the hand-rolled resolver at `src/config/paths.rs` — the seam is in my authored ADR-003 and I will align citation to the resolver on revision. (2) ADR-003 §3's inline canonical-action-set enumeration omits `keybinding` (included correctly in ADR-004 §9); same revision cycle, same document. No blocker here; commander's intent (portability, decade-longevity) served.

> **council-architect, 2026-04-24 — non-blocking**
>
> Roster honours every primitive the Architect ADRs assume: `nucleo-matcher` (slot 6) and `rusqlite` with `bundled` (slot 1) back ADR-001's ranking blend and frecency store; `toml` (slot 9), `serde` (slot 7), and `serde_json` (slot 8) back ADR-004's parser, typed validation, and canonical-JSON projection (§9) — no ADR-004 contract leans on a crate this ADR rejects. Module DAG in Architect position §4 (`ranking` ← `index` ← `search`; `config` ← `actions`; `ipc` ← `ui, search`) maps cleanly onto the roster: the `Matcher` trait's swap seam is one crate wide (slot 6), the `Store` trait's SQLite impl is one crate wide (slot 1), and no slot crosses the DAG sideways. Gate-C re-reading is load-bearing for my designs: `ratatui` 0.26, `crossterm` 0.27, `rusqlite` 0.31, `nucleo-matcher` 0.3, `toml` 0.8, `tracing-subscriber` 0.3, and `signal-hook` 0.3 all run perpetual 0.x; a literal reading empties the shortlist and the ADR-001 pipeline, the ADR-004 parser, and Surgeon's observability and signal-handling all lose their primitives — blessing the re-reading explicitly in Rationale up front is the correct shape. Non-blocking architectural note on the `crossbeam-channel` deferral (Rejected table): the Architect position §2 pipeline spawns N search workers that compete on a single `query_rx` — that is MPMC, which `std::sync::mpsc` does not support (mpsc is single-consumer). The pre-approval of `crossbeam-channel` as a swap candidate covers this, so this is not a blocker; surfacing now so Phase 3 1st Rifles does not encounter the swap as a surprise when implementing the `ipc` module. Cross-ADR tidy, seconding Intel, Surgeon, and Quartermaster: ADR-003 §4 still cites `dirs` for the macOS `$XDG_DATA_HOME` fallback while this ADR's Decision rejects `dirs` in favour of the hand-rolled resolver at `src/config/paths.rs` — Engineers must read one answer; Security has committed to the fix on revision. No blocker; commander's intent (portability, decade-longevity) served.

## Revision history

- 2026-04-24 — drafted by council-quartermaster.
- 2026-04-24 — signed by commander.
