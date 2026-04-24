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