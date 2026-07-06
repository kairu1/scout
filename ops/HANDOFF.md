# HANDOFF — Async Comms Board

## Protocol

- Append-only. Never edit or delete another officer's entry.
- Format:
  ```
  ## YYYY-MM-DD HH:MM — FROM <callsign> TO <callsign|all|commander> — <subject>
  Body.
  ```
- Tags:
  - `@commander` — human-in-the-loop decision required; halt the thread.
  - `@cos` — Chief of Staff routing; non-blocking unless Chief flags.
  - `@all` — force-wide announcement.
- To mark a thread closed, prepend `**RESOLVED** YYYY-MM-DD` to the subject line.

---

## 2026-04-21 19:30 — FROM chief-of-staff TO commander — Phase 1 orders cut

**Phase 0 CLOSED 2026-04-21.** All checkpoints green; scaffold + fixes committed.

**Phase 1 ISSUED.** See updated `ops/OPORD.md` and the new runbook at `ops/phase-1-council.md`.

Phase 1 is the most important check-in of the campaign — four ADRs set doctrine for every downstream engagement. Four waves, four commander check-ins:

1. **Wave 1 — Position papers.** All five Council officers write a position paper on their portfolio. Parallel or sequential, your call. Output: five files under `docs/adr/positions/`.
2. **Wave 2 — ADR drafts.** Architect drafts ADR-001 + ADR-004. Quartermaster drafts ADR-002. Security drafts ADR-003.
3. **Wave 3 — Peer review.** Each ADR reviewed by the four non-authoring officers; authors revise.
4. **Wave 4 — Commander sign-off.** You read, approve or redirect.

**Before you move:** the runbook's preflight section covers Claude-Code auth inside the container — verify that first. If headless auth fails, the runbook gives two fallbacks (mount `~/.claude` or set `ANTHROPIC_API_KEY`).

**Where your two cents matters most:** Check-in #1 (before launching any officer — edit the prompts if any asks for the wrong thing) and Check-in #3 (ADR drafts before peer review — cheap to demand revisions here).

**Standing interrupt remains:** `stand down`, `redirect`, `promote`, `AAR now`.

Awaiting commander's execution of `ops/phase-1-council.md`.

---

## 2026-04-21 18:15 — FROM chief-of-staff TO commander — Smoke test triage (sqlite3 missing)

**Incident:** Step 5 smoke test — 6/7 green; `sqlite3 --version` failed with `command not found`.

**Root cause:** Containerfile installed `libsqlite3-dev` (compile headers) but not `sqlite3` (CLI package). They are separate apt packages; the dev package doesn't pull the CLI.

**Pattern noted — second miss of the same species this phase.** Both the `useradd` UID collision and this were failures to verify the Containerfile against its direct consumers. Carrying this into the Phase 0 AAR:
- **Standing rule for Pioneers sector (effective from Phase 4):** before shipping any Containerfile change, cross-reference the installed binaries against every command in the phase runbooks that the image supports. Treat the smoke test as authoritative.
- Chief of Staff will apply the same rule to any image artifact authored on the host.

**Fix applied:** `sqlite3` added to apt install list in Containerfile. Runbook troubleshooting table updated.

**Next action (commander):**

1. Exit the container (if still inside): `exit`
2. Rebuild the image:
   ```bash
   cd ~/@kairu/@projects/@shell/scout
   podman build -t scout:latest -f Containerfile .
   ```
   Note: the apt layer is invalidated by adding a package, so it and every layer after it rebuilds. ~2–3 minutes (Rust install, Claude Code install re-run).
3. Replace the old container:
   ```bash
   podman rm -f scout
   podman run -d --name scout --hostname scout \
     -v ~/@kairu/@projects/@shell/scout:/workspace:Z \
     -w /workspace \
     scout:latest sleep infinity
   ```
4. Re-run the smoke test from step 5 of the runbook — all seven tools should now pass.

Expected output on `sqlite3 --version`: something like `3.45.1 2024-01-30 ...`

---

## 2026-04-21 17:30 — FROM chief-of-staff TO commander — Containerfile triage (step 3 build failure)

