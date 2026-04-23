# Position Paper — Surgeon: Reliability & Triage

**Author:** council-surgeon
**Date:** 2026-04-23
**Phase:** 1, Wave 1
**Portfolio:** Failure modes, partial-state handling, panic discipline, crash recovery, observability minimum.

Pathexplorer is not mounted; I reason from Architect's pipeline, Security's trust boundaries, and Quartermaster's roster.

---

## 1. Failure modes across the lifecycle

**Indexing.** (a) `ignore`'s parallel walker hits a wedged FUSE mount; a worker parks on `stat`, progress stalls, banner never clears. (b) A directory vanishes between `read_dir` and `metadata` (rsync, `cargo clean`, branch switch); unhandled `ENOENT` aborts the scan at 90%. (c) Disk fills mid-insert; `SQLITE_FULL` rolls back the batch; user sees "indexed 0 files" after a long wall-clock. (d) Non-UTF-8 path — lossy conversion writes a corrupted string and the later action `ENOENT`s.

**Searching.** (a) Stale paint: `g`-worker returns after `go`-worker and overwrites the correct top hit. (b) Matcher panics on pathological input — search thread dies, UI hangs at "searching..." because nothing joins the dead worker. (c) Long indexer write starves readers at WAL checkpoint; 200–400 ms stutter under load.

**Action execution.** (a) `execvp` returns `ENOENT` because `subl` is missing from `PATH`; TUI exits, no `print` reaches the shell, `cd` silently does not happen. (b) Detached spawn inherits the controlling terminal and swallows keystrokes after SCOUT exits — parent shell wedged. (c) Step 2 references `{env.FOO}` set by step 1, which failed with `on_failure = "continue"`; empty interpolation runs `rm ""`. Security's closed-placeholder rule must declare "env set by a failed step is undefined, not empty".

**Configuration loading.** (a) TOML parse fails at line 47 with a one-line message; SCOUT exits and the user does not know which action broke. (b) Schema version newer than binary (dotfiles upgraded faster than SCOUT); silent drop of unknown fields becomes drift. (c) System + user configs disagree on names; last-load-wins race.

## 2. Partial-state handling

**Interrupted indexing.** One transaction per batch of N≈1000 — per-file kills throughput, per-walk is amnesia. SIGINT drops the current batch; prior batches are durable. Do not resume: re-walking beats cursor tracking. A `scan_generation` column advances only on *completed* scans; a partial scan leaves the old generation intact, so searches serve the previous consistent set.

**Stale entries.** A path in the DB but absent on disk must not reach `execvp`. `stat` only the chosen candidate at action time; if gone, tombstone (`deleted_at`) and fall to the next. Tombstone — not `DELETE` — preserves frecency across a temporary unmount.

**Half-written rows.** SQLite is atomic per row; the logical hazard is a crash between "insert path" and "insert first visit". Put both in one `BEGIN IMMEDIATE`; make `visits` `NOT NULL DEFAULT 0` so a path without a visit is still valid.

**WAL corruption.** Power loss on a filesystem without barrier semantics (old `ext4 data=writeback`, some NFS) can truncate the WAL. Open with `journal_mode=WAL` + `synchronous=NORMAL`; run `PRAGMA integrity_check` at startup when the clean-shutdown sentinel is absent; on failure, rename `index.db*` → `index.db.corrupt-<epoch>` and rebuild. Never in-place repair.

**Filesystem races.** TOCTOU between walk and stat: treat `ENOENT`/`ENOTDIR` as "skip, log at debug", never fatal. Security's `O_NOFOLLOW` on config open must extend to any future file-content read.

## 3. Panic discipline

Panics in a TUI poison the alt-screen and leave the terminal raw. Rule: **panic only for programmer errors code review cannot catch** — broken invariants on our own types, `unreachable!()` after exhaustive matches. Every I/O and external input returns `Result`.

`panic::set_hook` at `main`: (1) `disable_raw_mode`, (2) write payload + backtrace to `$XDG_STATE_HOME/scout/panic-<epoch>.log`, (3) print a one-line pointer on stderr, (4) exit 101. The TUI also owns a raw-mode guard whose `Drop` runs on unwind — belt and suspenders.

Operator impact: a search-worker panic must not down the UI. Workers run under a join-handle monitor that logs, bumps `search_panics_total`, and respawns up to three times per minute before surfacing "search degraded, see log". An index-worker panic aborts the scan only, tombstones the partial batch, and leaves the previous `scan_generation` intact.

## 4. Crash-recovery story

**SIGINT mid-index.** `signal-hook` flips an `AtomicBool`; workers check between batches and exit clean; transaction rolls back; UI prints `"indexing cancelled at 14,203 / ~90,000 paths"`, exits 130. Next launch: `scan_generation` unchanged, previous results still served. Banner `"last full index: 2026-04-22"` appears if older than 7 days, cueing `scout index`.

**Corrupt DB on open.** `integrity_check` runs when the clean-shutdown sentinel is missing. On failure: rename, print `"index was damaged (likely power loss); saved old copy at <path>, rebuilding."`, rebuild. Rename — not delete — so a user who valued the frecency history can post-mortem.

**Schema mismatch.** Migrations numbered, idempotent, applied in a single transaction. A migration panic aborts startup with schema version and failing statement; DB stays at prior version, never between.

**Lockfiles.** SQLite sidecars (`-journal`, `-wal`, `-shm`) after `kill -9` are SQLite's own recovery — do not delete. `scout.lock` (flock) prevents concurrent indexers; stale lock (PID dead) breaks with a warning.

## 5. Observability minimum

Default `info`; `RUST_LOG=scout=debug` for triage; `--trace` writes to file in addition to stderr.

**Structured logs (`tracing`):** `index.{start,batch,complete,error{kind}}` with kind ∈ `{enoent,enotdir,loop,permission,other}`; `search.{query,partial,stale_dropped}`; `action.{spawn{argv_hash},exit,failed{kind}}` (hash — paths may be sensitive); `config.{load{sha256,actions},reject{reason}}`.

**Counters** (in-memory; dumped on SIGUSR1 and exit): `paths_indexed_total`, `index_errors_total{kind}`, `queries_total`, `search_panics_total`, `stale_queries_dropped_total`, `actions_spawned_total{name}`, `action_failures_total{name,kind}`, `db_integrity_failures_total`.

**Panic log** at `$XDG_STATE_HOME/scout/panic-<epoch>.log`; keep the last 16. Referenced from `--version --verbose` so users find it without docs.

**Explicitly not:** no phone-home telemetry (violates offline-first), no Prometheus endpoint (we are a CLI, not a daemon), no sampling (volume is not the problem — *finding* a failure is).

---

**Key claim.** Three doctrines keep SCOUT runnable for a decade without a maintainer-at-the-wheel: (1) **partial-state is normal** — every write transactional, every complete scan bumps a generation, stale entries tombstone, corrupt DBs rename-then-rebuild; (2) **panic only on programmer error** — every I/O returns `Result`, a global hook plus a `Drop` guard keep the terminal cooked on unwind; (3) **observability is a user-facing feature** — structured logs, a small fixed counter set, and a readable panic log turn "I'll try restarting" into "here is the exact file and line that broke". The load-bearing risk I will fight for in the ADRs is crash-recovery around the index: silent data loss on SIGINT or power loss is the failure users never forgive.
