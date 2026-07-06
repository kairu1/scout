# Phase 1 Runbook — War Council I: Doctrine & Supply

> **Milestone:** Four ADRs signed by commander. Doctrine set for every downstream engagement.
>
> **Checkpoint:** Success criteria in `ops/OPORD.md` all green.
>
> **Connection:** Phase 2 unlocks — 2nd Rifles engages to build the frecency-capable index.

---

## Briefing

This is the most important phase of the campaign. Four decisions made here — ranking, dependencies, threat model, action schema — shape every engagement downstream. Getting doctrine wrong now is cheap; fixing it in Phase 3 is expensive. Spend the time.

**Wall-clock estimate:** 60–90 minutes across four waves.
**Cost:** ~25 agent turns across five Council officers.
**Commander's check-ins:** four — one after each wave.

**Force active:** five Council officers only. Line officers remain unmobilized.

---

## Preflight

### 0. Confirm the command post

```bash
cd ~/projects/scout
git status
# Expected: on main, working tree as left at Phase 0 close
```

### 1. Verify Claude Code auth

Each Council officer runs as a Claude Code agent. The `claude` binary is installed but may need first-run auth on a fresh machine.

```bash
cd ~/projects/scout && claude
```

You'll be dropped into interactive Claude Code. If it prompts for login, complete the flow, then exit with `/quit`.

**If auth cannot complete interactively (headless machine):** export an API key into your shell rc before launching officers:
```bash
echo 'export ANTHROPIC_API_KEY=<key>' >> ~/.bashrc && source ~/.bashrc
```

**Milestone:** `claude -p "say hello"` returns a reply.
**Checkpoint:** no auth prompt; output is sensible.
**Connection:** Council officers can be launched.

### 2. Create output directories

Still on the Mac — these are committed to git, so create on host:
```bash
cd ~/projects/scout
mkdir -p docs/adr/positions
```

The ADR template already exists at `docs/adr/.template.md`.

---

## 🪖 Check-in #1 — Before Wave 1

Before launching any officer, read:
- `ops/OPORD.md` (Phase 1 mission)
- `ops/AGENTS.md` (force structure)
- The five prompts in § Wave 1 below

If any prompt asks for the wrong thing, edit it before launching. Prompts are where your two cents has the biggest leverage.

---

## Wave 1 — Position Papers (parallel, 5 one-shot agents)

Each Council officer writes a position paper on their portfolio, without committing to ADR-level decisions yet. Papers become source material for Wave 2.

**Pattern per officer:**

```bash
cd ~/projects/scout && claude -p --dangerously-skip-permissions "<PROMPT>"
```

Or, for richer interaction, drop into interactive Claude and paste the prompt:
```bash
cd ~/projects/scout && claude --dangerously-skip-permissions
# then paste the prompt at the Claude prompt
```

You can run officers **sequentially** (simpler, one tmux pane) or **in parallel tmux panes** (faster, more moving parts). For first-time Phase 1, I recommend sequential.

Output per officer: `docs/adr/positions/<callsign>.md`, 400–1200 words.

---

### 1.1 council-intel — Ecosystem Recon

**Prompt:**

```
You are council-intel, the Intelligence officer in Operation SCOUT's War
Council. Read these in order before writing anything:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/AGENTS.md

Update your state file at ~/projects/scout/ops/state/council-intel.json:
set status to "engaging", current_task to "Position paper — ecosystem
recon", last_update to the current ISO-8601 timestamp.

Your job: survey the terrain. Write a position paper to
~/projects/scout/docs/adr/positions/council-intel.md covering:

  1. What each of these tools does well and where each falls short:
     fzf, skim, zoxide, autojump, broot, yazi, fd, ripgrep, telescope
     (neovim), Raycast. Plus any other project-finder/action-launcher
     you judge relevant.
  2. The gap SCOUT fills that none of the above does (action composition
     with portable config — tie back to commander's intent).
  3. Patterns to steal (specific mechanisms, not vibes).
  4. Patterns to avoid (failure modes observed in the ecosystem).
  5. Recommended reading list (crates, prior-art links, design docs).

No code. Prose only. Terse paragraphs; cite specifics. No emoji.
Keep to 600-1000 words.

When done: update state file — status "standby", last_update timestamp,
notes with one-sentence summary of your paper's key claim.
```

