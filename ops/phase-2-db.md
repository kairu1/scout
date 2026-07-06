# Phase 2 Runbook — The DB Takes the Hill

> **Milestone:** 2nd Rifles ships the frecency-capable index foundation ADR-001 binds — schema, migrations, WAL, streaming indexer, visit path, crash recovery.
>
> **Checkpoint:** Success criteria in `ops/OPORD.md` all green.
>
> **Connection:** Phase 3 unlocks — 1st Rifles, 3rd Rifles, Engineers engage in parallel, pulling ranked candidates through the seams 2nd Rifles built.

---

## Briefing

Single line officer (`rifles-2`) engages. Four sequential engagements. Two commander check-ins. Ship order: **schema first, correctness everywhere, performance last.** A wrong schema in Phase 2 is a migration in Phase 4; a slow walk in Phase 2 is a tuning PR in Phase 3.

**Wall-clock estimate:** 2–4 hours across four engagements.
**Cost:** ~8–12 agent turns across four one-shot Claude sessions.
**Commander check-ins:** two — after engagement 2, after engagement 4.

**Force active:** `rifles-2` only. All other line officers remain unmobilized.

---

## Preflight

These steps are commander (and Chief of Staff) acts. Execute in order; none are reversible-by-accident but each has a clean restart path.

### 0. Confirm container and `.gitignore`

On the Mac:

```bash
podman ps
# Expected: scout container Up

grep -q '^/worktrees/' ~/projects/scout/.gitignore && echo "gitignore OK" || echo "FIX"
# Expected: gitignore OK
```

If the container was stopped since Phase 1:

```bash
podman start scout
```

**Milestone:** container live, `.gitignore` prepared for worktrees.
**Checkpoint:** both commands return expected output.
**Connection:** the battlefield is set; worktree creation will not accidentally pollute the main working tree.

### 1. Create the sector/index worktree (Chief of Staff act)

On the Mac, from the repo root:

```bash
cd ~/projects/scout
git worktree add worktrees/sector-index sector/index
```

Confirm:

```bash
git worktree list
# Expected:
#   ~/projects/scout                    <hash> [main]
#   ~/projects/scout/worktrees/sector-index <hash> [sector/index]
```

The worktree directory now exists on disk at `worktrees/sector-index/` and is pinned to the `sector/index` branch. Because `/worktrees/` is in `.gitignore`, the main working tree will not see its contents as untracked.

**Milestone:** `worktrees/sector-index/` exists as a physically isolated checkout of `sector/index`.
**Checkpoint:** `git worktree list` shows both paths; `ls worktrees/sector-index/` shows the repo contents.
**Connection:** 2nd Rifles has a battlefield that cannot be disturbed by any other officer's `git checkout`.

### 2. Activate rifles-2 state

On the Mac (writing to the main working tree — state files live here):

```bash
cd ~/projects/scout
```

Edit `ops/state/rifles-2.json` to:

```json
{
  "officer": "rifles-2",
  "role": "line-officer",
  "sector": "Core/Index",
  "branch": "sector/index",
  "worktree": "worktrees/sector-index",
  "status": "activated",
  "current_task": null,
  "last_update": "2026-04-24T16:30:00Z",
  "blockers": [],
  "notes": "Phase 2 activated. Awaiting engagement 1 launch."
}
```

(Use your actual timestamp.)

**Milestone:** `rifles-2` state reflects activation.
**Checkpoint:** `cat ops/state/rifles-2.json | jq .status` returns `"activated"`.
**Connection:** the silence-is-going-dark rule now applies to 2nd Rifles; any engagement launch updates `status` to `engaging`.

### 3. Confirm Claude Code auth inside the container

```bash
podman exec -it scout bash -c 'cd /workspace/worktrees/sector-index && claude -p "say hello"'
```

Expected: a sensible reply. If auth prompts, complete it. If the container cannot complete the flow, fall back to the `~/.claude` host-mount or `ANTHROPIC_API_KEY` patterns documented in `ops/phase-1-council.md` §Preflight.

**Milestone:** Claude Code responds from inside the worktree.
**Checkpoint:** no auth prompt; a real reply prints.
**Connection:** `rifles-2` can be launched.

### 4. Commit the Phase 2 preflight

On the Mac:

