# OPORD — Active Operation Order

**Phase:** 4 — Consolidation: Portability
**Status:** CLOSED 2026-07-05 — "Phase 4 green" filed in HANDOFF. Release tag + Phase 5 AAR await commander.
**Issued:** 2026-07-05
**Signed:** Chief of Staff, under commander goal directive 2026-07-05 ("add the preview pane then do phase 4")
**Previous phase:** 3 — Main Assault — CLOSED 2026-07-05 (HANDOFF "Phase 3 green"); post-close enhancements: TUI visual pass, preview pane (HANDOFF 2026-07-05 20:10 and Phase 4 entry).

---

## 1. Situation

The tool works end-to-end and is daily-drivable. Nothing about it is yet portable by the commander's checkpoint: no install artifact, no shell-integration snippet shipped by the product (the guarded wrapper lives only in the agent-container bootstrap), no CI tripwires, no MSRV pin, stale Phase 0 Containerfile.

## 2. Mission

Pioneers engages. Sector: `Containerfile`, `.github/**`, `install.sh`, `ops/**`, plus new `shell/**` and `examples/**` (assigned to Pioneers by this OPORD).

Deliverables, bound by ADR-002 §Consequences and ADR-003/004 packaging bindings:

1. **MSRV pin** — `rust-toolchain.toml` at the Phase 2 engagement toolchain (1.96.1); `Cargo.toml` gains `rust-version`; version becomes 0.1.0.
2. **CI tripwires** — `.github/workflows/ci.yml`: tests, clippy, `cargo audit` (hard fail), `cargo deny check` (hard fail; licence allow-list MIT/Apache-2.0/BSD-2/BSD-3/ISC/Unicode-DFS-2016/Zlib in `deny.toml`), transitive ceiling < 120 (warning that escalates), musl cross-compile smoke for x86_64 and aarch64 with `rusqlite` bundled.
3. **Release workflow** — `.github/workflows/release.yml`: tag push → musl artifacts for both arches.
4. **install.sh** — clone-and-run installer: builds release, installs to `~/.local/bin`, points at the shell snippet. MUST NOT drop a config into any discovery-chain location (ADR-003/004 binding: a shipped config would pre-trust itself).
5. **shell/scout.bash** — the guarded eval wrapper becomes a product artifact (canonical home; the agent-container bundle re-sources from here). Doctrine per HANDOFF 2026-07-05 19:05: actions print commands; wrapper allowlists line shapes.
6. **examples/config.toml** — reference config under `examples/` (allowed by ADR-003 §Consequences; never copied on install).
7. **Containerfile** — refreshed from the Phase 0 scaffold to a two-stage build+runtime image for the scout binary itself.
8. **README** — install, shell integration, status.

## 3. Constraints

- No new runtime dependency (ADR-002 roster is closed). Dev-tooling (cargo-audit/cargo-deny) runs in CI, not in the dependency graph.
- Environment note: this container has no root and no musl cross toolchain — the musl smoke runs in CI (its designed home, "the Pioneers build runner"); a local pass is not achievable and its absence is recorded, not hidden.
- Release itself (tag push, publishing) remains a commander act; Phase 4 ships the machinery.

## 4. Success criteria

- [ ] Fresh-HOME simulation: `install.sh` from a clean environment produces a working `scout` + wrapper flow (index, query, TUI dispatch) with zero manual steps beyond the trust prompt.
- [ ] `cargo audit` clean locally; `deny.toml` + workflows in tree and syntactically valid.
- [ ] Transitive crate count verified < 120.
- [ ] MSRV pin builds the workspace.
- [ ] agent-container bootstrap re-sources the wrapper from `shell/scout.bash` (no second canonical copy).
- [ ] HANDOFF "Phase 4 green" with any environment-limited items named.