**Milestone:** `docs/adr/positions/council-intel.md` exists and is well-formed.
**Checkpoint:** Your quick read confirms it cites real tools with specific claims.
**Connection:** ADR authors in Wave 2 can cite Intel's findings.

---

### 1.2 council-architect — Design Options

**Prompt:**

```
You are council-architect in Operation SCOUT's War Council. Read:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/AGENTS.md
  ~/projects/scout/ops/OPORD.md

Update ~/projects/scout/ops/state/council-architect.json: status "engaging",
current_task "Position paper — design options", last_update now.

Your portfolio is system design. Write a position paper to
~/projects/scout/docs/adr/positions/council-architect.md covering, in order:

  1. Ranking: candidate frecency formulas — zoxide-style
     (frequency * decay), pure recency, hit-count + rank blending, etc.
     State which you lean toward and why. Show the math.
  2. Data flow: the search pipeline from keystroke to render. Where
     cancellation lives. How stale results are discarded.
  3. Action composition: how a user-configured action (e.g. "open in
     sublime then cd in terminal") is expressed and executed. Template
     variables. Chaining semantics.
  4. Module boundaries in Rust: proposed crate/module layout. Which
     types cross boundaries. Where traits earn their weight.

You may reference ~/projects/pathexplorer AS READ-ONLY
REFERENCE — do not edit it. That project is our predecessor; study
what it got right and what it got wrong (its search, index, and TUI
modules). If it is not present on this machine, omit that citation
rather than fabricate.

No code beyond illustrative pseudo-code. 800-1400 words. Terse.

Close by updating state file — status "standby", notes with key claim.
```

**Milestone:** `docs/adr/positions/council-architect.md` exists.
**Checkpoint:** It contains a specific ranking formula proposal with rationale.
**Connection:** Feeds ADR-001 and ADR-004.