```bash
cd ~/projects/scout
git add .gitignore ops/state/rifles-2.json ops/OPORD.md ops/phase-2-db.md ops/playbook.md
git commit -m "Phase 2: OPORD issued; rifles-2 activated; worktree created"
```

(The playbook update is optional if it already links `phase-2-db.md`.)

---

## 🪖 Check-in #0 — Before engagement 1

Before launching any engagement, read:

- `ops/OPORD.md` (Phase 2 mission)
- `docs/adr/001-ranking-doctrine.md` (binds the schema and pacing)
- `docs/adr/002-dependency-roster.md` §Consequences §Binds-2nd-Rifles (admitted crates)
- `docs/adr/003-threat-model.md` §4 (DB file permissions, `O_NOFOLLOW`, parameter-bound SQL)
- `docs/adr/positions/council-surgeon.md` §2, §3, §4 (partial-state, panic discipline, recovery)

If any engagement prompt below asks for the wrong thing, edit it before launching. Prompts are where commander leverage is highest.

---

## Engagement 1 — Schema + forward migration

**Pattern:**

```bash
podman exec -it scout bash -c 'cd /workspace/worktrees/sector-index && claude -p --dangerously-skip-permissions "<PROMPT>"'
```

Or drop into interactive Claude:

```bash
podman exec -it scout bash -c 'cd /workspace/worktrees/sector-index && claude --dangerously-skip-permissions'
```

**Prompt:**

```
You are rifles-2. Phase 2 Engagement 1 — Schema + forward migration.

Read, in this order:

  /workspace/worktrees/sector-index/CLAUDE.md
  /workspace/worktrees/sector-index/ops/OPORD.md
  /workspace/worktrees/sector-index/docs/adr/001-ranking-doctrine.md
  /workspace/worktrees/sector-index/docs/adr/002-dependency-roster.md
  /workspace/worktrees/sector-index/docs/adr/003-threat-model.md
  /workspace/worktrees/sector-index/docs/adr/positions/council-surgeon.md

Update /workspace/ops/state/rifles-2.json: status "engaging",
current_task "Engagement 1 — Schema + migration", last_update now.

Your engagement:

  1. Create the `migrations/` directory at the repo root if absent.
  2. Author `migrations/0001_initial.sql` with the exact schema ADR-001
     §Consequences binds:
       - path TEXT NOT NULL
       - S REAL NOT NULL DEFAULT 0
       - last_update INTEGER NOT NULL DEFAULT 0
       - visits_total INTEGER NOT NULL DEFAULT 0
       - scan_generation INTEGER NOT NULL DEFAULT 0
       - tombstoned_at INTEGER
       - rowid as the implicit PK
     Plus a UNIQUE index on canonical path. Two metadata tables in the
     same migration:
       - `schema_version` — single-row, holds the current schema integer.
       - `run_state` — single-row, holds:
           current_generation INTEGER NOT NULL DEFAULT 0
           last_complete_generation INTEGER NOT NULL DEFAULT 0
           last_run_started_at INTEGER
           last_run_completed_at INTEGER
         The `current_generation` advances only on a cleanly-finished
         walk (Surgeon §2). Engagement 3 reads and writes this table;
         shipping it in 0001 keeps the schema in one migration.
  3. Author `src/index/mod.rs`, `src/index/schema.rs` with:
       - `apply_migrations(conn: &Connection) -> Result<()>` that runs
         every `.sql` file in `migrations/` in lexical order inside a
         single transaction per file.
       - `schema_version(conn: &Connection) -> Result<u32>` reader.
       - A compile-time-embedded migration list using `include_str!`
         (no filesystem read at runtime — portability).
  4. Wire `src/index/` into `src/lib.rs` (create `lib.rs` if absent)
     and keep `src/main.rs` a thin shim that calls into the library.
  5. Unit tests under `tests/index_schema.rs` covering:
       - Fresh DB → apply migrations → schema_version reports 1.
       - Re-apply migrations on an already-migrated DB is idempotent.
       - `sqlite3` inspection via a Rust in-memory connection asserts
         every column ADR-001 names, each with the correct type and
         NOT NULL constraint.

Doctrinal constraints — NON-NEGOTIABLE:

  - Every SQL statement is parameter-bound. No `format!`-assembled SQL
    anywhere in this engagement or any future one. (ADR-003 §1.)
  - Deps permitted in this engagement: `rusqlite` (with `bundled` +
    features as needed) and `tracing` for observability. `signal-hook`
    arrives later; `ignore` arrives at engagement 3. Nothing else.
  - Cargo.toml: add only the admitted deps. Pin `rusqlite = "0.31"`,
    `tracing = "0.1"`, `tracing-subscriber = "0.3"`. Do not add any
    crate not in the ADR-002 roster.
  - Path canonicalisation is inserted at the boundary that writes the
    path column. You can stub this as `fs::canonicalize` for now; the
    walker in engagement 3 will call it.

Commit discipline:

  - Commit on `sector/index` inside this worktree.
    `git status` first; stage only the files you authored; verify no
    stray files; commit with a message naming the engagement.
  - Do not `git checkout`. Do not `git worktree` anything.
  - Do not merge to main.

Close by updating state file:
  status "standby", last_update now,
  notes "Engagement 1 complete — schema + migration; N tests pass;
         Cargo.toml permits only ADR-002 Phase 2 admitted deps".

Verify before standdown:

  cargo check        # must compile
  cargo test         # all new tests must pass

Terse, decisive, commander-frame. No filler.
```

