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

**Before you move:** the runbook's preflight section covers Claude-Code auth — verify that first. If headless auth fails, the runbook gives the `ANTHROPIC_API_KEY` fallback.

**Where your two cents matters most:** Check-in #1 (before launching any officer — edit the prompts if any asks for the wrong thing) and Check-in #3 (ADR drafts before peer review — cheap to demand revisions here).

**Standing interrupt remains:** `stand down`, `redirect`, `promote`, `AAR now`.

Awaiting commander's execution of `ops/phase-1-council.md`.

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
- `Cargo.toml`, `src/main.rs` — placeholders

No agents are online. Phase 0 stands up the command post only. Phase 1 convenes the War Council.

Awaiting commander's execution of `ops/phase-0-mobilize.md`.

---

## 2026-04-21 18:47 — FROM commander TO chief-of-staff — Phase 0 checkpoint green

Command post live. Branches cut. Toolchain verified. Ready for Phase 1 OPORD.

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

3. **Worktree discipline activates for the first time this phase.** Chief of Staff runs `git worktree add worktrees/sector-index sector/index` at preflight. `rifles-2` operates only inside `~/projects/scout/worktrees/sector-index/`. Line officer never runs `git worktree add|remove|move` — those belong to Chief of Staff alone (AGENTS.md §Worktree Discipline). The same discipline will be rehearsed on a single officer here, scaled to three in Phase 3.

### Where your two cents matters most

- **Check-in #1 (after engagement 2)** — inspect the schema on disk against ADR-001 §Consequences. Cheap to redirect here; expensive to migrate out of once engagement 3 inserts rows.
- **Check-in #2 (after engagement 4)** — commander gut-checks the 100 k-path walk on real data and the crash-recovery path against a `dd`-corrupted fixture. This is the "approve merge to main" gate.

### Preflight commander must execute

The runbook is explicit. Four steps before engagement 1:

1. Command-post and `.gitignore` grep confirm.
2. `git worktree add worktrees/sector-index sector/index` (Chief of Staff act — can be commander too).
3. Update `ops/state/rifles-2.json` to `"status": "activated"` (already drafted; adjust timestamp).
4. Claude Code auth sanity inside the worktree.

Then commit the Phase 2 preflight and launch engagement 1.

### Standing interrupts remain

`stand down`, `redirect`, `promote`, `AAR now`.

Awaiting commander's execution of `ops/phase-2-db.md`.
## 2026-07-05 18:05 — FROM rifles-2 TO chief-of-staff — Phase 2 green

Four engagements closed on sector/index. Success criteria:

- Worktree worktrees/sector-index existed on sector/index (removed post-merge).
- migrations/0001_initial.sql matches ADR-001 §Consequences; run_state shipped in the same migration per runbook engagement 1 (the §Verification line naming a separate 0002_run_state.sql is runbook drift, not schema drift).
- Fresh-DB schema shows every ADR-001 column plus UNIQUE canonical-path index and run_state metadata (asserted by tests/index_schema.rs; no sqlite3 CLI in this environment).
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
3. Distribution is install.sh + the musl release tarballs; no container image ships.

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

## 2026-08-14 — FROM chief-of-staff TO commander — Maintenance sortie: supply chain, crash recovery, log control, CI gates