**Incident:** `podman build` failed at `RUN useradd` with `UID 1000 is not unique`.

**Root cause (5 whys applied):** Ubuntu 24.04 base image ships a pre-existing `ubuntu` user at UID 1000. My original Containerfile tried to create `scout` at UID 1000 → collision. I skipped verifying the base image's default users before authoring — a breach of the "no assumptions" doctrine. Owning it.

**Fix applied:** Containerfile now removes the pre-existing `ubuntu` user before creating `scout`:
```dockerfile
RUN (userdel -r ubuntu 2>/dev/null || true) \
 && useradd -m -u ${UID} -s /bin/bash ${USER}
```
Idempotent — `|| true` keeps us safe if a future Ubuntu image drops the default.

**Runbook also updated:** phase-0-mobilize.md troubleshooting table includes this failure mode.

**Next action (commander):** re-run step 3:
```bash
cd ~/@kairu/@projects/@shell/scout
podman build -t scout:latest -f Containerfile .
```
Cached layers mean the apt step won't re-run; the build should resume near the (now-fixed) user-creation step and finish.

If step 3 still fails, capture the bottom ~30 lines of output (screenshot or paste) and drop it back here — I'll triage the next one faster.

---

## 2026-04-21 14:00 — FROM chief-of-staff TO all — Command post scaffolded

Host-side scaffold complete. Structure in place:
- `CLAUDE.md` — standing orders
- `ops/CAMPAIGN.md` — full war plan
- `ops/OPORD.md` — active order (Phase 0, pending execution)
- `ops/AGENTS.md` — force structure and ownership
- `ops/playbook.md` — runbook index
- `ops/phase-0-mobilize.md` — first runbook (commander executes)
- `ops/state/*.json` — one file per officer, all `unmobilized`
- `Containerfile`, `Cargo.toml`, `src/main.rs` — placeholders

No agents are online. Phase 0 brings up the container only. Phase 1 convenes the War Council.

Awaiting commander's execution of `ops/phase-0-mobilize.md`.

---

## 2026-04-21 18:47 — FROM commander TO chief-of-staff — Phase 0 checkpoint green

Container built and running. Branches cut. Toolchain verified. Ready for Phase 1 OPORD.

---

## 2026-04-23 23:23 — FROM commander TO council-architect, council-quartermaster, council-security — Wave 2 drafting directives

Position papers read. Council is coherent. Draft your ADRs with these calls resolved:

- ADR-001 (Architect):
  - Adopt nucleo-matcher from v1; do not start on fuzzy-matcher.
  - Fix a p99 performance budget (first-paint, action-spawn). Name numbers.
  - Define behaviour for empty index (banner + hint, not error).
  - Specify index-writer pacing vs active search (no full checkpoint during in-flight queries).
  - Define "candidate" scope for v1 — all indexed paths, not project-filtered.

- ADR-002 (Quartermaster):
  - Bless the gate-C re-reading explicitly — I accept "1.0+ or stability demonstrated over ≥3y". Call it out in Rationale, not in a footnote.
  - Drop `arboard` from v1. No `copy` step in ADR-004.
  - Name the 11-crate shortlist as the authoritative v1 roster; any addition requires its own ADR.

- ADR-003 (Security):
  - Codify the first-run trust prompt as normative. Hash on canonicalised action set only — re-prompt on change, silent otherwise.
  - Name the two quoting seams (print output, sh -c opt-in) as the only shell-escape points; everything else is argv.
  - Declare: env set by a failed step is undefined, not empty. Unknown placeholder = parse error.

- ADR-004 (Architect, 2nd sitting):
  - Pin config discovery order: $XDG_CONFIG_HOME/scout/config.toml → ~/.config/scout/config.toml → /etc/scout/config.toml.
  - Stub a keybinding field on [[action]] — even if only one binding ships in v1, the schema must allow it.
  - Drop `copy` step kind.
  - Step kinds v1: spawn, print, env. Chaining is sequential-only.

Wave 3 reviewers — hold authors to these. Blocker for any ADR that contradicts commander's intent or skips a directive above without arguing why.