**Milestone:** Schema matches ADR-001 §Consequences on disk.
**Checkpoint:**
- `cat migrations/0001_initial.sql` shows every column.
- `cargo test` passes inside the worktree.
- `git log --oneline` on `sector/index` shows the engagement-1 commit.
**Connection:** Engagement 2 can configure WAL against the fresh schema.

---

## Engagement 2 — WAL + PRAGMA configuration

**Prompt:**

```
You are rifles-2. Phase 2 Engagement 2 — WAL + PRAGMA.

Confirm you are in /workspace/worktrees/sector-index and on branch
sector/index. If not, halt and escalate via HANDOFF @cos.

Re-read ADR-001 §Index-writer pacing. Update /workspace/ops/state/
rifles-2.json: status "engaging", current_task "Engagement 2 — WAL +
PRAGMA", last_update now.

Your engagement:

  1. Author `src/index/pragma.rs` with:
       - `open(path: &Path) -> Result<Connection>` that:
         a. Opens with `rusqlite::Connection::open` (O_NOFOLLOW on
            the final path component per ADR-003 §3 — if rusqlite
            does not expose it, a `std::fs::OpenOptions` pre-check
            with `O_NOFOLLOW` before handing the path to rusqlite is
            acceptable).
         b. Applies: `PRAGMA journal_mode = WAL`,
            `PRAGMA synchronous = NORMAL`,
            `PRAGMA wal_autocheckpoint = 0`.
         c. Runs migrations via engagement-1's `apply_migrations`.
         d. Returns the configured connection.
       - `QUERY_ACTIVE: AtomicU64` in `src/ipc/mod.rs` (create the
         module; ADR-001 §Index-writer pacing §3 names it). It holds
         the wall-clock ms of the most recent query; readers load it,
         writers don't touch it from this engagement — search workers
         in Phase 3 write it. Stub it at 0 for now.
  2. Author `src/index/mode.rs` or extend `pragma.rs` with a helper
     that queries back the three PRAGMAs to confirm they landed.
  3. Unit tests under `tests/index_pragma.rs`:
       - Fresh DB opened via `pragma::open` reports WAL journal_mode.
       - `synchronous` reports `NORMAL` (value 1).
       - `wal_autocheckpoint` reports 0.
       - Opening a symlinked DB path refuses (fixture: create a file,
         symlink to it, pass the symlink path).

Doctrinal constraints carry from engagement 1. No new deps.

File creation permissions:

  - The DB file at first creation is mode 0600. The parent directory,
    if SCOUT creates it, is 0700. (ADR-003 §4.)
  - You create these with explicit `OpenOptions::mode(0o600)` or a
    post-open `fchmod`; do not rely on umask.

Commit on sector/index inside the worktree. Do not merge.

Close by updating state file:
  status "standby", last_update now,
  notes "Engagement 2 complete — WAL + PRAGMA + O_NOFOLLOW; QUERY_ACTIVE
         atomic allocated; N tests pass".

Verify:
  cargo test --test index_pragma

Terse, decisive.
```

