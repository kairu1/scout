# Position Paper — Architect: Design Options

**Author:** council-architect
**Date:** 2026-04-23
**Phase:** 1, Wave 1
**Portfolio:** System design, data flow, ranking, action composition, module boundaries.

Pathexplorer reference terrain is not mounted in this sandbox; citations to it are omitted rather than fabricated. Intel's recon paper (`positions/council-intel.md`) stands in for that ground truth.

---

## 1. Ranking — candidate frecency formulas

Five shapes were considered.

- **A. Pure recency.** `score = -age`. Dead simple. Fails the daily-driver case: a project I touch every day loses to a directory I visited once five minutes ago.
- **B. Hit-count only.** `score = visits`. Old favourites dominate; abandoned projects never drain. No notion of "cooling".
- **C. zoxide-style tiered frecency.** `score = visits × age_weight(age)` with stepped weights (within hour ×4, day ×2, week ×0.5, older ×0.25). Correct intuition, but the tier cliffs create visible rank jitter at boundaries, and "visits" is an unbounded integer that grows forever.
- **D. Continuous exponential decay.** `S = Σ exp(-λ(now − tᵢ))` over all visits, collapsible to a single scalar maintained incrementally:
  `S ← S · exp(-λ · (now − last_update)) + 1; last_update ← now`.
  With half-life `t½ = 7 days`, `λ = ln 2 / 7 ≈ 0.099 /day`. A visit from 7d ago weighs 0.5 relative to now; 30d ≈ 0.05; 90d ≈ 0.000 in the tail. Two columns (`S`, `last_update`) per path, no tiers, no sweep job.
- **E. Query-time blend.** Once the user types, rank becomes `α · match_score + β · frecency + γ · path_bonus`, normalised per candidate set.

**Lean: D as the storage primitive, E as the runtime blend.** Reasons:

1. **Two fields, no migration on tuning.** Change `λ` and the scoring still works; tier schemes force a data migration when the cliffs move.
2. **Read-time score is a pure function.** `S_now = S · exp(-λ · Δt)` — computed in the query, no background decayer.
3. **Composes with match score.** Zero-query state ranks on `S` alone; typed-query ranks on normalised `0.6·match + 0.4·S` (initial constants; tune in ADR-001 after measurement).
4. **Bounded-abuse cheap.** Clamp per-visit increment to one per 10s window (dedup rapid `cd` bouncing); cap `S` at 10⁴ to keep f64 precision over a decade.
5. **Tie-breakers are orthogonal:** raw visit count → shorter path → lexicographic.

zoxide's tiers are a piecewise approximation of D. We take the limit.

## 2. Data flow — keystroke to render

One atomic generation counter `GEN: AtomicU64` is the spine of the pipeline. Every keystroke bumps it.

```
ui_thread:
  on_key(k):
    buf.push(k)
    g = GEN.fetch_add(1, SeqCst)
    token = tokens.replace(g)          # trips previous token
    query_tx.send(Query { text: buf.clone(), gen: g, token })

search_worker (N=available_parallelism):
  loop:
    q = query_rx.recv()
    if q.gen < GEN.load(): continue    # pre-stale, skip
    for chunk in index.stream(cursor): # paginated, bounded memory
      if q.token.is_cancelled(): break
      scored = matcher.score(chunk, &q.text)
      partial_tx.send(Partial { gen: q.gen, rows: ranked(scored) })

ui_thread:
  on_partial(p):
    if p.gen < last_rendered_gen: drop
    merge(p.rows); redraw()
```

Cancellation lives in two places: the `CancellationToken` the worker checks between chunks (coarse, every ~1k rows), and the UI's `last_rendered_gen` filter on inbound partials (fine, per message). Three discard layers — pre-stale drop at worker entry, token trip mid-scan, UI generation filter on render — so no single layer must be correct alone.

Index streams from SQLite via a prepared `SELECT ... ORDER BY rowid` with a rowid cursor. Paginated pulls (say, 4k rows/chunk) cap worker memory regardless of tree size. No in-memory buffering of the full index.