---

## 2026-04-24 11:11 — FROM chief-of-staff TO council-quartermaster, council-architect (2nd sitting) — Council does not commit

  Process tightening. Council officers do not `git commit` their drafts.
  Write the ADR file, update your state file, exit. The commit is the
  commander's act.

  Codified in CLAUDE.md §Forbidden and restated in every Wave 2 author
  prompt in `ops/phase-1-council.md`.

  Background: Wave 2 so far — ADR-003 committed itself, ADR-001 did not.
  Neither draft is defective; both will remain as filed. The ambiguity
  is in the standing orders (Council has no sector branch, so §2's
  "commit only to sector/<your-name>" was silent on them). This entry
  closes the gap before ADR-002 and ADR-004 launch.

  Wave 3 reviewers: ADR-003 §Dependencies refers to ADR-001 as
  "accepted" — it is Draft. Flag as a required revision. Non-blocking.

---

## 2026-04-24 12:12 — FROM chief-of-staff TO council-intel, council-quartermaster, council-security, council-surgeon — Wave 3 review directive

Four ADR drafts filed and read end-to-end at Check-in #3. No blocking
conflicts; three tidying items you are specifically tasked to flag
(non-blocking) so the authors clean up in the revision cycle:

1. ADR-003 §Dependencies and ADR-004 §Dependencies both refer to
  ADR-001 as "accepted" — it is Draft. Typo; request one-word fix.

2. ADR-001 §Visit credit reads ambiguously when step 1 succeeds and
  step 2 fails under on_failure=abort. ADR-004 §5 resolves the
  ambiguous case correctly. Request ADR-001 add one sentence
  aligning with ADR-004 §5 so a Phase 3 implementer reading only
  ADR-001 cannot get it wrong.

3. ADR-003 §3 enumerates a canonical-action-set field list that
  omits `keybinding`; ADR-004 §9 includes it (correctly). ADR-003
  already defers the hash schema to ADR-004 — request the in-line
  enumeration be replaced with a pointer to ADR-004 §9.

Beyond these three, review your own portfolio and flag any blocker
per the standard. Blocker only if the ADR contradicts commander's
intent, breaks a downstream ADR, or endangers decade-longevity.
Everything else is non-blocking.

Recommended mode: batched per reviewer — one session covers all four
ADRs in one pass. Saves tokens; reviewer cross-references naturally.

---

## 2026-04-24 16:00 — FROM commander TO all — Phase 1 signed; ADRs Accepted

**Phase 1 CLOSED 2026-04-24.** All four ADRs Accepted at Check-in #4:

- ADR-001 Ranking doctrine — council-architect
- ADR-002 Dependency roster — council-quartermaster
- ADR-003 Threat model — council-security
- ADR-004 Action & config schema — council-architect (2nd sitting)

Sixteen peer-review blocks filed across the five Council officers. Zero blockers. Four `non-blocking`s plus ten endorsements, with three cross-ADR tidying items surfaced consistently by the reviewers.

### Revisions owed before Phase 3 engages

Non-blocking per the review chorus, but each affects a Phase 3 line officer's reading of signed doctrine. All must land before 1st Rifles / 3rd Rifles / Engineers launch. These are doctrinal corrections, not new decisions — may be executed either as fresh author Claude sessions or as direct commander edits. Either way, log in `## Revision history` of the affected ADR; do not re-sign (status remains `Accepted`).

**council-architect — ADR-001 revision, one sentence:**

§Visit credit currently reads self-contradictory under `on_failure = "abort"` ("Action failure with abort credits nothing" vs. "first success wins"). ADR-004 §5 resolves it correctly: credit granted on the first successful step is not retracted by a later abort; credit is suppressed only when the first step fails. Add one sentence to ADR-001 §Visit credit citing ADR-004 §5 so a Phase 3 1st Rifles or Engineers implementer reading only ADR-001 cannot get the contract wrong.

**council-security — ADR-003 revision, two items:**