**Milestone:** WAL mode active; the three PRAGMAs visible; O_NOFOLLOW refuses a symlinked DB.
**Checkpoint:** `cargo test --test index_pragma` passes.
**Connection:** Engagement 3's walker has a correctly-configured writer to commit into.

---

## 🪖 Check-in #1 — After engagement 2

Commander inspects:

```bash
cd ~/projects/scout/worktrees/sector-index
cargo test --test index_schema --test index_pragma
ls migrations/
# Expected: 0001_initial.sql

# Open a fresh DB and inspect the schema
rm -f /tmp/scout-checkin.db
cargo run --example open-db -- /tmp/scout-checkin.db    # if an example exists
# OR write a quick Rust script, OR:
sqlite3 /tmp/scout-checkin.db "SELECT * FROM schema_version; .schema"
```

Expected in the `.schema` output: every column ADR-001 names, the UNIQUE index on path, the schema_version table.

If the schema drifts from ADR-001, redirect 2nd Rifles via HANDOFF before engagement 3. Cheap to fix now; expensive to migrate out of once engagement 3 inserts rows.

If the schema is clean, proceed.

---

## Engagement 3 — Streaming walker + batched insert

**Prompt:**

```
You are rifles-2. Phase 2 Engagement 3 — Walker + batched insert.

Confirm worktree + branch as before. Re-read:

  /workspace/worktrees/sector-index/docs/adr/001-ranking-doctrine.md
  /workspace/worktrees/sector-index/docs/adr/003-threat-model.md (§1 paths,
    §3 config trust is NOT relevant here — we are indexing paths, not
    loading configs)
  /workspace/worktrees/sector-index/docs/adr/positions/council-surgeon.md
    §1a, §1b, §2 (walker failure modes, batched transactions, partial
    state)

Update state: status "engaging", current_task "Engagement 3 — Walker +
insert", last_update now.

Your engagement:

  1. Add to Cargo.toml in this engagement:
       - `ignore = "0.4"` — the parallel walker. Admitted for Phase 2
         per OPORD §Doctrinal note.
       - `signal-hook = "0.3"` — SIGINT/SIGTERM discipline. Admitted
         for Phase 2 per ADR-002 §Consequences.
     No other crate enters.
  2. Author `src/index/walk.rs`:
       - `WalkConfig { root: PathBuf, follow_symlinks: bool, hidden:
         bool }`.
       - `walk(config: &WalkConfig) -> impl Iterator<Item = Result<
         PathBuf>>` built on `ignore::WalkBuilder::build_parallel`.
       - Each yielded path is canonicalised (`fs::canonicalize`) to
         an absolute, no-`..` form before yield. Canonicalisation
         failure (`ENOENT`, `ENOTDIR`) is a `debug` log, path is
         skipped, walk continues — never fatal (Surgeon §1a).
       - System denylist: paths inside `/proc`, `/sys`, `/dev` are
         refused at canonicalisation boundary (ADR-003 §1 refused).
  3. Author `src/index/insert.rs`:
       - `batched_insert(conn: &mut Connection, paths: impl Iterator<
         Item = PathBuf>, batch_size: usize) -> Result<InsertStats>`
         where `InsertStats { inserted, skipped, errors }`.
       - Each batch is a single `BEGIN IMMEDIATE` (Surgeon §2).
         Default batch size 1000 (Surgeon §2).
       - Insert statement uses named parameters.
         `INSERT INTO paths (path, scan_generation) VALUES (:path,
           :gen) ON CONFLICT(path) DO UPDATE SET scan_generation =
           excluded.scan_generation, tombstoned_at = NULL`.
       - `scan_generation` for a run is read-modify-write against the
         `run_state` table shipped in 0001 (engagement 1): read
         `current_generation` before the walk, walk-into-new rows with
         `current_generation + 1` written into each `paths` row, and
         on cleanly-finished walk update `run_state.current_generation`
         and `last_complete_generation` to the new value with
         `last_run_completed_at = now`. On early return (SIGINT, error)
         the `run_state` row is NOT updated; prior batches remain
         durable but the partial generation never becomes "current".
       - SIGINT/SIGTERM via `signal-hook`: register handlers in
         `src/index/signals.rs` (new module) that flip a shared
         `AtomicBool` "interrupt_requested" exposed via
         `pub fn interrupt_requested() -> bool`. Walker checks it
         between batches; on flip, drops the in-flight batch via
         early return. Never inside a `BEGIN IMMEDIATE`.
  4. Integration test `tests/index_walk.rs`:
       - 100-file fixture tree → walk + insert → row count matches.
       - 1000-file fixture → batched commits visible (measure via
         number of transactions).
       - `scan_generation` does not advance on an early-return walk.
       - Repeat walk (`scan_generation` advances) updates existing
         rows' generation without duplicating paths (UNIQUE enforced).

Performance gate (smoke, not benchmark): walk of a 100 k-path
synthetic tree (you may generate the fixture at test-run time under
`target/fixtures/`) completes in < 30 s wall-clock with RSS staying
under 100 MB. Use `tracing` spans `index.walk.start`,
`index.walk.batch`, `index.walk.complete` so the commander can read
the spans on check-in.

No `sqlx`, `async-std`, `tokio`, `notify`, `chrono`, or anything else
outside the admitted list. The walker is synchronous; `ignore`'s
parallel walk uses rayon-like worker threads internally, which is
fine and does not drag `tokio`.

Commit on sector/index. Do not merge.

Close state:
  status "standby", last_update now,
  notes "Engagement 3 complete — walker + batched insert; fixture walk
         passes; 100k-path smoke under 30s / 100MB RSS".

Verify:
  cargo test --test index_walk

Terse, decisive.
```