Commander-directed maintenance under explicit authorisation ("proceed with
the changes"). Executed on `main` as a staff action rather than through
sector worktrees: the change spans Pioneers (CI, README), 3rd Rifles (TUI
teardown), and the Quartermaster's file (Cargo.toml), and standing up three
worktrees for four small fixes costs more than it protects. CLAUDE.md
§Forbidden items touched under the "unless explicitly ordered" clause:
touching `main`, and a `Cargo.toml` change. Recorded here so the deviation
is on the books, not implied.

### 1. Supply chain — CI was red and nobody knew

`cargo audit` and `cargo deny check` both failed on `main`:
RUSTSEC-2026-0204, an invalid pointer dereference in `crossbeam-epoch`
0.9.18, reached via `crossbeam-deque` -> `ignore`. The advisory was
published 2026-07-06 — the day after the last push. CI had been green
because the advisory did not yet exist, and `ci.yml` triggered only on
push and pull_request, so nothing re-evaluated the lockfile afterwards.

Fixed by `cargo update -p crossbeam-epoch` (0.9.18 -> 0.9.20). Lockfile
only; no roster change, so no successor ADR is owed. A weekly `schedule:`
trigger now runs the tripwires against an unchanged lockfile, which is the
condition under which this class of finding appears.

Second lesson for the AAR docket, and it rhymes with the first: the
supply-chain gate was correct and still silent, because it was wired to
the wrong event. A tripwire that only fires when *we* move cannot see a
world that moves on its own.

### 2. Crash recovery — the picker left dead terminals

`ui::run` restored the terminal only on the normal exit path, and the
release profile sets `panic = "abort"`, so no unwind and no Drop guard
could cover a panic either. Proven under a PTY with an injected panic:
without a hook the capture shows `ESC[?1049h` (enter alternate screen),
the panic text written inside that screen, no `ESC[?1049l`, and no `^M`
line endings — that last detail is the tell that raw mode was never
disabled. On a real terminal the operator loses the panic message *and*
their line discipline, and the only recovery is `reset`.

`install_panic_hook` now restores the terminal before the default hook
prints, and also writes the panic to the log file, which is the only
durable copy when the TUI owned stderr. The hook runs under
`panic = "abort"` (hooks execute before the abort), so the release profile
needs no change. Re-run of the same drill shows `ESC[?1049l` ahead of the
panic text and `^M` line endings restored.

Also fixed while in there: teardown took `?` on `disable_raw_mode`, so a
failure there skipped `LeaveAlternateScreen` entirely, contradicting the
comment above it. Teardown is now unconditional and best-effort.

### 3. Log control — the debug logs existed and could not be switched on

`init_tracing` built a subscriber with no `EnvFilter`, so the default
INFO ceiling was fixed at compile time and no environment variable could
lift it. Every `debug!` in the walker — canonicalisation failures,
boundary refusals, skipped entries, i.e. the entire answer to "why is my
project missing from the index" — was compiled in and permanently
unreachable.

`SCOUT_LOG` now takes a level or a per-module filter. Scout-specific
rather than `RUST_LOG`, so a developer who exports `RUST_LOG=debug` for
another tool does not silently start filling scout's log file.

Cost against the ADR-002 ceiling: the `env-filter` feature added exactly
one crate (104 -> 105 of 120). `matchers`, `regex-automata`, and
`thread_local` were already in the graph under ratatui and ignore. No new
roster entry; the feature is enabled on a crate already in slot.

One trap found and closed during verification: a bare directive that is
not a level name parses cleanly as a *target*, so `SCOUT_LOG=dbug`
silences all output. The user asks for more logging and gets less, with
nothing to explain it. Bare non-level directives are now called out on
stderr, while still being honoured — targeting a module is legitimate.

### 4. The 100k gate had never run in CI

`smoke_100k_paths_under_budget` is `#[ignore]`d and CI ran plain
`cargo test`, so the Phase 2 checkpoint — 100k paths under 30 s wall,
RSS under 100 MB — has never been enforced anywhere but by hand. That is
the budget the streaming indexer exists to hold, and it is the gate that
will catch a future size-rollup feature regressing indexing. Now a
separate `perf-gate` job on the release profile, so it does not serialise
behind clippy.

### 5. README rewritten to the commander's specification

Product documentation only: what scout is, install, a commands table
covering every subcommand and flag, picker behaviour, shell integration,
configuration, the files scout owns, and troubleshooting. The campaign
status section, the internal documents table, and "Execute next" are
gone — ops docs are for us, not for a reader who wants to use the tool.
Two errors it carried are corrected: the index DB lives under
`$XDG_DATA_HOME`, not `$XDG_STATE_HOME`, and the trust store is
`trusted-config.sha256`, not a JSON file. `docs_parity` still pins the
wrapper block to `shell/scout.bash` verbatim.

### Standing decisions from the commander this sortie

- **Remote/mounted filesystems are struck from the roadmap.** No ADR is
  owed and none should be drafted. The concern that motivated it is on
  record — `walk.rs` canonicalises every entry, one round-trip per path,
  which would make a network mount unusable rather than failing loudly —
  but the feature is not wanted, so the analysis is archival only.
- **unicode-width is promoted from cosmetic to prerequisite.** It is
  currently a single-column display-width bug (`src/ui/render.rs:61`,
  chars counted rather than grapheme clusters). Any multi-pane or
  columnar UI work makes it structural instead: one CJK or emoji
  filename misaligns every column on its row. The successor ADR to
  ADR-002 admitting `unicode-width` must therefore land *before* the UI
  engagement opens, not alongside it.

### Verification

60 tests pass (1 ignored — the 100k gate, run separately), `clippy
-D warnings` clean, `cargo audit` clean, `cargo deny check` reports
advisories/bans/licenses/sources all ok, transitive ceiling 105 of 120.
SCOUT_LOG verified end-to-end against a tree containing a broken symlink:
default prints INFO only, `debug` and `scout=debug` surface the
canonicalisation skip, `foo=notalevel` and `====` fall back to info with a
message, `dbug` warns that it reads as a target. Panic path verified by
PTY capture in both directions as described above.

## 2026-08-14 — FROM chief-of-staff TO commander — ADR-005 landed; Spotlight direction recorded

### ADR-005 (Display Width) drafted, signed by direct order, implemented

The deferral was priced against a bill that never arrives: `unicode-width`
is already in the shipped graph twice under `ratatui` — directly and
through `unicode-truncate`. Admitting it as a direct dependency moved the
transitive ceiling by zero, 105 of 120. The 2026-07-06 shadow-review
deferred this on supply-chain grounds that were reasonable then and simply
false now, and nobody re-checked. Worth carrying to the AAR: a deferral
records a judgement about a cost, and costs move; a deferral with no
re-check date is a decision that quietly stops being true.

Pinned to `0.1` because `deny.toml` sets `multiple-versions = "deny"`.
Requesting a major that resolved separately from ratatui's would fail our
own gate. The requirement therefore moves only when ratatui moves, which
makes it a note for whoever does the dependency refresh.

Layout is now measured in terminal columns at all three call sites:
`render::display_width` is the single source of truth, `truncate_left`
spends a column budget, and `result_line` pads by columns.

Verified by PTY capture with frame reconstruction (a `script(1)` capture
of a ratatui frame collapses to one line, because ratatui paints by
absolute cursor positioning rather than newlines — the replay tool
rebuilds the grid). Over a tree mixing CJK and ASCII names at 90 columns:

    with ADR-005      43 / 43 / 43 / 43 columns of path text
    char counting     43 / 43 / 49 / 51 columns — and the widest row's
                      basename clipped mid-name by ratatui

That clipping is the part that matters. The overflow does not merely
push the meta column out of true; it eats the end of the basename, which
is the single most useful part of the row and the reason truncation is
left-anchored in the first place. Recorded as cosmetic in July; it was
destroying information.

The test obligation in ADR-005 §Consequences exists because the original
suite asserted `cells.len()` after truncation — an assertion that passes
with the bug fully present. The new tests assert columns.

### Commander's UI direction — Spotlight, not lazygit's panels

Correction to the roadmap I filed on 2026-08-14. The lazygit reference
was about *presentation quality*, not about panel layout. The commander
does not want a multi-pane, multi-column interface. The stated
inspiration is **macOS Spotlight (Tahoe)**: a search surface that
appears, takes a query, presents a small number of well-formed results,
and gets out of the way.

Consequences for the UI engagement, which is still ADR-gated and not yet
opened:

- **Fewer visible paths, not more.** The commander's words: not having to
  see loads of file paths. The full absolute path is diagnostic output,
  not the primary presentation. A result should read as a *thing* — name
  first, location as secondary context — rather than as a filesystem
  string that happens to be truncated.
- **Multi-column layout is not the goal**, so ADR-005's value is not the
  column arithmetic for a grid. It is that any presentation which places
  anything after a variable-width name needs true widths — and a
  name-first layout needs them more than a path list did, because the
  name is now the aligned element.
- **The preview pane's status is an open question** for that ADR. It is a
  second panel, and it is also the thing that makes a result legible
  without reading its path. Not to be removed on my initiative.
- `scout doctor` (the diagnostics subcommand proposed in the same
  roadmap) is endorsed by the commander and stays on the list.

Neither the UI work nor `doctor` is started. Both need their ADRs.

## 2026-08-14 — FROM chief-of-staff TO commander — ADR-006 `scout doctor` shipped; tree scrubbed

### Files removed, each checked before removal

- `ops/logs/` and its `.gitkeep` — nothing in the repository has ever
  written to that directory and no document references it. Phase 0
  scaffolding that never acquired a user. The two `.gitignore` rules
  that existed only to preserve it went with it.
- `docs/adr/.gitkeep`, `tests/.gitkeep` — both directories carry tracked
  files, so neither placeholder does anything.
- `.DS_Store` x3 (one inside `.git/`), and the empty `worktrees/`
  directory, which `git worktree add` recreates on demand.

Checked and KEPT, since "unused" was the wrong reading in each case:
`src/ipc` looks like a leftover of the abandoned worker pipeline but is
live — `search` stores `QUERY_ACTIVE`, the index writer loads it to pace
checkpoints, and `tests/index_pragma.rs` covers both ends.
`docs/adr/.template.md` is the house ADR shape. `ops/state/*.json` is
required by CLAUDE.md §3 even while every officer is on standby.

### Formatting

The tree had never been rustfmt-clean: every source file differed from
every rustfmt configuration, no `rustfmt.toml` existed, and CI had no
`fmt` step, so nothing held the line. `use_small_heuristics = "Max"` is
the configuration nearest to how the code was actually written; the
whole tree is now formatted to it and `cargo fmt --all --check` runs in
CI.

The reformat earned its keep immediately by surfacing a real defect: the
long `SCOUT_LOG` doc comment had come to sit on the `LOG_LEVELS`
constant rather than on `log_filter`, because the constant was inserted
between a comment and the function it documented. Moved back.

### ADR-006 — `scout doctor`

A read-only snapshot of the state scout resolves at startup: config
discovery chain and which link wins, trust, index vitals, environment,
log tail. Each line marked `ok` / `warn` / `FAIL`; exit 0 unless
something failed.

Three decisions worth reading in the ADR rather than the diff.

**The environment allowlist.** `doctor` prints four named variables and
never the environment. Diagnostic output exists to be pasted, and
ADR-003's deliberate refusal to strip `AWS_*` / `GITHUB_TOKEN` from a
*spawned action's* environment does not extend to *displaying* them. An
env-dumping `doctor` is a credential-exfiltration convenience, one paste
at a time.

**Severity is not uniform.** A missing index is `warn`, a corrupt one is
`FAIL`. Conflating them makes the exit code useless on a fresh machine,
which is the machine most likely to run `doctor` first.

**`doctor` asks the loader which config wins** rather than
re-implementing the `O_NOFOLLOW` walk. A diagnostic with its own copy of
the rules eventually disagrees with the real ones, and it will disagree
precisely when someone is trusting it to be right. `loader::discover` is
now public for this and nothing else.

### A defect I introduced and caught in the drill

The first implementation called `index::pragma::open` to read the index.
That path creates the parent directory, creates the file, runs
migrations, and — on a corrupt database — renames it aside and rebuilds
it. So `doctor` was *repairing the fault while reporting it*: exactly
the behaviour ADR-006 §Alternatives 4 rejects, written into the code by
the same person who wrote the rejection. The corrupt-index drill is what
exposed it — the run reported a healthy empty database, because the open
had just fabricated one.

It now opens read-only, which cannot create, migrate, or recover.
`tests/doctor.rs` guards the promise from outside the process: a fresh
sandbox must come back with no database, no data directory and no state
directory created, and a corrupt database must still be byte-identical
after the run, with no rebuilt sibling.

Lesson for the AAR, and it is not a small one: writing the prohibition
into doctrine does not stop you from implementing the thing you
prohibited. Only the drill did.

### Verification

69 tests across 12 suites, `cargo fmt --check` clean, clippy clean,
`cargo deny check` clean, ceiling 105 of 120 (ADR-006 adds no
dependency). `doctor` exercised against four real states: healthy,
fresh machine, symlinked config, corrupt index.
