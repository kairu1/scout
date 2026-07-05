# OPORD — Active Operation Order

**Phase:** 3 — Main Assault: Search, TUI, Actions
**Status:** ISSUED.
**Issued:** 2026-07-05
**Signed:** Chief of Staff, under commander goal directive 2026-07-05 ("get scout working")
**Previous phase:** 2 — First Engagement — CLOSED 2026-07-05 (merged to main; HANDOFF "Phase 2 green").

---

## 1. Situation

Phase 2 shipped the index foundation: ADR-001 schema, WAL discipline, streaming indexer, visit primitive, crash recovery. 19 tests green; 100k smoke gate passed. `main` carries the merge.

## 2. Mission

Three line officers engage, each in a worktree on their sector branch:

- **1st Rifles — `sector/search`, `src/search/**`.** Ranking-aware search per ADR-001: `nucleo-matcher` behind a `Matcher` trait; `ranking::blend` with tanh soft-normalisation (`K_match = 100`, `K_frec = 10`, weights 0.6/0.4); zero-query mode ranks by decayed frecency; candidate set = non-tombstoned rows of the current generation; total tie-break order (visits_total desc, path byte length asc, lexicographic, rowid). Generation counter honours `ipc::QUERY_ACTIVE`.
- **Engineers — `sector/actions`, `src/config/**`, `src/actions/**`.** ADR-004 TOML loader (discovery chain, staged validation, closed placeholder set, single-slot rule, keybinding stub); canonical-JSON projection + SHA-256 trust prompt per ADR-003 §3 (hash implementation hand-rolled — no hashing crate on the roster); action executor: spawn/print/env, sequential-only, on_failure policies, POSIX-single-quoting on print, undefined-env abort, `unsafe_shell_template` attestation, PATH sanitisation, setsid for wait=false; visit credit via `index::record_visit` (first-success-wins, 10 s per-path rate limit). Compiled-in defaults `edit` and `print-path`.
- **3rd Rifles — `sector/tui`, `src/ui/**`.** ratatui+crossterm: query bar, streaming ranked results, empty-index/partial-scan banners as render states, action menu, Enter dispatch, control-byte strip at the render boundary, trust prompt outside the alt-screen.

`src/main.rs` wiring (clap CLI: default TUI, `index`, `open-db`) lands with the final sector merge.

## 3. Execution

Admitted crates (full ADR-002 roster now open to line officers): clap, crossterm, ratatui, nucleo-matcher, serde, serde_json, toml, anyhow, thiserror, plus the Phase 2 set. Nothing else.

Merge order: search, actions, tui — later merges rebase-free via --no-ff into main after each sector's tests pass.

## 4. Success criteria (Phase 3)

- [ ] `cargo test` green across all sectors.
- [ ] End-to-end: index a real tree, query returns ranked results, Enter (or `--pick`-equivalent non-TTY path) executes the resolved action, visit credit lands, exit code honours the chain rule.
- [ ] Config on disk drives behaviour: user action overrides compiled default by name; trust prompt fires on first load and on hash change; non-TTY refuses untrusted config.
- [ ] Empty index renders banner+hint, exit 0.
- [ ] HANDOFF carries "Phase 3 green"; sector branches merged --no-ff.

When green, signal **"Phase 3 green"**. Phase 4 (packaging/portability) remains commander-gated.