**Milestone:** 100 k-path tree walks to completion in < 30 s / < 100 MB RSS; batched commits land cleanly.
**Checkpoint:** `cargo test --test index_walk` passes; `tracing` spans are observable.
**Connection:** Engagement 4 can credit visits against the populated index.

---

## Engagement 4 — Visit path + crash recovery

**Prompt:**

```
You are rifles-2. Phase 2 Engagement 4 — Visit path + recovery.

Confirm worktree + branch. Re-read:

  /workspace/worktrees/sector-index/docs/adr/001-ranking-doctrine.md
    §Visit credit, §Index-writer pacing
  /workspace/worktrees/sector-index/docs/adr/003-threat-model.md §4
  /workspace/worktrees/sector-index/docs/adr/positions/council-surgeon.md
    §3, §4

Update state: status "engaging", current_task "Engagement 4 — Visit
+ recovery", last_update now.

Your engagement:

  1. Author `src/index/visit.rs`:
       - `record_visit(conn: &Connection, candidate_id: i64) ->
         Result<()>` that runs the frecency update in a single
         `BEGIN IMMEDIATE` transaction:
           UPDATE paths
              SET S = min(S * exp(-lambda * (now - last_update)) + 1,
                          10000.0),
                  last_update = now,
                  visits_total = visits_total + 1
            WHERE rowid = :id
              AND tombstoned_at IS NULL;
         `lambda = ln(2) / 604_800` (7-day half-life per ADR-001).
         If SQLite lacks `exp` as a built-in, register it as a scalar
         function via `Connection::create_scalar_function` at `open`
         time (extend engagement-2's `pragma::open`).
       - Median latency target: ≤ 5 ms (ADR-001 §Performance budgets).
         Measure in the unit test; fail if median > 10 ms.
       - Rate limit is CALLER'S responsibility (the action executor in
         Phase 3). This engagement does NOT implement rate limit; a
         one-line doc comment on `record_visit` names the contract.
  2. Author `src/index/recovery.rs`:
       - `clean_shutdown_sentinel_path(db_path) -> PathBuf` =
         `<db_path>.clean`.
       - On `pragma::open`, before applying migrations, check for the
         sentinel. If absent: run `PRAGMA integrity_check`. Budget:
         target ≤ 2 s; hard fail at 10 s (OPORD §3 doctrinal note
         on Surgeon's p99 exclusion). Emit `tracing` span
         `index.recovery.integrity_check` with the elapsed time.
         On `integrity_check` failure, rename `<db_path>*` →
         `<db_path>.corrupt-<epoch>` and create a fresh DB. Emit
         `tracing` event `index.recovery.corrupt_renamed`.
       - On clean shutdown (caller invokes `shutdown(conn) -> Result<()>`
         before drop), write the sentinel; delete on next open.
       - The visit path checks `signals::interrupt_requested()` (the
         AtomicBool that engagement 3 wired via `signal-hook`) between
         statements; mid-transaction is never interrupted — a
         `BEGIN IMMEDIATE` runs to commit or rollback by SQLite's own
         atomicity. This engagement does not register new signal
         handlers; it consumes the ones already in place.
  3. Integration tests under `tests/index_recovery.rs`:
       - `record_visit` median ≤ 5 ms over 1000 repeats on a 100k-row
         index.
       - Fresh DB: sentinel is absent on creation; present after
         `shutdown()`; absent again after next `open()`.
       - A DB with corrupted bytes (fixture: `dd` random bytes into a
         known-valid DB) triggers `integrity_check` failure, renames,
         and creates a fresh index.
       - Mid-walk SIGINT (simulated by flipping the `interrupt_requested`
         AtomicBool during a batch) rolls back the in-flight batch;
         `run_state.last_complete_generation` stays at the prior value.
  4. Update the default `src/main.rs` to expose `scout index <path>`
     and `scout open-db <path>` subcommands that exercise the walker
     and the open/recovery paths respectively. Keep it minimal — this
     is scaffolding for commander to gut-check at Check-in #2, not
     the production CLI (that lands in Phase 3 via `clap` slot 2).
     **Hand-roll the arg parsing** in ~20 lines of `std::env::args`.
     Two subcommands, two paths, no flags beyond positional. `clap`
     is not admitted in Phase 2; its entry is Phase 3's call. Adding
     it here is a doctrine breach — review will reject.

Commit on sector/index. Do not merge.

Close state:
  status "standby", last_update now,
  notes "Engagement 4 complete — visit + recovery + SIGINT; N tests pass;
         record_visit median ≤ 5 ms on 100k-row fixture".

Verify:
  cargo test
  # all index_* tests pass

Terse, decisive.
```

