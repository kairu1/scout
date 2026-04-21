# Phase 0 Runbook — Mobilization

> **Milestone:** Command post live. Container image built. Container running with host volume mounted. Main branch initialised. Sector branches cut. Smoke HANDOFF entry round-trips.
>
> **Checkpoint:** Every item in § Verification below is green.
>
> **Connection:** Phase 1 unlocks — War Council convenes inside the container with the five deliberators.

---

## 0. Prerequisites

Run these checks on your Mac before touching anything else.

```bash
# You have podman installed
podman --version
# Expected: podman version 4.x or newer

# The podman machine is running
podman machine list
# Expected: at least one machine with CURRENT VM marked * and STATE: running
```

If the machine is stopped:
```bash
podman machine start
```

If you don't have a machine yet (first-time install):
```bash
podman machine init --cpus 4 --memory 8192 --disk-size 60
podman machine start
```

**Milestone:** podman machine running.
**Checkpoint:** `podman info` prints without error.
**Connection:** The VM exists; we can build images.

---

## 1. Navigate to the command post

```bash
cd ~/@kairu/@projects/@shell/scout
pwd
# Expected: /Users/tc/@kairu/@projects/@shell/scout
ls
# Expected: Cargo.toml  CLAUDE.md  Containerfile  README.md  docs  ops  src  tests
```

If the listing is wrong, **stand down and escalate** — the scaffold was incomplete.

---

## 2. Initialise the host-side git repository

The scaffold files exist on disk but are not yet under version control. Bring them in, then cut sector branches.

```bash
cd ~/@kairu/@projects/@shell/scout

# Init on main (not master)
git init -b main

git add .
git status
# Eyeball the staged files — confirm nothing unexpected (e.g., no .db files)

git commit -m "Phase 0: scaffold SCOUT command post

Operation SCOUT scaffolded per commander's approved campaign plan.
No implementation code; no dependencies; agents not yet launched.
Reference terrain (pathexplorer) left untouched.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
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

## 3. Build the container image

```bash
cd ~/@kairu/@projects/@shell/scout
podman build -t scout:latest -f Containerfile .
```

This takes ~3–5 minutes on first run (apt layers, Rust install, Claude Code install). Subsequent builds are cached.

**Milestone:** Image built.
**Checkpoint:**
```bash
podman images | grep scout
# Expected: localhost/scout   latest   <hash>   <time>   ~1-2 GB
```

**Troubleshooting:** if the Claude Code install step fails, it's almost always a network hiccup inside the VM. Re-run the build; layer cache will skip completed steps.

---

## 4. Start the container

One long-lived container. We'll attach tmux later for multi-pane work.

```bash
podman run -d \
  --name scout \
  --hostname scout \
  -v ~/@kairu/@projects/@shell/scout:/workspace:Z \
  -w /workspace \
  scout:latest \
  sleep infinity
