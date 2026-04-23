# Position Paper — Intelligence (S2): Ecosystem Recon

**Author:** council-intel
**Date:** 2026-04-23
**Phase:** 1, Wave 1
**Portfolio:** Ecosystem recon — what the adjacent tools do, where the gaps sit, what to steal, what to avoid.

---

## 1. Terrain survey

**fzf** (Junegunn Choi, Go). De facto interactive fuzzy finder. Wins on ubiquity, speed, shell-integration keybindings (Ctrl-T, Alt-C, Ctrl-R), and the `--preview` / `--bind` extension contract. Stateless by design — no ranking memory across invocations. Config lives in env vars and shell rc, which fights portability. A filter, not a launcher: "action" is whatever the caller does with stdout.

**skim (sk)** (lotabout, Rust). Rust-native fzf-alike. UX parity, slightly better tmux story, but matcher quality trails `nucleo` and maintenance cadence is thinner. Same statelessness as fzf.

**zoxide** (ajeetdsouza, Rust). Canonical frecency implementation for directories. Visit-count × recency-decay, persisted in a compact binary store, shell hooks for every major shell. Scope is deliberately narrow: it only ranks directories you have already `cd`'d into — unvisited projects are invisible. No action composition; its verb is "cd".

**autojump** (William Ting, Python). zoxide's intellectual ancestor. Frecency idea was correct; Python dependency and single-maintainer bus factor were fatal. Textbook case for our decade-longevity gates.

**broot** (Canop, Rust). Interactive tree-navigator with a **verbs** system: named actions defined in portable TOML (`conf.toml`), accepting template holes like `{file}` and `{directory}`. Closest existing analogue to what SCOUT's action schema must be. Weakness: no cross-project frecency; the tree metaphor is for drilling into one directory, not for "find any project across disk".

**yazi** (sxyazi, Rust). Async TUI file manager. Excellent UX, rich preview pipeline. Scope is file-management. Plugin system is Lua — smuggles a full interpreter into the supply chain, a cost we should not pay.

**fd** (sharkdp, Rust). Path finder with smart defaults (respects `.gitignore`, parallel walk). Non-interactive; idiomatically piped into fzf. Its real contribution is demonstrating the `ignore` crate + parallel walker.

**ripgrep** (BurntSushi, Rust). Gold standard for content search. Not our competitor; our **supply line** — `ignore` is the walker to adopt for Phase 2.

**telescope.nvim** (nvim-telescope, Lua). Editor-bound, architecturally instructive: separate *picker* (enumerate), *sorter* (rank), *previewer* (show). Composition over monolith. Locked inside Neovim and paying Lua startup cost.

**Raycast** (Raycast Inc., Swift, macOS-only). Polished "launcher + argument + action" schema, extensions, chained actions. Closed-source, macOS-only, opaque settings DB — violates portability outright. Its trigger / argument / action separation is still the right conceptual frame.

**Also noted.** `nucleo` (Helix's matcher crate — direct competitor to `fuzzy-matcher`, stronger matching and parallelism; already flagged for Quartermaster). `fasd` (abandoned; merged frecency across files and dirs — instructive failure). `projectile` (Emacs) — "project" as a first-class concept with registered actions.

---

## 2. The gap SCOUT fills

No tool in the adjacent ecosystem combines all four of the commander's requirements simultaneously:

1. **Frecency over *indexed* paths, not just *visited* paths** — zoxide only sees what you cd into; SCOUT must index projects and then weight them by visits.
2. **Action composition driven by portable TOML** — broot has verbs; Raycast has actions; neither ships config you can commit to dotfiles and expect to work on every machine.
3. **CLI-native, editor-agnostic, OS-agnostic** — telescope requires nvim; Raycast requires macOS; fzf is a filter, not a launcher.
4. **Persistent ranked state that survives machines** — no tool combines a SQLite-backed index with a committed TOML config such that `clone dotfiles && install && scout` reproduces behaviour.

The intent ("fast, composable, portable") picks out exactly this empty quadrant.

---

## 3. Patterns to steal

- **zoxide's frecency math.** Visit-count times a tiered exponential decay (roughly: same hour = 4×, same day = 2×, same week = 0.5×, older = 0.25×). The tiers, not the formula surface, are the real artifact. Architect should confirm the constants in ADR-001.
- **broot's verb schema.** `[[verbs]]` blocks with `invocation`, `execution`, and `{file}` / `{directory}` template holes. This is a direct template for ADR-004.
- **ripgrep/fd's `ignore` crate + parallel walker.** Quartermaster already flagged `ignore`; adopt the `WalkBuilder::build_parallel` pattern for Phase 2's streaming indexer.
- **nucleo over fuzzy-matcher.** Better-tuned bonuses (prefix, camelCase, path separators), first-class SMP. The Helix team's maintenance is active. Quartermaster should confirm.
- **telescope's picker/sorter/previewer split** as *architecture*, not as code — clean seams between "what to enumerate", "how to rank", "what to preview" pay off when Phase 3 adds action-picker overlays.
- **fzf's Unix-citizen contract.** `--print0`, stdin/stdout discipline, `--expect` for bind keys. SCOUT's `--print` flag for shell integration should inherit this ethos.
- **Raycast's trigger / argument / action separation** as a schema lesson — not as UI.

## 4. Patterns to avoid

- **Plugin runtimes in interpreted languages.** yazi's Lua, telescope's Lua, IntelliJ-style JVM plugins — each is an interpreter you drag along, a supply-chain surface, and a decade-rot risk. SCOUT's actions should be declarative TOML, not scripted.
- **Single-maintainer, interpreted-language tools** (autojump, fasd). Bus factor and runtime drift killed them. Our decade-longevity gates exist to refuse this class of dependency.
- **Stateless finders as sole UI** (fzf-only stacks). Push ranking into brittle shell glue; cannot persist frecency.
- **Config in shell rc or env vars** (fzf's `FZF_DEFAULT_OPTS`). Portable means TOML in dotfiles, not lines in `.zshrc`.
- **Editor- or OS-coupling** (telescope, Raycast). Violates portability by construction.
- **Unbounded shell templates** (Raycast-style "run this string"). Without explicit template holes and shell-safe interpolation this is a command-injection vector — flagged for council-security.
- **Scope drift toward file-management** (yazi territory). SCOUT finds projects and acts on them; it is not a file manager.

## 5. Recommended reading

- zoxide source, `src/db.rs` and the rank/age logic — frecency constants.
- `ignore` crate docs, particularly `WalkBuilder::build_parallel` and gitignore layering.
- `nucleo` README and benchmarks vs `fuzzy-matcher`; Helix's matcher-switch rationale.
- broot's `conf.toml` reference and the verbs chapter of its book.
- fzf man page — `--bind`, `--expect`, `--preview`, `--print0`.
- Telescope's notes on pickers/sorters/previewers.
- ripgrep README benchmarks — sets indexing expectations.
- `fasd` and `autojump` post-mortems — evidence for the decade-longevity doctrine.

---

**Key claim:** SCOUT's defensible ground is the empty quadrant where *indexed-path frecency*, *portable-TOML action composition*, and *editor/OS-agnostic CLI* intersect — a quadrant every adjacent tool brushes against but none occupies.