**Milestone:** Visit credit runs in ≤ 5 ms median; integrity_check triggers on missing sentinel; corrupt DB renames and rebuilds; SIGINT leaves prior generation intact.
**Checkpoint:** `cargo test` passes; `tracing` spans visible on the recovery path.
**Connection:** Phase 2 is complete; Phase 3 can query ranked candidates and credit visits on action execution.

---

## 🪖 Check-in #2 — After engagement 4 (Phase 2 close)

Commander gut-checks responsiveness on real data.

```bash
cd ~/projects/scout/worktrees/sector-index

# Build
cargo build --release

# Pick a real tree — e.g., a sizable project directory you have
# on your machine. 100k paths is the stated target.
./target/release/scout index ~/some/large/tree
# Observe: walk completes cleanly; elapsed under 30s; RSS under 100MB.

# Inspect the DB
sqlite3 ~/.local/share/scout/index.db "SELECT count(*) FROM paths;
  SELECT avg(S), max(S) FROM paths;"

# Simulate corruption
cp ~/.local/share/scout/index.db /tmp/corrupt.db
dd if=/dev/urandom of=/tmp/corrupt.db bs=1 count=1024 seek=4096 conv=notrunc
./target/release/scout open-db /tmp/corrupt.db
# Expect: message naming the rename target; a fresh DB gets created.

# SIGINT mid-index
./target/release/scout index ~/very/large/tree &
sleep 3
kill -INT %1
wait
# Expect: clean exit; prior generation intact on next open.
```

If any of the above feels wrong — the walk stutters, the RSS climbs, the integrity_check hangs — flag via HANDOFF with specifics.

If satisfied, the commander authorises the merge of `sector/index` into `main`:

```bash
cd ~/projects/scout
git merge --no-ff sector/index -m "Phase 2: DB foundation merged to main"
```

(The `--no-ff` preserves the sector-branch story in history.)

Remove the worktree (Chief of Staff act):

```bash
git worktree remove worktrees/sector-index
# The sector/index branch persists; the worktree directory is gone.
```

Update `rifles-2.json` to `"status": "standby"`, notes mentioning Phase 2 close.

Append to HANDOFF.md:

```
## 2026-MM-DD HH:MM — FROM rifles-2 TO chief-of-staff — Phase 2 green

Four engagements closed. Success criteria met:
  [list each ticked box from ops/OPORD.md]

Ready for Phase 3 OPORD.
```

