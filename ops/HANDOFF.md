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