1. §4 "Windows and macOS" cites `dirs` for the macOS `$XDG_DATA_HOME` fallback. ADR-002 Decision rejected `dirs` in favour of a hand-rolled XDG resolver at `src/config/paths.rs` under the Engineers sector. Point the citation at the resolver, not the crate, so Phase 3 Engineers reads one answer.

2. §3's inline enumeration of canonical-action-set hash fields (`name`, `argv` or step list, `on_failure`, `wait`, `cwd`, `unsafe_shell_template`) omits `keybinding`, which ADR-004 §9 correctly includes. Replace the inline enumeration with a pointer to ADR-004 §9 so reviewers comparing the two documents find no disagreement on what re-prompts.

### Non-blocking items deferred to AAR or Phase 3 preflight

Real concerns surfaced by reviewers that do not gate Phase 2 or Phase 3 launch, but should not be lost:

- **Surgeon on ADR-001:** cold-start → first-paint p99 ≤ 100 ms budget should name an exclusion for startup `PRAGMA integrity_check` on a missing clean-shutdown sentinel. Revise in ADR-001 at convenience; carry to Phase 4 performance review.
- **Surgeon on ADR-003:** non-TTY refusal stderr message should explicitly name the interactive-run requirement so operators do not burn a triage cycle.
- **Intel on ADR-001:** `K_match = 100` calibration is load-bearing; recommend a Phase 2 `tracing` span on raw `m_c` per query to calibrate before Phase 3 locks the UX.
- **Architect on ADR-002:** Phase 3 `ipc` module with N search workers on one `query_rx` is MPMC and needs `crossbeam-channel` — ADR-002 Rejected table already pre-approves this as a swap candidate, so no new ADR is required, but 1st Rifles should not encounter it as a surprise.
- **Runbook typo:** `ops/phase-1-council.md` §Verification mentions `status: "standdown"` but CLAUDE.md §3 defines `standby` as the correct stand-down status. Fix in Phase 1 AAR.

### Posture

Council stands down. Line officers remain unmobilized.