---

## § Verification (Phase 2 checkpoint)

All of these must be green before declaring Phase 2 closed. They mirror `ops/OPORD.md` §Success criteria:

- [ ] `git worktree list` shows (or showed, pre-teardown) `worktrees/sector-index` on `sector/index`.
- [ ] `migrations/0001_initial.sql` and `migrations/0002_run_state.sql` exist; match ADR-001 §Consequences schema.
- [ ] `sqlite3 <freshDB> .schema` shows every column ADR-001 names plus the `UNIQUE` index on path and the `run_state` metadata.
- [ ] `cargo test -p scout` passes every index test.
- [ ] 100 k-path synthetic walk completes in < 30 s with RSS < 100 MB on the scout container.
- [ ] `record_visit` median ≤ 5 ms over 1 000 repeats on a 100 k-row fixture.
- [ ] Missing sentinel → `integrity_check` runs; corrupt fixture → rename + rebuild.
- [ ] SIGINT mid-walk rolls back the in-flight batch; prior generation intact.
- [ ] `rifles-2` state at `status: "standby"` with Phase 2 close note.
- [ ] `ops/HANDOFF.md` carries the `Phase 2 green` entry.
- [ ] ADR-002 §Consequences §Binds-2nd-Rifles has a Revision history entry noting the `ignore` admission per this OPORD.
- [ ] `sector/index` merged to `main` via `--no-ff`.
- [ ] `worktrees/sector-index` removed after merge.

When every box is ticked, Chief of Staff drafts Phase 3 OPORD and `ops/phase-3-assault.md` for 1st Rifles + 3rd Rifles + Engineers' parallel engagement.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `git worktree add` fails with "already checked out" | You previously tried to check out `sector/index` in the main working tree | `git checkout main` in the main working tree first; sector branches should never be checked out there. |
| `rusqlite` build fails with "`sqlite3.h` not found" | `bundled` feature not enabled | Ensure Cargo.toml has `rusqlite = { version = "0.31", features = ["bundled"] }`. |
| `integrity_check` hangs on 100 k-row DB | Native query path, not the fast one | Use `PRAGMA quick_check` for the on-open path instead of the full `integrity_check` — it catches the failures that matter (header, page links) without walking every row. ADR-001 and Surgeon's intent both permit this. |
| 100 k-path walk exceeds 100 MB RSS | Agent buffered paths in-memory before insert, contrary to "streaming" requirement | Re-read Surgeon §2 and ADR-001 §Index-writer pacing. Inserts commit every 1 000 paths. No collect-then-insert. |
| `record_visit` median > 10 ms | `BEGIN IMMEDIATE` contention with a long-held reader elsewhere in the test, or a missing `PRAGMA synchronous = NORMAL` | Re-verify the PRAGMA; check that test setup does not hold a reader transaction across the visit measurement loop. |
| Agent imports a crate outside the admission list | Prompt drift | Kill the session. Redo Cargo.toml manually; re-launch the engagement with a tighter prompt. |
| Agent commits to `main` instead of `sector/index` | Worktree not set up, or agent left the worktree | Verify `git rev-parse --abbrev-ref HEAD` inside the worktree reports `sector/index`. If not, the worktree is misconfigured — tear down with `git worktree remove` and recreate. |
| SIGINT mid-walk corrupts the DB | Transaction discipline is wrong | Re-read ADR-001 §Index-writer pacing and Surgeon §2. Each batch is its own `BEGIN IMMEDIATE`; SIGINT between batches drops the current batch cleanly; SIGINT *during* `INSERT` rolls back on commit attempt. Fix the batch framing. |

---

## When to call the commander

Halt and escalate via HANDOFF `@commander` if:

- 100 k-path walk cannot be made to complete in < 30 s / 100 MB after one agent retry.
- The schema must diverge from ADR-001 §Consequences to achieve the performance target.
- Any crate outside the Phase 2 admission list is needed.
- `integrity_check` on a missing-sentinel open path cannot be made to complete within 10 s on a real-world-sized DB.
- You find that `sector/index` has been touched outside the worktree, or that another officer's branch has been checked out in the main working tree.
- Any command prompts for a password or MFA you did not expect.