Debounce only the *render*, not the *query*. Queries fire every keystroke; the UI coalesces partials at 16ms. This keeps perceived latency low while clipping redraw thrash.

## 3. Action composition

Actions are declarative TOML. One `[[action]]` is a named ordered list of steps; steps execute sequentially; arguments interpolate at argv level, never shell level.

```toml
[[action]]
name = "edit-then-cd"
description = "Open in Sublime; cd in the parent shell"
on_failure = "abort"     # or "continue"
steps = [
  { kind = "spawn",  argv = ["subl", "{path}"] },
  { kind = "print",  format = "cd {path}" },
]
```

**Template variables** (closed set; unknown placeholders are a config error, not silent empty strings):
`{path}`, `{name}`, `{parent}`, `{ext}`, `{repo_root}`, `{query}`, `{home}`.

**Step kinds:**
- `spawn` — `execvp(argv)`; detached unless `wait = true`. Each `{var}` fills exactly one argv slot; no word-splitting, no glob. This closes the command-injection surface Intel flagged.
- `print` — write a formatted line to stdout. Canonical use: `cd` can't be spawned from a child, so the shell wrapper `eval`s SCOUT's stdout. This is why `print` is first-class, not a footnote.
- `env` — set a variable for later steps in the same action.
- `copy` — clipboard (pending Quartermaster on `arboard`).

**Shell opt-in is explicit.** If the user wants a one-liner, they write `argv = ["sh", "-c", "cd {path} && make"]` — the shell is a chosen runtime, not a default. Unsafe-by-default is how broot and Raycast get injection bugs.

**Chaining semantics.** Sequential only. `on_failure = "abort"` halts the chain on non-zero exit; `"continue"` proceeds. No conditionals, no branches, no loops in v1 — a declarative action language stays declarative. Parallel steps are out of scope. Reuse is by duplication; a future `include = "other-action"` is deferred until demand is real.

The "open in editor then cd" workflow is the canonical case and fits cleanly: spawn (detached) + print (eval'd upstream).

## 4. Module boundaries in Rust

Single crate, library + binary, internal module split. A workspace is not worth the ceremony at this size.

```
scout/
  src/
    main.rs       CLI arg parse, wiring
    lib.rs        re-exports, facade
    ranking/      frecency algebra — pure, no I/O, no deps on other modules
    index/        SQLite schema, migrations, walker, visit upsert
    search/       query pipeline, matcher integration, scoring blend
    actions/      Action, Step, executor
    config/       TOML parse, validation, schema version
    ui/           ratatui event loop, draw, action overlay
    ipc/          channels, GEN counter, CancellationToken
```

**Dependency DAG (strict, one-way):**
`ranking` ← `index` ← `search`; `config` ← `actions`; `ipc` ← `ui`, `search`; `ui` → `search`, `actions`, `config`. No reverse edges. The DAG makes sector ownership enforceable: `sector/search` cannot accidentally edit `ui/` because the compile graph doesn't let it reach in.

**Types crossing boundaries (the small shared vocabulary):**
- `Candidate { id, path, S, last_update, visits }` — index → search.
- `Query { text, gen: u64, token: CancellationToken }` — ui → search.
- `Ranked { candidate, match_score, frecency_score }` — search → ui.
- `Action`, `Step`, `Context { path, query, env }` — config → actions.
- `IndexStream` — a bounded cursor; index → search.

**Traits earn their weight only where swap is concrete, not hypothetical:**
- `Matcher` — `fuzzy-matcher` today, `nucleo` tomorrow (Intel flagged the swap).
- `ActionExecutor` — real vs dry-run, for `--print` previews and tests.
- `Store` — SQLite in prod; in-memory impl for integration tests without booting the full DB.

No trait for `Candidate`, `Step`, `Config` — plain structs. Polymorphism there only simulates complexity.

---

**Key claim:** exponential-decay frecency (one scalar per path) plus a generation-counter pipeline with three discard layers plus argv-level action templates is the smallest design that fulfils commander's intent. Tier cliffs, shell interpolation, and plugin runtimes are the three traps to refuse.
