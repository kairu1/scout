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
