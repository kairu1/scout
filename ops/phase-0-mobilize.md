# Phase 0 Runbook — Mobilization

> **Milestone:** Command post live. Main branch initialised. Sector branches cut. Smoke HANDOFF entry round-trips.
>
> **Checkpoint:** Every item in § Verification below is green.
>
> **Connection:** Phase 1 unlocks — War Council convenes with the five deliberators.

---

## 0. Prerequisites

The dev machine has the toolchain scout needs. Confirm before touching anything else.

```bash
rustc --version    # Expected: rustc 1.x
cargo --version    # Expected: cargo 1.x
claude --version   # Expected: Claude Code CLI version string
tmux -V            # Expected: tmux 3.x
git --version      # Expected: git 2.28 or newer (for `git init -b`)
```

Any missing tool → install it before proceeding.

**Milestone:** Toolchain present.
**Checkpoint:** Every version command prints without error.
**Connection:** The workbench exists; we can build and run.

---

## 1. Navigate to the command post

```bash
cd ~/projects/scout
pwd
# Expected: ~/projects/scout
ls
# Expected: Cargo.toml  CLAUDE.md  README.md  docs  ops  src  tests
```

If the listing is wrong, **stand down and escalate** — the scaffold was incomplete.

---

## 2. Initialise the git repository

The scaffold files exist on disk but are not yet under version control. Bring them in, then cut sector branches.

```bash
cd ~/projects/scout

# Init on main (not master)
git init -b main

git add .
git status
# Eyeball the staged files — confirm nothing unexpected (e.g., no .db files)

git commit -m "Phase 0: scaffold SCOUT command post

Operation SCOUT scaffolded per commander's approved campaign plan.
No implementation code; no dependencies; agents not yet launched.
Reference terrain (pathexplorer) left untouched."
```

**Milestone:** Scaffold under version control.
**Checkpoint:** `git log --oneline` shows one commit; `git branch` shows `* main`.

### Cut sector branches

Each line officer gets their own branch off `main`. They will not exist remotely until a remote is added later.

```bash
for s in search index tui actions ops; do
  git branch "sector/$s"
done

git branch
# Expected:
#   * main
#     sector/actions
#     sector/index
#     sector/ops
#     sector/search
#     sector/tui
```

**Milestone:** Five sector branches exist.
**Checkpoint:** The listing above matches exactly.
**Connection:** Line officers have a place to commit when activated.

---

## 3. Smoke-test the toolchain

With zero dependencies declared, the placeholder main should build cleanly.

```bash
cd ~/projects/scout
cargo check
# Expected: Compiling scout v0.0.0 ... Finished
```

**Milestone:** Toolchain verified against the scaffold.
**Checkpoint:** `cargo check` succeeded.
**Connection:** The workbench is fit for deployed agents.

---

## 4. tmux smoke test

tmux gives each officer its own pane during multi-agent phases.

```bash
tmux new-session -d -s scout -n cos
tmux list-sessions
# Expected: scout: 1 windows ...
```

Attach briefly to confirm it renders:

```bash
tmux attach -t scout
# Prefix is Ctrl-b by default. Detach with: Ctrl-b then d
```

Leave the session running — Phase 1 will reuse it.

**Milestone:** tmux session ready for officer deployment.
**Checkpoint:** `tmux list-sessions` shows the `scout` session.
**Connection:** Phase 1 can spawn War Council panes here.

---

## 5. Smoke HANDOFF round-trip

Append an acknowledgement to `ops/HANDOFF.md` — this exercises the primary command channel. Use your editor of choice; a minimal append-only entry is fine.

```
## 2026-MM-DD HH:MM — FROM commander TO chief-of-staff — Phase 0 checkpoint green

Scaffold committed. Branches cut. Toolchain verified. Ready for Phase 1 OPORD.
```

**Milestone:** HANDOFF channel exercised.
**Checkpoint:** The entry appears at the bottom of `ops/HANDOFF.md`.
**Connection:** The primary command channel is operational.

---

## 6. Commit the Phase 0 close

```bash
cd ~/projects/scout
git add ops/HANDOFF.md
git commit -m "Phase 0: command post verified

All checkpoints green. Branches cut, toolchain verified,
HANDOFF round-trip confirmed. Standing by for Phase 1 OPORD."
```

---

## § Verification (Phase 0 checkpoint)

All of these must be green before declaring Phase 0 closed:

- [ ] `git -C ~/projects/scout branch` shows `main` plus five `sector/*` branches.
- [ ] `cargo check` succeeds against the scaffold.
- [ ] `tmux list-sessions` shows the `scout` session.
- [ ] Your HANDOFF entry appears in `ops/HANDOFF.md`.
- [ ] `ops/state/*.json` all still show `"status": "unmobilized"` (unchanged — agents not yet launched).

When every box is checked, return to the Chief of Staff (resume our conversation) with **"Phase 0 green"**. I will:
1. Author `ops/phase-1-council.md` — the Phase 1 runbook.
2. Draft Phase 1 OPORD updating `ops/OPORD.md`.
3. Brief you on how the five Council officers get launched into tmux panes and what outputs you'll see.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `git init -b main` flag unknown | Old git (<2.28) | `git init && git branch -m master main` |
| `cargo check` fails to find a toolchain | Rust not on PATH | Ensure `~/.cargo/bin` is on PATH (rustup adds this) |
| `claude --version` not found | Claude Code not installed | Install per the Claude Code docs before Phase 1 |

## When to call the commander

If any of the following happen during Phase 0, stop and escalate:
- You cannot make `cargo check` succeed after two attempts.
- You find edits in `pathexplorer` (that project must stay untouched).
- Any command prompts you for a password or MFA you weren't expecting.
