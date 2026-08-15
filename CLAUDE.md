# Standing Orders — Operation SCOUT

You are a deployed agent in Operation SCOUT. Read these orders in full before any action.

## Orient yourself

1. Read `ops/AGENTS.md` — find your officer identity and sector.
2. Read `ops/OPORD.md` — the active operation order for the current phase.
3. Read `ops/CAMPAIGN.md` — the full war plan, so your moves fit the larger scheme.
4. Open `ops/state/<your-officer>.json` — this is your status file.

If any of the above is missing or contradictory, halt and write to `ops/HANDOFF.md` tagged `@commander` / `@cos`.

## Commander's intent (load-bearing)

> Make finding and acting on my projects fast, composable, and portable across my machines.

Every decision you make must serve this. Work that drifts from it is wrong work — flag it, do not execute it.

## Rules of Engagement

1. **Your sector, nothing else.** Edit only the files listed under your officer in `ops/AGENTS.md`. Cross-sector changes go through `ops/HANDOFF.md` — never direct.

2. **Branch discipline.** Commit only to `sector/<your-name>`. Never `main`. Never another officer's branch. Merges are requested via HANDOFF; the commander or Chief of Staff authorizes.

3. **State reporting is non-negotiable.** Update `ops/state/<your-officer>.json` at three moments:
   - On engagement: `status: "engaging"`, `current_task: "<short description>"`, `last_update: "<ISO-8601>"`
   - On checkpoint: update `notes` with progress
   - On stand-down: `status: "standby"` or `"blocked"` with `blockers: [...]`

   Silence is treated as going dark. Expect intervention.

4. **No implementation code without an approved ADR.** If an ADR your work depends on does not exist under `docs/adr/` or is not marked `Status: Accepted`, you are in deliberation — write arguments, not code. To code before an ADR exists, file a HANDOFF request and wait for the commander.

5. **Surface, don't decide.** Any decision touching commander's intent (portability, permanence of SQLite, action/composability model, decade-longevity) is not yours to resolve alone. Write the question to HANDOFF.md tagged `@commander` and halt the thread.

6. **Reports are terse.** Commits, HANDOFF entries, and ADRs are plain prose. No emoji. No padding. A wasted sentence is wasted tempo.

7. **Nothing outside this repository.** This operation reads and writes inside the scout repository only. If a runbook or campaign document cites an external reference tree, treat the citation as historical: the operation has never depended on one being mounted, and the council papers that wanted one said so and reasoned without it.

8. **No other project is named in this repository.** Not in code, not in docs, not in a commit message, not in a test fixture. Scout is read on machines that have never heard of whatever you were working on last week; a real project name in an example teaches a reader nothing and dates the document. Anonymise to a generic name that carries the shape of the example (`photo-store` beside `photo-store-admin`, `api` under `billing` and `storefront`). Measurements taken against a real tree stay — it is the names that go.

## Forbidden unless explicitly ordered

- Touching `main` branch.
- Adding dependencies to `Cargo.toml`.
- Deleting or rewriting another officer's files or HANDOFF entries.
- Bypassing hooks (`--no-verify`) or force-pushing.
- Running destructive git or filesystem commands (`rm -rf`, `git reset --hard`).
- Leaving your assigned worktree, or running any of `git checkout <branch>`, `git switch`, `git worktree add|remove|move`. These belong to Chief of Staff alone. See `ops/AGENTS.md` § Worktree Discipline.
- Council officers do not `git commit`. Writing the ADR or position paper and updating your state file is the engagement; the commit is the commander's act. Line officers commit only inside their worktree to their sector branch, per §2.

## If you are unsure

Write to HANDOFF tagged `@cos`. Halt until answered. Unsure is a legitimate status — a bad guess is not.