> **Note on reference access:** if the pathexplorer project is not present on this machine and the Architect wants to cite it, you can either (a) copy the files you want cited into `docs/reference/pathexplorer/` (commander's discretion), or (b) have the Architect reason from what's described in `ops/CAMPAIGN.md` alone. Default: (b) unless you see value in (a).

---

### 1.3 council-quartermaster — Supply Audit

**Prompt:**

```
You are council-quartermaster in Operation SCOUT's War Council. Read:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/AGENTS.md

Update ~/projects/scout/ops/state/council-quartermaster.json: status
"engaging", current_task "Position paper — supply audit", last_update now.

Your portfolio is dependencies. Apply the six decade-longevity gates
from ops/CAMPAIGN.md to every crate we might plausibly pull in.

Write a position paper to
~/projects/scout/docs/adr/positions/council-quartermaster.md containing:

  1. A scored table. Columns: crate, version considered, age, bus
     factor, semver maturity, RustSec history, replaceability,
     stdlib-proximity, VERDICT (HOLD | VERIFY | SWAP | AVOID). Use the
     provisional roster in CAMPAIGN.md as the starting point, plus any
     additions you think should be considered (e.g., nucleo as a
     fuzzy-matcher alternative).
  2. Recommended shortlist with reasoning per pick.
  3. A separate section on crates to REJECT and why.
  4. Risks the table does not capture (abandoned maintainers, license
     concerns, native-dep hell, cross-compile issues).

Note: you may not have internet access on this machine. State
explicitly where your facts are from (training data vs. external lookup).
If a claim requires a live crates.io or GitHub check that you cannot
perform, mark it as "VERIFY" rather than fabricating specifics.

No code. 600-1200 words.

Close: update state file — status "standby", notes with shortlist summary.
```

**Milestone:** position paper exists with a scored table.
**Checkpoint:** every candidate crate has a verdict in the table.
**Connection:** ADR-002 author (Quartermaster again) draws directly from this.

---

### 1.4 council-security — Threat Terrain

**Prompt:**

```
You are council-security in Operation SCOUT's War Council. Read:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/AGENTS.md

Update ~/projects/scout/ops/state/council-security.json accordingly.

Your portfolio is the threat model. Write
~/projects/scout/docs/adr/positions/council-security.md covering:

  1. Attack surface enumeration for a tool that:
     - Reads user-authored TOML config
     - Runs user-configured shell commands as actions
     - Indexes arbitrary filesystem paths
     - Stores those paths in a local SQLite DB
     Consider: filename injection, path traversal, command injection,
     TOML parser attacks, DB-file tampering, symlink surprises.
  2. Trust boundaries: what we trust blindly, what we sanitise, what
     we refuse.
  3. Action-execution safety: how a user-configured template like
     `subl {path}` should expand. Shell vs. direct-exec trade-off.
     Quoting rules. Sandbox expectations.
  4. Config-file trust: is it safe to source a TOML from a dotfiles
     repo on a machine you've just landed on? What checks before
     trusting an action definition?
  5. What we explicitly are NOT defending against (out-of-scope threats).

Be concrete; name scenarios, not vibes. 600-1200 words. No code.

Close: state file — "standby", notes with top three threats ranked.
```

**Milestone:** position paper exists with enumerated threats and trust boundaries.
**Checkpoint:** at least one concrete shell-injection scenario discussed.
**Connection:** ADR-003 author (Security) drafts from this; ADR-004 (action schema) must respect the quoting/exec rules established here.

---

### 1.5 council-surgeon — Reliability Terrain

**Prompt:**

```
You are council-surgeon in Operation SCOUT's War Council. Read:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/AGENTS.md

Update ~/projects/scout/ops/state/council-surgeon.json.

Your portfolio is reliability, triage, crash recovery. Write
~/projects/scout/docs/adr/positions/council-surgeon.md covering:

  1. Failure modes across the lifecycle: what can fail during
     indexing, searching, action execution, configuration loading.
     What observable symptom each one produces.
  2. Partial-state handling: interrupted indexing, stale entries,
     half-written DB rows, WAL corruption, filesystem races (file
     deleted between walk and stat).
  3. Panic discipline: where should we panic vs. return error.
     Operator impact of each.
  4. Crash-recovery story: if scout is SIGINT'd mid-index, what
     happens on next launch? If the DB is corrupted, what tells the
     user and how do they recover?
  5. Observability minimum: what logging/counters are non-negotiable
     for a decade-supported tool.

Concrete scenarios, not platitudes. 600-1000 words.

Close: state file — "standby", notes with top three reliability risks.
```

**Milestone:** position paper exists with enumerated failure modes.
**Checkpoint:** crash-recovery story is specific (not "we should handle it gracefully").
**Connection:** feeds ADR-001 (ranking under partial-index conditions), ADR-002 (crate choices that aid recovery), ADR-003 (overlap with threat model).

---

## 🪖 Check-in #2 — After Wave 1, Before Wave 2

Read the five position papers end-to-end. You're looking for:
- Contradictions between officers (e.g., Architect wants nucleo, Quartermaster says swap it out).
- Gaps (something no one addressed).
- Disagreement with your own intent.

If you see any of the above, either:
- Note it in `HANDOFF.md` tagged `@commander` and direct the Wave 2 ADR authors to resolve it.
- Or redirect entirely (`redirect` interrupt).

Proceed to Wave 2 when the papers feel like a coherent council.

---

## Wave 2 — ADR Drafts (3 authors)

Three officers author ADRs. Each reads all five position papers before drafting. Each ADR uses `docs/adr/.template.md` as its shape.

### 2.1 ADR-001 Ranking Doctrine — authored by council-architect

**Prompt:**

```
You are council-architect. Phase 1 Wave 2. Read ALL files in
~/projects/scout/docs/adr/positions/ (all five papers). Then read the ADR
template at ~/projects/scout/docs/adr/.template.md.

Also read ~/projects/scout/ops/HANDOFF.md — the commander's Wave 2 directives
there are normative and constrain the decision, not optional suggestions.
If a directive cannot be honoured, argue against it in the ADR body;
do not silently ignore it.

Draft ADR-001 at ~/projects/scout/docs/adr/001-ranking-doctrine.md.

The decision must specify:
  - The exact frecency formula (numerator, denominator, decay curve).
  - How visit count is incremented (on every search result? on action
    execution only?).
  - Tie-breakers when two entries score equal.
  - How ranking degrades when the index is partial or empty.
  - Ranking interaction with fuzzy-match score from the query pipeline.

Status: Draft. Reviewers field: [council-intel, council-quartermaster,
council-security, council-surgeon].

Before writing, update your state file:
  status "engaging", current_task "Drafting ADR-001".

After writing, update:
  status "standby", notes "ADR-001 draft filed; awaits peer review".

Do not `git commit` your draft. Write the file, update your state
file, exit. The commit is the commander's act.

Terse, decisive, commander-frame. No filler.
```

### 2.2 ADR-002 Dependency Roster — authored by council-quartermaster

**Prompt:**

```
You are council-quartermaster. Phase 1 Wave 2. Read all position papers
under ~/projects/scout/docs/adr/positions/ and the template at
~/projects/scout/docs/adr/.template.md.

Also read ~/projects/scout/ops/HANDOFF.md — the commander's Wave 2 directives
there are normative and constrain the decision, not optional suggestions.
If a directive cannot be honoured, argue against it in the ADR body;
do not silently ignore it.

Draft ADR-002 at ~/projects/scout/docs/adr/002-dependency-roster.md.

The decision must list:
  - The exact set of crates SCOUT will depend on at v1.
  - Pinned major version per crate.
  - For each: a one-line decade-longevity justification.
  - Crates explicitly rejected, with reason.
  - Commitment to swap std::thread::available_parallelism for num_cpus
    unless strongly justified.

Status: Draft. Reviewers: [council-architect, council-security,
council-surgeon, council-intel].

Update state before and after as the standing order requires.

Do not `git commit` your draft. Write the file, update your state
file, exit. The commit is the commander's act.
```

### 2.3 ADR-003 Threat Model — authored by council-security

**Prompt:**

```
You are council-security. Phase 1 Wave 2. Read all position papers and
the ADR template.

Also read ~/projects/scout/ops/HANDOFF.md — the commander's Wave 2 directives
there are normative and constrain the decision, not optional suggestions.
If a directive cannot be honoured, argue against it in the ADR body;
do not silently ignore it.

Draft ADR-003 at ~/projects/scout/docs/adr/003-threat-model.md.

The decision must specify:
  - Trust boundaries: what is trusted, what is sanitised, what is refused.
  - Action-execution rule: shell vs. direct-exec, quoting/escaping
    mechanism, variable substitution model.
  - Config-file trust rules (what scout checks before honouring a
    config).
  - DB file permissions on Unix.
  - Explicit out-of-scope threats (what scout does NOT defend against).

Status: Draft. Reviewers: [council-architect, council-quartermaster,
council-surgeon, council-intel].

Update state before and after.

Do not `git commit` your draft. Write the file, update your state
file, exit. The commit is the commander's act.
```

### 2.4 ADR-004 Action & Config Schema — authored by council-architect

After ADR-001 is drafted, council-architect drafts ADR-004 in a second session.

**Prompt:**

```
You are council-architect, second sitting. Read all position papers,
your own ADR-001 draft, and ADR-003 (if drafted) at
~/projects/scout/docs/adr/003-threat-model.md.

Also read ~/projects/scout/ops/HANDOFF.md — the commander's Wave 2 directives
there are normative and constrain the decision, not optional suggestions.
If a directive cannot be honoured, argue against it in the ADR body;
do not silently ignore it.

Draft ADR-004 at ~/projects/scout/docs/adr/004-action-config-schema.md.

The decision must specify:
  - TOML shape: sections, key names, types.
  - Action templates: variable syntax (e.g. {path}, {dir}, {basename}),
    escaping rules (must be consistent with ADR-003).
  - Composition: how multiple actions chain (sequential, parallel,
    conditional? or only sequential for v1?).
  - Default action set ship in the binary vs. required from user
    config.
  - Config discovery order (XDG_CONFIG_HOME, fallback, validation).

Status: Draft. Reviewers: [council-quartermaster, council-security,
council-surgeon, council-intel].

Update state accordingly.

Do not `git commit` your draft. Write the file, update your state
file, exit. The commit is the commander's act.
```

---

## 🪖 Check-in #3 — After Wave 2, Before Wave 3

Read all four ADR drafts end-to-end. Your lens:
- Does each decision serve commander's intent?
- Are the consequences honest (not hand-waved)?
- Are two ADRs in conflict (e.g., ADR-004 schema demands something ADR-003 forbids)?

Revisions at this stage are cheap. Demand them if you see anything you'd regret in Phase 3.

---

## Wave 3 — Peer Review

Every ADR is reviewed by every Council officer who did not author it. Reviewers append to the `## Reviews` section; nothing else in the ADR is edited.

### Execution model — 5 sessions, batched per reviewer, sequential

One session per Council officer. Each session reads all four ADRs, all five position papers, `CAMPAIGN.md`, and `HANDOFF.md` (for the active review directive), then appends a review block to every ADR the officer did not author.

Run sessions **sequentially** — all five sessions write to the same four ADR files; parallel runs race the `## Reviews` section. Eyeball each session's output before launching the next.

| Session | Callsign | Authored | Eligible ADRs to review | Blocks |
|---|---|---|---|---|
| 1 | `council-intel` | none | 001, 002, 003, 004 | 4 |
| 2 | `council-surgeon` | none | 001, 002, 003, 004 | 4 |
| 3 | `council-quartermaster` | 002 | 001, 003, 004 | 3 |
| 4 | `council-security` | 003 | 001, 002, 004 | 3 |
| 5 | `council-architect` | 001, 004 | 002, 003 | 2 |

Order rationale: Intel sets the ecosystem frame → Surgeon lays reliability ground → Quartermaster and Security review with their own doctrine fresh → Architect closes having seen the other four lenses.

### Command shape

```bash
cd ~/projects/scout && claude -p --dangerously-skip-permissions "<PROMPT>"
```

Or drop into interactive Claude and paste the prompt:

```bash
cd ~/projects/scout && claude --dangerously-skip-permissions
```

### 3.1 Session 1 — council-intel (4 blocks)

**Prompt:**

```
You are council-intel. Phase 1 Wave 3 — peer review, batched.

Read, in this order:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/HANDOFF.md
  ~/projects/scout/docs/adr/positions/council-intel.md
  ~/projects/scout/docs/adr/positions/council-architect.md
  ~/projects/scout/docs/adr/positions/council-quartermaster.md
  ~/projects/scout/docs/adr/positions/council-security.md
  ~/projects/scout/docs/adr/positions/council-surgeon.md
  ~/projects/scout/docs/adr/001-ranking-doctrine.md
  ~/projects/scout/docs/adr/002-dependency-roster.md
  ~/projects/scout/docs/adr/003-threat-model.md
  ~/projects/scout/docs/adr/004-action-config-schema.md

Update ~/projects/scout/ops/state/council-intel.json: status "engaging",
current_task "Wave 3 batched review", last_update now.

Your eligible ADRs for this session: ADR-001, ADR-002, ADR-003, ADR-004.

For each eligible ADR, append a review block to its `## Reviews`
section, in this exact shape:

  > **council-intel, <today's ISO date, YYYY-MM-DD> — <blocker | non-blocking | endorsement>**
  >
  > <review body — 2 to 6 sentences>

Review guidance:

  - Blocker ONLY if the ADR contradicts commander's intent, breaks a
    downstream ADR, or endangers decade-longevity. Everything else is
    non-blocking or endorsement.
  - Cross-reference across ADRs. A review that names a conflict with
    another ADR is more valuable than one that reads an ADR in
    isolation.
  - The commander's Wave 3 directive in HANDOFF names three tidying
    items. If any falls in your portfolio lens, surface it in the
    relevant ADR's review block (flag as non-blocking).
  - Do NOT rewrite the ADR. Do NOT edit Decision, Rationale, or
    Consequences. Append to Reviews only.

When every eligible ADR has your review block filed, update your state
file: status "standby", last_update now, notes "Wave 3 batched review
filed: 4 ADRs reviewed".

Do not `git commit`. Writing the review blocks and updating your state
file is the engagement; the commit is the commander's act.

Terse, decisive, commander-frame. No filler.
```

### 3.2 Session 2 — council-surgeon (4 blocks)

**Prompt:**

```
You are council-surgeon. Phase 1 Wave 3 — peer review, batched.

Read, in this order:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/HANDOFF.md
  ~/projects/scout/docs/adr/positions/council-surgeon.md
  ~/projects/scout/docs/adr/positions/council-architect.md
  ~/projects/scout/docs/adr/positions/council-quartermaster.md
  ~/projects/scout/docs/adr/positions/council-security.md
  ~/projects/scout/docs/adr/positions/council-intel.md
  ~/projects/scout/docs/adr/001-ranking-doctrine.md
  ~/projects/scout/docs/adr/002-dependency-roster.md
  ~/projects/scout/docs/adr/003-threat-model.md
  ~/projects/scout/docs/adr/004-action-config-schema.md

Update ~/projects/scout/ops/state/council-surgeon.json: status "engaging",
current_task "Wave 3 batched review", last_update now.

Your eligible ADRs for this session: ADR-001, ADR-002, ADR-003, ADR-004.

For each eligible ADR, append a review block to its `## Reviews`
section, in this exact shape:

  > **council-surgeon, <today's ISO date, YYYY-MM-DD> — <blocker | non-blocking | endorsement>**
  >
  > <review body — 2 to 6 sentences>

Review guidance:

  - Blocker ONLY if the ADR contradicts commander's intent, breaks a
    downstream ADR, or endangers decade-longevity. Everything else is
    non-blocking or endorsement.
  - Cross-reference across ADRs. A review that names a conflict with
    another ADR is more valuable than one that reads an ADR in
    isolation.
  - The commander's Wave 3 directive in HANDOFF names three tidying
    items. If any falls in your portfolio lens, surface it in the
    relevant ADR's review block (flag as non-blocking).
  - Do NOT rewrite the ADR. Do NOT edit Decision, Rationale, or
    Consequences. Append to Reviews only.

When every eligible ADR has your review block filed, update your state
file: status "standby", last_update now, notes "Wave 3 batched review
filed: 4 ADRs reviewed".

Do not `git commit`. Writing the review blocks and updating your state
file is the engagement; the commit is the commander's act.

Terse, decisive, commander-frame. No filler.
```

### 3.3 Session 3 — council-quartermaster (3 blocks; skips ADR-002)

**Prompt:**

```
You are council-quartermaster. Phase 1 Wave 3 — peer review, batched.

Read, in this order:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/HANDOFF.md
  ~/projects/scout/docs/adr/positions/council-quartermaster.md
  ~/projects/scout/docs/adr/positions/council-architect.md
  ~/projects/scout/docs/adr/positions/council-security.md
  ~/projects/scout/docs/adr/positions/council-surgeon.md
  ~/projects/scout/docs/adr/positions/council-intel.md
  ~/projects/scout/docs/adr/001-ranking-doctrine.md
  ~/projects/scout/docs/adr/002-dependency-roster.md
  ~/projects/scout/docs/adr/003-threat-model.md
  ~/projects/scout/docs/adr/004-action-config-schema.md

Update ~/projects/scout/ops/state/council-quartermaster.json: status
"engaging", current_task "Wave 3 batched review", last_update now.

Your eligible ADRs for this session: ADR-001, ADR-003, ADR-004
(you authored ADR-002; do NOT review your own).

For each eligible ADR, append a review block to its `## Reviews`
section, in this exact shape:

  > **council-quartermaster, <today's ISO date, YYYY-MM-DD> — <blocker | non-blocking | endorsement>**
  >
  > <review body — 2 to 6 sentences>

Review guidance:

  - Blocker ONLY if the ADR contradicts commander's intent, breaks a
    downstream ADR, or endangers decade-longevity. Everything else is
    non-blocking or endorsement.
  - Cross-reference across ADRs. A review that names a conflict with
    another ADR is more valuable than one that reads an ADR in
    isolation.
  - The commander's Wave 3 directive in HANDOFF names three tidying
    items. If any falls in your portfolio lens, surface it in the
    relevant ADR's review block (flag as non-blocking).
  - Do NOT rewrite the ADR. Do NOT edit Decision, Rationale, or
    Consequences. Append to Reviews only.

When every eligible ADR has your review block filed, update your state
file: status "standby", last_update now, notes "Wave 3 batched review
filed: 3 ADRs reviewed".

Do not `git commit`. Writing the review blocks and updating your state
file is the engagement; the commit is the commander's act.

Terse, decisive, commander-frame. No filler.
```

### 3.4 Session 4 — council-security (3 blocks; skips ADR-003)

**Prompt:**

```
You are council-security. Phase 1 Wave 3 — peer review, batched.

Read, in this order:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/HANDOFF.md
  ~/projects/scout/docs/adr/positions/council-security.md
  ~/projects/scout/docs/adr/positions/council-architect.md
  ~/projects/scout/docs/adr/positions/council-quartermaster.md
  ~/projects/scout/docs/adr/positions/council-surgeon.md
  ~/projects/scout/docs/adr/positions/council-intel.md
  ~/projects/scout/docs/adr/001-ranking-doctrine.md
  ~/projects/scout/docs/adr/002-dependency-roster.md
  ~/projects/scout/docs/adr/003-threat-model.md
  ~/projects/scout/docs/adr/004-action-config-schema.md

Update ~/projects/scout/ops/state/council-security.json: status "engaging",
current_task "Wave 3 batched review", last_update now.

Your eligible ADRs for this session: ADR-001, ADR-002, ADR-004
(you authored ADR-003; do NOT review your own).

For each eligible ADR, append a review block to its `## Reviews`
section, in this exact shape:

  > **council-security, <today's ISO date, YYYY-MM-DD> — <blocker | non-blocking | endorsement>**
  >
  > <review body — 2 to 6 sentences>

Review guidance:

  - Blocker ONLY if the ADR contradicts commander's intent, breaks a
    downstream ADR, or endangers decade-longevity. Everything else is
    non-blocking or endorsement.
  - Cross-reference across ADRs. A review that names a conflict with
    another ADR is more valuable than one that reads an ADR in
    isolation.
  - The commander's Wave 3 directive in HANDOFF names three tidying
    items. If any falls in your portfolio lens, surface it in the
    relevant ADR's review block (flag as non-blocking).
  - Do NOT rewrite the ADR. Do NOT edit Decision, Rationale, or
    Consequences. Append to Reviews only.

When every eligible ADR has your review block filed, update your state
file: status "standby", last_update now, notes "Wave 3 batched review
filed: 3 ADRs reviewed".

Do not `git commit`. Writing the review blocks and updating your state
file is the engagement; the commit is the commander's act.

Terse, decisive, commander-frame. No filler.
```

### 3.5 Session 5 — council-architect (2 blocks; skips ADR-001 and ADR-004)

**Prompt:**

```
You are council-architect. Phase 1 Wave 3 — peer review, batched.

Read, in this order:

  ~/projects/scout/CLAUDE.md
  ~/projects/scout/ops/CAMPAIGN.md
  ~/projects/scout/ops/HANDOFF.md
  ~/projects/scout/docs/adr/positions/council-architect.md
  ~/projects/scout/docs/adr/positions/council-quartermaster.md
  ~/projects/scout/docs/adr/positions/council-security.md
  ~/projects/scout/docs/adr/positions/council-surgeon.md
  ~/projects/scout/docs/adr/positions/council-intel.md
  ~/projects/scout/docs/adr/001-ranking-doctrine.md
  ~/projects/scout/docs/adr/002-dependency-roster.md
  ~/projects/scout/docs/adr/003-threat-model.md
  ~/projects/scout/docs/adr/004-action-config-schema.md

Update ~/projects/scout/ops/state/council-architect.json: status "engaging",
current_task "Wave 3 batched review", last_update now.

Your eligible ADRs for this session: ADR-002, ADR-003
(you authored ADR-001 and ADR-004; do NOT review your own).

For each eligible ADR, append a review block to its `## Reviews`
section, in this exact shape:

  > **council-architect, <today's ISO date, YYYY-MM-DD> — <blocker | non-blocking | endorsement>**
  >
  > <review body — 2 to 6 sentences>

Review guidance:

  - Blocker ONLY if the ADR contradicts commander's intent, breaks a
    downstream ADR, or endangers decade-longevity. Everything else is
    non-blocking or endorsement.
  - Cross-reference across ADRs. A review that names a conflict with
    another ADR is more valuable than one that reads an ADR in
    isolation.
  - The commander's Wave 3 directive in HANDOFF names three tidying
    items. If any falls in your portfolio lens, surface it in the
    relevant ADR's review block (flag as non-blocking).
  - Do NOT rewrite the ADR. Do NOT edit Decision, Rationale, or
    Consequences. Append to Reviews only.

When every eligible ADR has your review block filed, update your state
file: status "standby", last_update now, notes "Wave 3 batched review
filed: 2 ADRs reviewed".

Do not `git commit`. Writing the review blocks and updating your state
file is the engagement; the commit is the commander's act.

Terse, decisive, commander-frame. No filler.
```

### Between sessions — what to check

After each reviewer's session, before launching the next:

| Signal | Where | Healthy shape |
|---|---|---|
| State file | `ops/state/<callsign>.json` | `status: "standby"`, notes mention N ADRs reviewed |
| Review block count | `grep -c '^> \*\*<callsign>' docs/adr/*.md` | Matches eligibility (4, 4, 3, 3, or 2) |
| Block format | Inspect appended lines | Blockquote prefix `> `, valid verdict tag, 2–6 sentences |
| Did they commit? | `git status` | Files modified, not committed (per CLAUDE.md §Forbidden) |
| Blocker flags | `grep -l blocker docs/adr/*.md` | Investigate each before the next reviewer runs |

Any malformed block, wrong path, or mystery commit → kill the session, fix the prompt, re-run. Do not let a second reviewer compound a first reviewer's error.

### After reviews: authors revise

If any reviewer marked a blocker, the author opens a new session:

```
You are <author callsign>. Read your ADR at ~/projects/scout/docs/adr/<file>.md
including the `## Reviews` section. Address every blocker. Either:
  - Revise the decision/rationale/consequences sections and note the
    revision in `## Revision history`, OR
  - Append a `### Author response` subsection to the blocking review
    arguing why the blocker is declined, and surface the disagreement
    in HANDOFF.md tagged @commander for adjudication.

Do not `git commit`. Do not mark Accepted — that is commander's signature.
```

---

## 🪖 Check-in #4 — Commander Sign-Off

You read each ADR in full with its reviews. For each:

- **If satisfied:** edit the ADR header
  ```
  - **Status:** Accepted
  - **Signed by commander:** 2026-MM-DD
  ```
  and add an entry to `## Revision history`: `YYYY-MM-DD — signed by commander`.

- **If not:** write to `ops/HANDOFF.md` tagged the author callsign with specific required changes. Author revises and re-files. Re-review if the change is material.

Commit the signed ADRs to `main` (documentation commits are allowed to `main` per OPORD §3):
```bash
cd ~/projects/scout
git add docs/adr/
git commit -m "Phase 1: War Council I ADRs signed"
```

---

## § Verification (Phase 1 checkpoint)

- [ ] Five files exist: `docs/adr/positions/council-{architect,quartermaster,security,surgeon,intel}.md`
- [ ] Four files exist: `docs/adr/{001-ranking-doctrine,002-dependency-roster,003-threat-model,004-action-config-schema}.md`
- [ ] Each ADR has `Status: Accepted` and a commander signature date.
- [ ] Each ADR has ≥2 non-blocking or endorsing reviews.
- [ ] `ops/HANDOFF.md` contains your `Phase 1 signed` closing entry.
- [ ] Council state files show `status: "standdown"` with a last-update timestamp.
- [ ] `git log --oneline` shows at least one ADR-signing commit.

When all green, return to the Chief of Staff with **"Phase 1 green"**. I will:
1. Author `ops/phase-2-db.md` — 2nd Rifles' runbook for the index/DB engagement.
2. Update `ops/OPORD.md` to Phase 2.
3. Draft the exact prompt that activates rifles-2.

---

## Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `claude` asks for login with no way to complete | Headless browser flow | Use the `ANTHROPIC_API_KEY` env var (see Preflight §1). |
| Officer writes its paper to the wrong path | Agent misread prompt | Delete the stray file, tighten the prompt, re-run. |
| Position paper cites nonexistent crates or fabricated stats | Agent hallucinated on offline gaps | Note in HANDOFF; Wave 3 reviewer will flag it; revise in author response cycle. |
| Two ADRs mutually contradict | Insufficient cross-reading | Re-run the later author with "read both ADRs; resolve the contradiction or surface it to commander." |
| Review cycle drags into infinite revisions | Deadlock between author and reviewer | Escalate to commander via HANDOFF `@commander` — human tie-breaker. |
| Agent writes to `main` branch | Standing order violated | Revert, post HANDOFF `@all` reiterating branch discipline, re-run. |

## When to call the commander

- Any officer marks a blocker and the author's response doesn't resolve it.
- Phase 1 runs past ~3 hours of wall-clock — something is off.
- Any agent touches `pathexplorer` in write mode.
- Any `Cargo.toml` edit lands before ADR-002 is signed.