Chief of Staff will author Phase 2 OPORD and `ops/phase-2-db.md` (2nd Rifles' runbook for the frecency-capable index) once:

1. The ADR-001 and ADR-003 revisions above have landed.
2. Chief of Staff has added `/worktrees/` to `.gitignore` at Phase 2 preflight per AGENTS.md § Worktree Discipline.

Standing interrupts remain: `stand down`, `redirect`, `promote`, `AAR now`.

---

## 2026-05-01 09:00 — FROM chief-of-staff TO commander, rifles-2 — Phase 2 orders cut

**Phase 1 CLOSED.** Phase 2 preconditions met: ADR-001 + ADR-003 revisions landed (commit `b1e0e0e`); `/worktrees/` added to `.gitignore`.

**Phase 2 ISSUED.** See updated `ops/OPORD.md` and the new runbook at `ops/phase-2-db.md`. `ops/playbook.md` updated to link the Phase 2 runbook.

Phase 2 is the DB Foundation engagement. **Single line officer (`rifles-2`), four sequential engagements, two commander check-ins.** Scope: schema + migration, WAL + PRAGMA, streaming walker + batched insert, visit path + crash recovery. Binding ADRs are ADR-001 (schema, pacing, visit-credit contract), ADR-002 (admitted deps), ADR-003 (DB permissions, parameter-bound SQL, O_NOFOLLOW).

### Three things commander should scrutinise before sign-off

1. **ADR-002 Phase 2 admission list expansion for `ignore`.** OPORD §Doctrinal note argues that CAMPAIGN.md's "streaming indexer" milestone requires the `ignore` crate (slot 3), which ADR-002 §Consequences §Binds-2nd-Rifles omits from the Phase 2 admission list. The OPORD extends the list to `{rusqlite, tracing, tracing-subscriber, signal-hook, ignore}`. **Sign-off on this OPORD constitutes approval of the expansion.** A one-line revision to ADR-002 Revision history will capture it post-signature. If you disagree, redirect before launching engagement 1.

2. **Surgeon's integrity_check exemption** from ADR-001's 100 ms cold-start budget. OPORD §3 acknowledges the recovery path runs its own budget (≤ 2 s target / 10 s hard fail) and emits a `tracing` span so the budget miss is visible. No re-sign of ADR-001 required; Wave 3 already flagged this as non-blocking.

3. **Worktree discipline activates for the first time this phase.** Chief of Staff runs `git worktree add worktrees/sector-index sector/index` at preflight. `rifles-2` operates only inside `/workspace/worktrees/sector-index/` under `podman exec`. Line officer never runs `git worktree add|remove|move` — those belong to Chief of Staff alone (AGENTS.md §Worktree Discipline). The same discipline will be rehearsed on a single officer here, scaled to three in Phase 3.

### Where your two cents matters most

- **Check-in #1 (after engagement 2)** — inspect the schema on disk against ADR-001 §Consequences. Cheap to redirect here; expensive to migrate out of once engagement 3 inserts rows.
- **Check-in #2 (after engagement 4)** — commander gut-checks the 100 k-path walk on real data and the crash-recovery path against a `dd`-corrupted fixture. This is the "approve merge to main" gate.

### Preflight commander must execute

The runbook is explicit. Four steps before engagement 1:

1. `podman ps` sanity and `.gitignore` grep confirm.
2. `git worktree add worktrees/sector-index sector/index` (Chief of Staff act — can be commander too).
3. Update `ops/state/rifles-2.json` to `"status": "activated"` (already drafted; adjust timestamp).
4. Claude Code auth sanity inside the worktree via `podman exec`.

Then commit the Phase 2 preflight and launch engagement 1.

### Standing interrupts remain

`stand down`, `redirect`, `promote`, `AAR now`.

Awaiting commander's execution of `ops/phase-2-db.md`.
## 2026-07-05 18:05 — FROM rifles-2 TO chief-of-staff — Phase 2 green

Four engagements closed on sector/index. Success criteria:

- Worktree worktrees/sector-index existed on sector/index (removed post-merge).
- migrations/0001_initial.sql matches ADR-001 §Consequences; run_state shipped in the same migration per runbook engagement 1 (the §Verification line naming a separate 0002_run_state.sql is runbook drift, not schema drift).
- Fresh-DB schema shows every ADR-001 column plus UNIQUE canonical-path index and run_state metadata (asserted by tests/index_schema.rs; no sqlite3 CLI in this container).
- cargo test: 19 index tests green.
- 100k-path synthetic walk (release): <2 s, RSS under 100 MB.
- record_visit median well under 5 ms over 1000 calls on a 100k-row fixture.
- Missing sentinel triggers integrity_check (10 s interrupt watchdog); corrupt fixture renamed index.db.corrupt-<epoch> and rebuilt; healthy DB survives the same path.
- Interrupt mid-walk leaves prior generation current — including the interrupted-empty-stream edge found and fixed during engagement 3.
- ADR-002 revision entry for the ignore admission filed by CoS.

One platform finding for the record: O_NOFOLLOW is 0o100000 on asm-generic Linux arches (aarch64) vs 0o400000 on x86 — pinned per-arch in pragma.rs.

Ready for Phase 3 OPORD.

## 2026-07-05 18:06 — FROM chief-of-staff TO all — Phase 2 merged; Phase 3 ISSUED

Commander authorisation: standing goal directive 2026-07-05 ("get scout working") covers phase check-ins and merge authorisations; evidence above stands in for the interactive gut-check. sector/index merged to main --no-ff.

Phase 3 OPORD issued in ops/OPORD.md: 1st Rifles (search), 3rd Rifles (TUI), Engineers (actions/config) engage. Milestone: type -> ranked results -> pick -> action -> side effect.

## 2026-07-05 18:20 — FROM chief-of-staff TO commander — Phase 3 green

Three sectors shipped and merged --no-ff (search, actions, tui). Success criteria:

- cargo test: 45 tests green on merged main; 100k smoke gate 2.2 s release.
- End-to-end, PTY-driven with the real binary: index a fixture tree; trust prompt fires on first config load and accepts on `y`; typed query ranks; Enter dispatches the user print action; stdout emits POSIX-quoted `cd '<path>'` (eval contract holds because the TUI draws on stderr); visit credit lands (S=1.0, visits_total=1); second launch loads silently (hash match); zero-query then ranks the visited path first (frecency observable); corrupt DB renamed .corrupt-<epoch> and rebuilt at the CLI level.
- Config on disk drives behaviour: user `go` action took Enter from the compiled `edit` default; compiled defaults survive by name.
- Empty index renders banner+hint, exit 0 (drilled pre-config).

Deviations for the record, none blocking:
1. Search ships synchronous in-memory ranking, not the multi-worker streaming pipeline of Architect §2 — budgets hold at 100k with wide margin; the QUERY_ACTIVE/GEN seam is preserved. Revisit if index scale demands partials.
2. Trust-change view shows the full action listing, not an added/removed diff — the store keeps only hashes. A stored projection alongside the hash is the Phase 4+ fix if wanted.
3. Detached spawns (wait=false) use process_group(0)+null stdio as the setsid approximation; true setsid needs libc, which is off the roster. ADR-003's hazard (terminal steal/wedge) is covered.
4. index::unix_now visibility widened to pub by 3rd Rifles for CLI wiring — one-line cross-sector touch, flagged here per ROE §1.
5. `scout query` subcommand added beyond the OPORD list — composability surface for scripts and the non-TTY path named in the trust-refusal message.

Phase 4 (packaging, install, CI tripwires per ADR-002) remains commander-gated. Standing down.

## 2026-07-05 19:05 — FROM chief-of-staff TO all — Field report: eval-seam hardening (Phase 4 input)

Live finding from the commander's first day driving the tool. The naive shell wrapper `eval "$(scout)"` executes WHATEVER lands on stdout; the compiled `print-path` default emits a bare quoted VALUE, so dispatching it under the wrapper made bash try to execute the selected file ("Permission denied" on a .json; an executable would have RUN). Surprising execution — exactly the class ADR-003 exists to refuse — introduced at the seam between two individually-correct pieces.

Doctrine for the shell-integration snippet Pioneers ship in Phase 4:

1. Under an eval wrapper, every dispatched action must PRINT A COMMAND, never a value. Reference config now field-tested: go = "cd {path} 2>/dev/null || cd {parent}"; edit = "${{EDITOR:-micro}} {path}" (brace-escaped; shell-side default closes the unset-EDITOR abort without weakening the strict undefined-env grammar — the fallback lives in the one seam a shell already owns); print-path override = "printf '%s\n' {path}".
2. The wrapper must allowlist eval-able line shapes (cd/printf/${EDITOR prefixes, per line, refuse-and-report otherwise) — defense in depth so a value-emitting action, a future default, or corrupted output degrades to a printed line, never an execution. Guard verified against: bare quoted path, executable path, rm smuggle, hostile second line after a valid cd. 8/8.
3. Spawn-kind editor actions are incompatible with command-substitution wrappers (child inherits the capture pipe as stdout; terminal editors break). The wrapper pattern is: scout prints, shell executes after scout exits. Compiled `edit` (spawn) remains correct for wrapper-less use; docs must say which mode wants which.

Suggested Phase 4 work items: ship the guarded function + reference config as the official snippet; consider a compiled default set that is wrapper-safe by construction; README section on the two consumption modes.

## 2026-07-05 20:10 — FROM chief-of-staff TO all — Visual pass shipped (post-Phase-3 enhancement)

Commander flagged the TUI as bland against pm2/lazygit-class tools. Two-sector engagement, merged --no-ff:

- 1st Rifles: QueryScorer gains score_with_indices; Ranked carries matched char positions (empty on zero-query). Swap seam preserved.
- 3rd Rifles: visual grammar in src/ui/render.rs (pure, 5 unit tests) + draw rewrite. Single amber accent (deliberately not the default TUI cyan), dim-dir/bold-basename typography, ~ home collapse, match-char highlighting, left-truncation preserving basenames, frecency signal meter (3-cell ramp calibrated to K_frec = 10) with dim visit count, prompt/cursor/counter query row, severity-styled banners (ADR strings verbatim), rounded action-menu popup. ANSI named colors only — user terminal themes restyle. Strip filter intact (moved inline into path-cell classification, still covers chrome).

Verified by PTY capture + frame reconstruction; all suites green; binary reinstalled via scout-bundle restore.

## 2026-07-05 21:20 — FROM pioneers TO chief-of-staff — Phase 4 green

Deliverables merged --no-ff (also in this window: TUI preview pane, HANDOFF context above). Success criteria:

- Fresh-machine checkpoint drilled end-to-end in a clean HOME: install.sh built and installed 0.1.0, the printed day-one steps were followed verbatim (snippet sourced, dotfiles config copied), trust prompt listed 3 actions and took y, typed query + Enter, and the invoking shell's PWD changed to the picked project. Commander's intent checkpoint ("clone dotfiles -> install -> works day one") met.
- MSRV pin live: rust-toolchain.toml resolves 1.96.1; workspace builds and all 10 suites pass under it; clippy -D warnings clean after an 8-lint sweep.
- cargo audit: zero vulnerabilities; two allowed transitive warnings on record (paste unmaintained, lru unsound IterMut — both below ratatui/nucleo; watch for upstream bumps).
- deny.toml + ci.yml + release.yml in tree; syntax-checked locally. CI executes on push (this box has no GitHub runner).
- shell/scout.bash is the canonical wrapper; the agent-container bundle now re-sources it (bundled copy demoted to explicit fallback).

Findings for the record:

1. ADR-002 transitive ceiling: the ADR's literal metric (cargo tree --target all) reads 124 — four over — entirely from Windows-only crates under crossterm, for a platform ADR-002 itself defers. Shipped-target graph is 104. CI enforces the shipped-target reading; requesting a one-line ADR-002 revision ratifying the metric (Quartermaster/commander act).
2. musl cross-compile smoke could not run locally (no root, no musl toolchain in the agent container); it is wired as a hard CI job on both arches with a static-link assertion, which is the "Pioneers build runner" ADR-002 names. First CI run is the real smoke.
3. Containerfile repurposed from the Phase 0 agent-battlefield scaffold to a two-stage product image; the battlefield rig lives in the ops repo now.

Release is armed, not fired: pushing a v0.1.0 tag is the commander's act; the workflow attaches musl tarballs + sha256.

Phase 5 (AAR) awaits commander word.

## 2026-07-06 — FROM chief-of-staff TO commander — shadow-review escalations resolved

All six escalated findings actioned under commander authorisation ("go with your suggestions"). Committed cbca014 (fixes) atop 7955b5f (confirmed-fix batch).

- Print-seam injection: real vuln (unquoted {name}/{ext}/{query}/{env.*} on a wrapper-eval'd line). ADR-003 §2 + ADR-004 §3/§4 revised to quote EVERY placeholder at the print seam; enforced in template.rs; drilled end-to-end (dir named `proj $(touch PWNED)` no longer executes). This was the highest-value escalation.
- print-path default: ADR-004 §7 revised; default emits a command, works under the wrapper.
- deny.toml/ADR-002: ratified in ADR-002 Revision history. Running cargo-deny for real caught two gaps I'd missed by hand — nucleo-matcher (signed slot 6) is MPL-2.0, absent from the ADR allow-list; windows-sys duplicates. Both now recorded/allowed. `cargo deny check` fully clean.
- Actions SHA-pinned; musl build extracted to a reusable workflow (CI+release share one source); release gains a tag==version guard + single fan-in publish.
- unicode display-width: NOT fixed — needs unicode-width, an ADR-002 successor-ADR decision. Documented as a v2 known limitation.

Meta-lesson for the AAR: the CI supply-chain tripwire earned its keep before it ever ran in CI — cargo-deny surfaced the MPL-2.0 roster/allow-list inconsistency that four council reviewers and the commander missed when ADR-002 was signed. The gate found a hole in the doctrine that authored it.

58 tests green; clippy clean; cargo deny clean. Ready for a v0.1.0 tag when the commander chooses.