```

Flags explained:
- `-d` — detached; container runs in background.
- `--name scout` — stable name for future `podman exec` calls.
- `-v <host>:/workspace:Z` — mount the project read-write; `:Z` relabels for SELinux compatibility.
- `-w /workspace` — start in the mounted dir.
- `sleep infinity` — keeps the container alive; we'll enter with `podman exec`.

**Milestone:** Container running.
**Checkpoint:**
```bash
podman ps
# Expected: one line showing scout container, STATUS: Up
```

---

## 5. Enter the container and smoke-test the toolchain

```bash
podman exec -it scout bash
```

Inside the container (prompt should read `scout@scout:/workspace$`):

```bash
whoami      # Expected: scout
pwd         # Expected: /workspace
ls          # Expected: the project files, mounted from host
rustc --version   # Expected: rustc 1.x
cargo --version   # Expected: cargo 1.x
claude --version  # Expected: claude code CLI version string
tmux -V     # Expected: tmux 3.x
sqlite3 --version # Expected: sqlite version
```

Any missing tool → **exit, destroy the container (`podman rm -f scout`), investigate the Containerfile**.

### Compile smoke test

With zero dependencies declared, the placeholder main should build cleanly.

```bash
cargo check
# Expected: Compiling scout v0.0.0 ... Finished
```

**Milestone:** Toolchain verified inside the sandbox.
**Checkpoint:** All version commands produced output; `cargo check` succeeded.
**Connection:** The container is a fit battlefield for deployed agents.

---

## 6. tmux smoke test

Still inside the container:

```bash
tmux new-session -d -s scout -n cos
tmux list-sessions
# Expected: scout: 1 windows ...
```

Attach briefly to confirm it renders:

```bash
tmux attach -t scout
# You're now in tmux. Prefix is Ctrl-b by default.
# Detach with: Ctrl-b then d
```

Leave the session running — Phase 1 will reuse it.

**Milestone:** tmux session ready for officer deployment.
**Checkpoint:** `tmux list-sessions` shows the `scout` session.
**Connection:** Phase 1 can spawn War Council panes here.

---

## 7. Smoke HANDOFF round-trip

Exit the container shell (back to Mac):
```bash
exit
```

On the Mac, write an acknowledgement to HANDOFF.md — this verifies the volume mount is bidirectional. Use your editor of choice; a minimal append-only entry is fine.

Append this entry at the bottom of `ops/HANDOFF.md` on the host:

```
## 2026-MM-DD HH:MM — FROM commander TO chief-of-staff — Phase 0 checkpoint green

Container built and running. Branches cut. Toolchain verified. Ready for Phase 1 OPORD.
```

Now verify the container sees the same content:
```bash
podman exec scout tail -n 6 /workspace/ops/HANDOFF.md
```
You should see your entry. Bidirectional mount confirmed.

**Milestone:** Host ↔ container file sync verified via HANDOFF round-trip.
**Checkpoint:** The `tail` output includes your commander entry.
**Connection:** The primary command channel is operational.

---

## 8. Commit the Phase 0 close

On the Mac:

```bash
cd ~/@kairu/@projects/@shell/scout
git add ops/HANDOFF.md
git commit -m "Phase 0: command post verified

All checkpoints green. Container up, branches cut, toolchain verified,
HANDOFF round-trip confirmed. Standing by for Phase 1 OPORD.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## § Verification (Phase 0 checkpoint)

All of these must be green before declaring Phase 0 closed:

- [ ] `podman images | grep scout` → `localhost/scout latest ...`
- [ ] `podman ps` → `scout` container `Up`
- [ ] `git -C ~/@kairu/@projects/@shell/scout branch` shows `main` plus five `sector/*` branches
- [ ] `podman exec scout cargo check` succeeds
- [ ] `podman exec scout tmux list-sessions` shows `scout` session
- [ ] Your HANDOFF entry appears in `podman exec scout cat /workspace/ops/HANDOFF.md`
- [ ] `ops/state/*.json` all still show `"status": "unmobilized"` (unchanged — agents not yet launched)

When every box is checked, return to the Chief of Staff (resume our conversation) with **"Phase 0 green"**. I will:
1. Author `ops/phase-1-council.md` — the Phase 1 runbook.
2. Draft Phase 1 OPORD updating `ops/OPORD.md`.
3. Brief you on how the five Council officers get launched into tmux panes and what outputs you'll see.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `podman build` stalls on apt | Network flakiness in the VM | Re-run; layer cache handles it |
| `podman run` errors `container exists` | Leftover from prior attempt | `podman rm -f scout` and re-run |
| Files inside container appear owned by unexpected UID | UID mismatch host↔VM | Ensure `-v ... :Z` is present; on rootless podman the UID mapping handles this automatically |
| `git init -b main` flag unknown | Old git (<2.28) | `git init && git branch -m master main` |
| Claude Code install fails in build | Upstream network hiccup | Re-run the build; if persistent, pin a version in the Containerfile and escalate |

## When to call the commander

If any of the following happen during Phase 0, stop and escalate:
- You cannot make `podman build` succeed after two attempts.
- You find edits in `pathexplorer` (that project must stay untouched).
- Any command prompts you for a password or MFA you weren't expecting.
- The container image size exceeds ~3 GB (investigate bloat before moving on).
