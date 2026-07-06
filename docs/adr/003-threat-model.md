# ADR-003 — Threat Model

- **Status:** Accepted
- **Authored:** council-security
- **Date authored:** 2026-04-24
- **Reviewers:** council-architect, council-quartermaster, council-surgeon, council-intel
- **Signed by commander:** 2026-04-24

## Context

SCOUT is an offline, single-user CLI that indexes paths, ranks them by frecency, and runs user-defined actions against them. It is intended to be carried across machines via a dotfiles repo that contains `config.toml` with `[[action]]` blocks. Two properties of the intended deployment shape every threat:

1. **The config file travels.** It lives in a synced dotfiles repo — `chezmoi`, `yadm`, `stow`, a plain git clone — and the commander's intent ("portable across my machines") makes that mobility a feature, not an anti-pattern. A new machine can inherit a malicious `[[action]]` the moment the dotfiles land.
2. **The action executor runs the user's own programs at the user's own privilege.** There is no privilege gradient to defend. The question is not "how do we contain the action?" but "how do we stop an action the user did not write from being run as if they had?".

Position papers that constrain this ADR:

- Security (`positions/council-security.md`) ranked the three realistic threats — synced-dotfiles smuggling, shell-template injection, filename-injection into render/print/SQL — and named the three mitigations: first-run trust prompt with content hash, argv-default templating with shell as a ceremonial opt-in, canonicalise-and-strip on every rendering path.
- Architect (`positions/council-architect.md` §3) committed to argv-level `execvp` templating with a closed placeholder set; shell is an explicit `["sh", "-c", …]` shape chosen by the user.
- Surgeon (`positions/council-surgeon.md` §1c) flagged that a step referencing `{env.FOO}` set by a failed earlier step must not expand to empty and run `rm ""`.
- Quartermaster (`positions/council-quartermaster.md` §4) owns supply-chain longevity and `cargo audit`; this ADR defers there on Rust-dependency threats.
- Intel (`positions/council-intel.md` §4) flagged unbounded shell templates and embedded interpreters as the class of bug this ADR exists to refuse.

Commander's Wave 2 directives (HANDOFF 2026-04-23 23:23) fix three calls and this ADR resolves all three: codify the first-run trust prompt as normative and hash on the canonicalised action set only (re-prompt on change, silent otherwise); name the print-output and `sh -c` opt-in as the *only* two shell-escape seams; declare env set by a failed step as **undefined**, not empty, and unknown placeholders as **parse errors**.

## Decision

We will enforce a three-rule threat posture for v1:

1. **Argv-only action execution** — every step expands into a list of argv tokens executed via `execvp`. Shell interpretation is available at exactly two named seams (stdout of a `print` step, and an explicit `argv = ["sh", "-c", …]` that carries a `unsafe_shell_template = true` attestation when combined with placeholders); everywhere else, a shell never sees a templated string.
2. **First-run trust prompt, hash-pinned** — on first encounter with a config, SCOUT prints every action's `name`, `description`, and fully-expanded `argv`, requires explicit `y` confirmation, and persists `SHA-256(canonicalised_action_set)` to a user-scoped trust store. Subsequent matching hashes load silently; any change re-prompts.
3. **Canonicalise, strip, refuse** — every path crossing into render, `--print`, or SQL is canonicalised to an absolute, no-`..` form; C0/C1 control bytes are stripped on render; paths containing NUL or newline, and configs that fail schema/size/symlink checks, are refused at the boundary rather than coerced.

## Rationale

### 1. Trust boundaries

What we **trust**, in the narrow sense that the code acts on the value without further challenge:

- **Indexed path as an inode reference.** Once canonicalised and written to the DB, a row's path is trusted as the argument SCOUT will hand to `execvp` and to `--print`. It is not trusted as a display string, not trusted as a shell token, not trusted as a SQL fragment.
- **`Cargo.lock` at build time.** Supply-chain trust in the Rust ecosystem is Quartermaster's gate (ADR-002); this ADR defers. Once the binary is built, the crates it was built from are trusted.
- **SQLite's own atomicity and WAL recovery.** Surgeon owns this (ADR-001 Consequences, `positions/council-surgeon.md` §4). Security does not re-audit SQLite internals.
- **The OS's privilege model.** A process running as the user may read and write the user's files; that is the operating system's contract, and SCOUT operates inside it.

What we **sanitise** — transform defensively before use:

- **Every indexed path.** Canonicalise (`std::fs::canonicalize` or equivalent) to an absolute no-`..` form before insertion into the index. Strip C0 (`0x00`–`0x1F`) and C1 (`0x80`–`0x9F`) control bytes before the string enters `ratatui` render buffers. Before a path leaves `--print` to be `eval`'d by a wrapping shell, POSIX-single-quote it (`'` → `'\''`) and frame the whole token in single quotes.
- **Every config value.** Schema-validate — unknown keys reject, wrong-typed values reject, placeholders outside the closed set reject. Size-cap at 256 KiB before handing the bytes to `toml`. Open the final path component with `O_NOFOLLOW` (or the `nix`/`rustix` equivalent) and refuse if not a regular file.
- **Every SQL parameter.** Bound via `rusqlite` named or positional parameters; no `format!`-assembled SQL anywhere in the `index` or `search` modules. This is a code-review standing rule; the Surgeon's observability counters include `db_integrity_failures_total` but there is no counter for "string SQL" because the shape does not exist in tree.
- **The inherited environment passed to spawned actions.** Strip `.` and empty entries from `PATH` before `execvp`. Do *not* strip secrets (`AWS_*`, `GITHUB_TOKEN`, `SSH_AUTH_SOCK`) — editors and build tools legitimately need them, and removal would break the canonical workflow without reducing a realistic threat, since the action runs at the user's privilege regardless. A future `[[action]] env_deny = ["AWS_*"]` knob is out of scope for v1.

What we **refuse outright** — the load fails, the action does not run, the path does not enter the index:

- Configs over 256 KiB, configs reachable only through a symlink (via `O_NOFOLLOW` refusal), configs whose schema version exceeds what the binary understands.
- Any action whose `argv` is a single string instead of a list.
- Any action whose argv or format contains an unknown placeholder (parse error — never silent empty expansion).
- Any action of shape `argv = ["sh", "-c", "... {...} ..."]` that does not also carry `unsafe_shell_template = true`.
- Any `include = "https://…"` or other remote-include form (parse error, today and forever).
- Paths over 4 KiB, paths containing NUL or newline, paths resolving inside the system denylist (`/proc`, `/sys`, `/dev`) — refused at the indexer boundary.
- A DB whose schema version disagrees with the binary's (Surgeon §4): refuse to run rather than auto-migrate downward.

The asymmetry — trust the inode, sanitise the string, refuse the wrong shape — is the load-bearing invariant of this ADR. A path is *one thing* to `execvp` and *another thing* to `ratatui`, and the two rules do not share.

### 2. Action-execution rule

**Default execution shape.** A `spawn` step executes `execvp(argv[0], argv)`. No shell is involved. The kernel does the lookup; argv elements cross into the child as a `char **`; word-splitting, globbing, command substitution, and variable expansion simply do not exist on this path.

**Variable substitution model.** The placeholder set is closed and versioned in the binary: `{path}`, `{name}`, `{parent}`, `{ext}`, `{repo_root}`, `{query}`, `{home}` (confirmed in ADR-004). Each `{var}` must fill exactly one argv slot — the substring `{foo}` in an argv element `"subl --wait {path}"` is **a parse error**, because that element would contain three shell-splittable words if it ever met a shell; we refuse at load-time rather than hope no shell ever sees it. Unknown placeholders are a parse error with file and line pointing to the offending `[[action]]`. `{env.FOO}` references to `env` values set by an earlier step in the same action evaluate against the step-local scope; a reference to an `env` name whose setter step failed (`on_failure = "continue"` let the action proceed) resolves as **undefined**, which is itself a parse-time error if statically detectable or a run-time abort otherwise. **`{env.FOO}` does not expand to the empty string, ever.** This closes the `rm ""` hazard Surgeon flagged.

**The two shell-escape seams, named.** Everywhere other than these two points, a shell does not parse a templated SCOUT string. The two seams are:

1. **`print` step output.** A `print` step writes a line to stdout that the surrounding shell wrapper is expected to `eval` (the canonical "open-and-cd" workflow). The `print` step is responsible for POSIX-single-quoting **every placeholder expansion** — `{path}`, `{parent}`, `{home}`, and equally `{name}`, `{ext}`, `{query}`, and every `{env.*}` — literally, wrap in `'…'`, replacing any inner `'` with `'\''`. A value (path or otherwise) that contains a newline or NUL is refused by the `print` step (not silently stripped) because both characters survive single-quoting and both break the `eval` contract. The contract the shell wrapper meets is: "SCOUT's stdout is a sequence of newline-terminated POSIX-safe commands; `eval` them." **Revised 2026-07-06 (see Revision history): the original wording quoted only the three path-family placeholders and let the rest "expand literally", which left `{name}`/`{ext}`/`{query}`/`{env.*}` — all of which can carry attacker-influenced shell metacharacters (a filename `` $(cmd) ``, a typed query, an env value) — unquoted on a line the wrapper evals. That is filename/query injection, the exact class §1 exists to refuse. Every placeholder is now quoted at this seam.** A value that must be interpreted *as* a command uses the `sh -c` seam with its `unsafe_shell_template` attestation, never an unquoted print placeholder.
2. **Explicit `sh -c` opt-in.** A user may write `argv = ["sh", "-c", "cd {path} && make"]`. This shape is available because sometimes a one-liner is what the workflow wants; SCOUT must not prevent it and must not pretend to sanitise into a language it is not parsing. The user owns the quoting inside the `-c` string. SCOUT's rules at this seam:
   - The shape `["sh", "-c", "…{placeholder}…"]` requires the `[[action]]` to carry `unsafe_shell_template = true`. The loader refuses the combination without that attestation. The danger is ceremonial — the user must type the word "unsafe" to buy the behaviour.
   - An `sh -c` without any placeholder is a user-authored string that SCOUT merely passes through; no attestation required.
   - A warning is emitted to the `config.reject{reason}` tracing event even on successful load of an `unsafe_shell_template` action, so the operator can audit how many actions cleared the ceremonial gate.

These two seams are the only points at which a shell parses any SCOUT-templated content. Nowhere else.

**Detached and setsid.** `wait = false` spawns get `setsid` so the child does not inherit the controlling terminal (Security position §3); this prevents the "parent shell wedged on SCOUT exit" failure Surgeon flagged. `wait = true` spawns are the default and do inherit stdin/stdout/stderr.

**Umask and cwd.** The action inherits the user's `umask`; SCOUT does not weaken it. `cwd` defaults to `$HOME`, never to SCOUT's install directory or the directory SCOUT was invoked from, to avoid accidental "the editor opened in my dotfiles repo" surprises. An `[[action]] cwd = "{path}"` override is available and explicit.

**Rate limit around execution.** Visit credit (ADR-001) is already rate-limited to one per `(path, 10s)`; execution itself is not rate-limited — a user holding Enter is a user making a deliberate choice, and throttling would conflate ergonomics with security.

### 3. Config-file trust rules

**The problem shape.** Cloning a dotfiles repo onto a fresh machine is the *default unsafe operation* in SCOUT's deployment model. The user runs `chezmoi apply`, `git clone`, or `stow scout`; SCOUT reads the config on next launch; a malicious `[[action]]` authored by an attacker with write access to the dotfiles repo (compromised GitHub token, teammate with commit rights, stolen laptop) executes on the next `Enter`.

The only defence that actually addresses this threat is a human confirmation gate on first load. This ADR makes it normative.

**First-run trust prompt (normative).**

On every config load, SCOUT computes `H = SHA-256(canonicalised_action_set)` — the input bytes are defined below. SCOUT then reads the user-scoped trust store at `$XDG_STATE_HOME/scout/trusted-config.sha256` (falling back to `~/.local/state/scout/trusted-config.sha256` when `$XDG_STATE_HOME` is unset). One of three branches fires:

1. **Trust store missing, or present but does not contain `H`** — SCOUT enters the trust prompt. It prints the absolute config path, the config's `mtime` in ISO-8601, the count of actions parsed, and for each action its `name`, `description`, and the literal argv of each step (with placeholders shown as `{path}`, unexpanded). It then prompts on a controlling TTY: `trust these N actions from <path>? [y/N]`. Anything other than `y` (case-sensitive, followed by newline) declines; SCOUT exits with a non-zero status and does not load the config. A `y` writes `H` to the trust store (creating the file at mode `0600` and its parent at `0700`) and continues.
2. **Trust store contains `H`** — SCOUT loads silently. This is the steady state.
3. **Trust store contains some `H'` for this config path but `H ≠ H'`** — the config changed. SCOUT re-enters the trust prompt, showing a diff-style view (added actions, removed actions, action names whose argv changed) before the full listing, and requires fresh `y` confirmation. The prior `H'` is replaced only on confirmation.

**Canonicalised action set — defined by ADR-004 §9.** The exact byte sequence this ADR hashes is fixed by ADR-004 §9, which is the single source of truth for the canonical-JSON projection. That projection drops cosmetic fields (including `description` and all comments), sorts actions by `name`, serialises each action's execution-relevant fields in fixed order — including `keybinding`, because a keybinding change alters which action runs on Enter and must re-prompt — preserves placeholders literally (so `{path}` hashes differently from `{parent}`), emits canonical JSON under a versioned `scout/trust-hash-v1` header, and SHA-256s the result. Hashing the raw file is deliberately refused: it would re-prompt on every whitespace or comment edit and train the user to rubber-stamp. `description`-only edits do not re-prompt. Any change that could alter what SCOUT runs — or which key dispatches it — does re-prompt.

**What we do not do.** We do not call `git` to ask whether the file is from `dotfiles@main`. We do not verify GPG signatures. We do not compare against a remote allowlist. Provenance is the user's problem; *making the change visible* is ours. Surfacing mtime and path at the prompt lets the user notice that something new appeared; enforcing a cryptographic chain of custody on the user's own dotfiles is not SCOUT's weight class.

**Non-TTY launch.** If SCOUT is invoked without a controlling TTY (cron, a shell script, CI) and a trust prompt would be required, SCOUT refuses to load the config and exits non-zero with a message naming the trust store path and the expected hash. It does not prompt silently, does not load partially, does not auto-trust. Automated use of a new config requires the user to run SCOUT interactively once on that machine.

**Config discovery and symlink policy.** ADR-004 pins the discovery order. At each candidate path, SCOUT opens the final component with `O_NOFOLLOW` — if the inode is a symlink, the open fails and SCOUT moves to the next candidate (or surfaces the error if no candidate remains). This blocks the "config symlinked to `/etc/shadow`" class and the "teammate dropped a symlink in your `~/.config`" class. It does not block an attacker who has write access to the real regular file; that is the threat the trust prompt addresses.

**Size and shape refusal.** Before `toml::from_str`, the file is read into a buffer capped at 256 KiB. A `toml` crate is robust against pathological TOML, but the cap is belt-and-suspenders: a 10 MiB crafted TOML is a denial-of-service opportunity we refuse trivially.

### 4. DB file permissions (Unix)

The SQLite index contains a record of every indexed path and the frecency history — "where have I been working" is privacy-sensitive metadata even on a single-user laptop.

**At first creation,** SCOUT creates the database at mode `0600` (owner read/write, no group, no other). The immediate parent directory (`$XDG_DATA_HOME/scout/` or `~/.local/share/scout/`) is created at mode `0700` if SCOUT creates it. SCOUT does not modify the mode of a pre-existing parent directory — that is the user's call — but it does open the DB with `O_NOFOLLOW` on the final component and refuses to proceed if the file is a symlink.

**At every open,** SCOUT `stat`s the DB path before issuing the first query. If the file's owner is not the invoking UID, SCOUT aborts with a message naming the observed owner and the expected owner. If the mode grants any group or other bit (`0o077 & mode != 0`), SCOUT emits a warning to tracing and to stderr, but continues — the user may have a deliberate reason (read-only access from a log-shipper, a shared research machine) and SCOUT is not in the business of fighting intentional configuration. The warning names the current mode and the recommended `chmod 600`.

**WAL and SHM siblings** (`-wal`, `-shm`) are created by SQLite; on Unix, SQLite inherits the DB file's mode. We do not intervene after creation. Before first open, however, SCOUT creates the WAL and SHM with the DB's `0600` umask-agnostic mode by `open`-then-`fchmod` on startup when the siblings are absent, to guarantee the initial state regardless of the user's umask.

**Windows and macOS.** Windows ACL semantics do not map cleanly to `chmod` and are out of scope for v1. On macOS, the Unix rules apply unchanged (Darwin is POSIX-mode-honouring); the `$XDG_DATA_HOME` fallback resolves to `~/Library/Application Support/scout/` via the hand-rolled XDG resolver at `src/config/paths.rs` (ADR-002 §Dirs resolution, Engineers sector). The `dirs` crate is explicitly **not** on the v1 roster — ADR-002 Decision rejects it in favour of the hand-rolled module; citations here point at the resolver, not the crate.

**No auto-repair.** If permissions drift, SCOUT warns; SCOUT does not `chmod` files it did not just create. A user who has deliberately widened permissions should not have SCOUT silently revert them.

### 5. Placeholder semantics — unknown and undefined

The commander's Wave 2 directive fixes two rules. This ADR states both as normative:

- **Unknown placeholder = parse error.** A placeholder not in the ADR-004 closed set is rejected at config load with a message pointing to the offending `[[action]]`, the offending step index, and the literal placeholder name. The action does not load partially; the step does not expand to empty. This closes the `rm {pat}` typo-for-`{path}` case where silent empty expansion would expand to `rm ""` which some tools (looking at you, GNU `rm --preserve-root=all`) would refuse, and some would not. We do not rely on the downstream tool's charity.
- **`{env.FOO}` set by a failed step = undefined, not empty.** A step may set environment for later steps (`kind = "env"`). If an earlier step failed (either aborting the action under `on_failure = "abort"`, in which case later steps do not run, or proceeding under `on_failure = "continue"`), the `env` bindings that earlier step *would have* set are **not defined for later steps**. A later step that references such a binding aborts with an `action.failed{kind="undefined_env"}` trace and a non-zero exit; it does not expand to empty, does not expand to the shell's own environment variable by the same name, does not fall through to a default.

Both rules exist to remove silent failure modes where an injection-adjacent expansion could succeed in a way the user did not intend. The rules are strict on purpose.

### 6. Supporting guarantees

The paragraphs above handle the major seams. Three smaller rules complete the model:

- **Control-byte stripping on render.** Every string that reaches `ratatui` — candidate path, action name, description, error message — passes through a strip filter that removes C0 (`0x00`–`0x1F` except `\t` which is preserved as whitespace for display) and C1 (`0x80`–`0x9F`) bytes. A directory named `\x1b]0;owned\x07` does not rewrite the terminal title. The strip is lossy and deliberate; the DB retains the canonical bytes and re-derives the display string on every render.
- **`print` refusal on hazardous paths.** A path whose canonicalised bytes contain NUL or newline is refused at the `print` step with a clear error. Both characters break the `eval` contract; stripping them would produce a different path than the one the user selected; refusing is the only safe behaviour.
- **No `PATH=.` re-entry.** Before spawning, `PATH` is processed: empty entries become `/usr/bin`-style defaults or are dropped (platform-appropriate), and `.` is dropped. This prevents the `execvp("subl", …)` call from resolving to a `./subl` dropped by an attacker into a directory the user happens to have cwd'd into. The canonical case that motivates this is a user who set `cwd = "{path}"` on an action and then navigated to a repo containing a hostile `./subl`; even then, dropping `.` from the spawn-time PATH removes the hazard.

### Gate alignment

- **Portability.** The trust store is a single file under `$XDG_STATE_HOME`; it does not commit to dotfiles (the whole point is that it is per-machine) and it does not require a daemon. A fresh machine gets a fresh prompt, which is the *correct* behaviour for a threat model that named the fresh machine as the scenario.
- **Composability.** Argv-level templating is orthogonal to step kinds (ADR-004); a new step kind does not require a new escaping rule.
- **Decade-longevity.** The trust file format is a single hex line; the hash input is canonical JSON with a version field; both survive a decade of SCOUT evolution with a one-field version bump.
- **Commander's intent.** "Fast, composable, portable across machines" — the trust prompt is the one-shot cost of "portable". We pay it deliberately at each new machine and never again until the config changes. Nothing in this ADR slows search, ranking, or action spawn below the ADR-001 p99 budgets.

## Alternatives considered

1. **Silent config trust — load and run whatever TOML is on disk.** This is the status-quo-of-most-CLI-tools design and it is what this ADR exists to refuse. It treats "the dotfiles repo is authoritative" as an axiom, when the whole threat is that *a dotfiles repo is writable by anyone with access to it*. Rejected.

2. **GPG-verified configs.** Require every config to carry a detached signature by a key the user has pre-registered with SCOUT. Rejected on three grounds: (a) it forces GPG-keyring management onto every SCOUT user, including users who do not and should not have a GPG identity; (b) it does not address the realistic threat (a stolen laptop has the signing key too); (c) it violates commander's intent — a fresh machine without the keyring cannot use SCOUT at all, which is the opposite of portable. The first-run trust prompt is weaker cryptographically but stronger operationally.

3. **Hash the whole config file.** Simpler to implement, but it re-prompts on every whitespace, comment, or `description` edit. Users trained to rubber-stamp a prompt are users who do not read the prompt. Rejected in favour of hashing the canonicalised action set.

4. **Sandbox actions via seccomp, namespaces, or AppArmor.** Rejected. The action is the user's chosen editor, compiler, or shell; sandboxing it would break the tool's utility. The action runs at the user's privilege because the user pressed Enter on it; sandboxing SCOUT itself is the wrong threat surface, and sandboxing the user's editor is unacceptable. Position paper §3 is the source.

5. **Escape-on-output for shell templates.** Some tools (Raycast, various IDEs) attempt to escape user-supplied values into a shell string before passing to `sh -c`. Rejected as unsound: escaping is a function of the target shell's grammar (bash, dash, zsh, fish all differ), the escape is fragile under composition, and the history of SQL-injection-and-its-escape cousins tells us this class of defence leaks. Argv-by-default with a *ceremonial* `unsafe_shell_template` opt-in is the only honest shape.

6. **`env` set by a failed step = empty string.** This is the shell's default (and the source of `rm ""`-class bugs). Rejected per commander directive and Surgeon §1c. Empty is a silently dangerous value; undefined is a loud one.

7. **Allowlist `PATH` entries rather than sanitising.** Require actions to name absolute paths for `argv[0]`. Rejected as unergonomic: the user's editor lives wherever `brew`/`pacman`/`nix` put it, and forcing every `[[action]]` to pin `/opt/homebrew/bin/subl` would break portability across machines. Sanitising `.` and empty entries is sufficient for the realistic threat.

8. **Mandatory re-prompt every 30 days.** Rejected as prompt-fatigue bait. The threat is "a new or changed action runs unexpectedly"; a time-based re-prompt does not address that threat and does train users to `y` without reading.

9. **Anti-virus / malware-scan the config.** Rejected as category error. We do not know what "malicious" looks like in a TOML file better than the user does; asking the user to confirm is the correct abstraction.

## Consequences

**Binds `actions` sector (Engineers).** The action loader must implement argv-level templating with a closed placeholder set (closed set defined by ADR-004). It must reject `sh -c` + placeholder combinations that lack `unsafe_shell_template = true`. It must implement `print`-step POSIX-single-quoting and NUL/newline refusal. It must implement the undefined-`env` abort rule. It must implement the first-run trust prompt, including the non-TTY refusal and the trust-store write with `0600` / `0700` modes. The diff-rendering branch of the trust prompt (modification case) is a user-facing feature to get right on first ship.

**Binds `config` sector (Engineers).** The loader must implement the `O_NOFOLLOW` open, the 256 KiB size cap, schema-version refusal, and the canonical-JSON serialisation for hashing. The canonicalised-action-set format is itself a schema change that ADR-004 must ratify — specifically, which fields count as semantic vs. cosmetic.

**Binds `index` sector (2nd Rifles).** The DB opener must implement the owner check, the mode warning, the `O_NOFOLLOW` on the DB file, and the `fchmod`-to-`0600` on initial WAL/SHM creation. SQL must be parameter-bound everywhere (no `format!`-assembled SQL); path canonicalisation happens at this boundary before insertion.

**Binds `search` sector (1st Rifles).** The `--print` path must POSIX-single-quote every token derived from a candidate path. Control-byte stripping happens at the render boundary, which is shared with the `ui` sector.

**Binds `ui` sector (3rd Rifles).** Every string rendered via `ratatui` passes through the strip filter. The trust prompt is rendered during startup, outside the main TUI alt-screen if feasible — or inside it with a pre-confirm raw-mode gate — depending on the shape ADR-004 chooses for config loading timing.

**Binds `ipc` sector (Architect's module map).** No specific binding from this ADR.

**Binds `ops` / packaging sector (Pioneers).** The install path must not drop a default `config.toml` into `$XDG_CONFIG_HOME/scout/` during install — any default config would bypass the trust prompt on first run by being pre-trusted via a packaging shortcut. An example config shipped in the repo under `examples/` is fine; copying it on install is not.

**Dependencies on other ADRs.**

- **ADR-001 (Architect, accepted).** Canonical-path discipline for visit credit is consistent with this ADR's canonicalise-at-index rule. No conflict.
- **ADR-002 (Quartermaster).** Supply-chain longevity (RustSec, `cargo audit`, MSRV) covers the class of threat this ADR explicitly defers. This ADR requires `rusqlite` with `bundled` so SQLite is compiled in (closes a surface where a system-`libsqlite3` could be attacker-controlled on a compromised machine); Quartermaster's shortlist already names bundled. No new dependency added by this ADR.
- **ADR-004 (Architect, 2nd sitting).** The closed placeholder set (`{path}`, `{name}`, `{parent}`, `{ext}`, `{repo_root}`, `{query}`, `{home}`, plus `{env.FOO}` with the failed-step rule above), the step kinds (`spawn`, `print`, `env`; no `copy` per commander directive), the `unsafe_shell_template` attribute, and the canonical-JSON schema for hashing are all defined by ADR-004. This ADR is the *enforcement* document; ADR-004 is the *shape* document. If ADR-004 changes the step set or the placeholder set, this ADR revises in lockstep.

**Explicitly out of scope (threats SCOUT does NOT defend against).**

- **A local attacker with the user's UID.** Defending against a process already running as the user is theatre; that process can read `trusted-config.sha256`, write a new `config.toml`, and re-hash. The trust prompt defends against *social* injection (a dotfiles commit), not against *a running attacker on the box*.
- **Side channels.** Timing attacks on the matcher, cache-based attacks on frecency values, shoulder-surfing the TUI. Not our weight class.
- **Supply-chain attacks on Rust crates.** `cargo audit` and the decade-longevity gates (ADR-002) own this. This ADR does not re-audit `ratatui`, `rusqlite`, or `nucleo-matcher`.
- **Network-borne attacks.** SCOUT is offline. There is no HTTP surface, no DNS lookup, no socket. A future feature that adds network is a new ADR.
- **Multi-user shared installs.** v1 is single-user. A `/etc/scout/config.toml` in the discovery chain is supported as an operator-provided default, but a multi-tenant mode where several users share a SCOUT install with per-user configs is out of scope.
- **The user's editor's bugs.** If `subl`, `vim`, `code`, or the user's `Makefile` parses a file and explodes, that is the downstream tool's problem. SCOUT's job ends at `execvp`.
- **Rendering vulnerabilities in `ratatui` or `crossterm`.** Control-byte stripping is belt; the suspenders are Quartermaster's longevity audit. We do not audit the renderer's internals.
- **Filesystem TOCTOU between index and action.** A path that was a regular file at index time and a symlink to `/etc/shadow` at action time will have its symlink followed by the user's chosen editor, not by SCOUT. SCOUT hands an absolute path to `execvp`; what the spawned process does with that path is not SCOUT's contract. Security position §1 named this explicitly as a race we will not close.
- **Wedged filesystems (FUSE hang, hung NFS).** Surgeon owns this (ADR-001 Consequences). SCOUT times out or skips; this is a reliability concern, not a threat.
- **Data loss on SIGINT or power loss.** Surgeon owns this (ADR-001 §4 + Surgeon position §4). Not a security threat — but worth naming because users will ask.
- **Cross-machine index sync.** Out of scope for v1 (ADR-001 Consequences). A future import-index feature is a new ADR with its own threat model, specifically: opening an attacker-controlled SQLite file.
- **Weakened `umask` by the user.** If the user sets `umask 000`, SCOUT still creates the DB at `0600` via explicit mode (not umask-derived), but SCOUT does not fight the user's umask on other files.
- **Audit logging of every action execution.** Surgeon's `actions_spawned_total{name}` counter is what we ship; a per-execution audit log is not a security feature for a single-user tool and can be recovered from shell history and `tracing` spans.

## Reviews

_Appended by peer reviewers._

> **council-intel, 2026-04-24 — non-blocking**
>
> The first-run trust prompt with hash-pinning on the canonicalised action set closes the exact load-bearing threat Intel named in position §2: synced dotfiles carrying a malicious `[[action]]` onto a fresh machine — a scenario no adjacent tool (broot, Raycast, fzf) addresses because none ship cross-machine state with execution semantics. Argv-default plus ceremonial `sh -c` attestation closes the Raycast-style shell-escape lineage Intel flagged in position §4; the "type the word unsafe to buy the behaviour" framing is the right shape. Three non-blocking tidies per HANDOFF 2026-04-24 12:12 and my own portfolio lens: (1) §Dependencies cites ADR-001 as "accepted" — it is Draft. (2) §3 enumerates canonical-action-set fields (`name`, `argv` or step list, `on_failure`, `wait`, `cwd`, `unsafe_shell_template`) and omits `keybinding`, which ADR-004 §9 correctly includes; this ADR already defers the hash schema to ADR-004, so replace the inline enumeration with a pointer to ADR-004 §9. (3) §4 references `dirs` for the macOS `$XDG_DATA_HOME` fallback, but ADR-002 Decision replaced `dirs` with a hand-rolled XDG resolver in `src/config/paths.rs` — update the citation so Engineers reads one answer. No blockers; posture serves commander's intent and decade-longevity.

> **council-surgeon, 2026-04-24 — non-blocking**
>
> Two tidying items per HANDOFF 2026-04-24 12:12, and a surgeon-lens readiness point. (1) §Dependencies cites ADR-001 as "accepted" — it is Draft; one-word fix. (2) §3 enumerates the canonical-action-set hash-input fields (`name`, `argv` or step list, `on_failure`, `wait`, `cwd`, `unsafe_shell_template`) and omits `keybinding`, which ADR-004 §9 correctly includes; this ADR already defers the hash schema to ADR-004, so replace the inline enumeration with a pointer to ADR-004 §9 — otherwise a reviewer comparing the two documents finds a disagreement on what re-prompts, which is the exact class of drift the trust-prompt discipline exists to avoid. Surgeon-lens endorsements: §4 DB permissions (owner-check on open, mode-warn-not-auto-repair, `fchmod` to `0600` on WAL/SHM creation when absent) does not conflict with position §4's "SQLite sidecars after `kill -9` are SQLite's own recovery — do not delete" — the ADR's touch is creation-time on absent siblings, not recovery-time on live ones. §5's undefined-env rule (binding does not land, later reference aborts with `action.failed{kind="undefined_env"}`) closes the `rm ""` hazard from position §1c by construction; this is the specific reliability invariant I pressed for and it survived the drafting intact. Non-blocking readiness point: §3's non-TTY refusal means a first `scout` invocation from cron or CI on a fresh machine fails closed with a non-zero exit — correct behaviour, but the stderr message should explicitly name the interactive-run requirement so operators do not spend a triage cycle debugging it as a crash.

> **council-quartermaster, 2026-04-24 — non-blocking**
>
> Three tidying items — items 1 and 3 per HANDOFF 2026-04-24 12:12, item 2 from the quartermaster-lens cross-ADR check. (1) §Dependencies cites ADR-001 as "accepted" — it is Draft; one-word fix, seconding Intel and Surgeon. (2) §4 "Windows and macOS" cites `dirs` for the macOS `$XDG_DATA_HOME` fallback (`~/Library/Application Support/scout/`), but ADR-002 Decision rejects `dirs` in favour of a hand-rolled XDG resolver in `src/config/paths.rs` under the Engineers sector — the citation must point at the resolver, not the crate, so Phase 3 Engineers is not chasing two answers on where `$XDG_DATA_HOME` resolution actually lives. (3) §3's inline enumeration of canonical-action-set fields omits `keybinding`, which ADR-004 §9 correctly includes; replace the enumeration with a pointer to ADR-004 §9, per HANDOFF item 3. Supply-line endorsements: the `rusqlite` with `bundled` requirement (closing the system-`libsqlite3` surface on a compromised machine) aligns with ADR-002 slot 1; §Dependencies' deferral of supply-chain advisory discipline to ADR-002 is now satisfied by ADR-002's `cargo audit` + `cargo-deny` + <120-transitive commitments. No blocker; posture serves commander's intent and decade-longevity.

> **council-architect, 2026-04-24 — non-blocking**
>
> This ADR is the enforcement document for which ADR-004 is the shape document, and the two line up on every load-bearing contract: argv-level execution with shell confined to the two named seams (§2 `print` output, §2 `sh -c` attestation) matches ADR-004 §3 and §4 exactly; the closed placeholder set named here (`{path}`, `{name}`, `{parent}`, `{ext}`, `{repo_root}`, `{query}`, `{home}`, plus `{env.FOO}`) is the eight-element set ADR-004 §4 fixes; the undefined-env rule in §5 aligns with ADR-004 §3's all-or-nothing `env`-step landing semantics, which is the non-negotiable invariant that closes Surgeon's `rm ""` hazard. Two tidying items in the Architect portfolio per HANDOFF 2026-04-24 12:12, fourthing the review chorus: (1) §Dependencies cites ADR-001 as "accepted" — it is Draft; one-word fix. (2) §3's inline canonical-action-set field enumeration (`name`, `argv` or step list, `on_failure`, `wait`, `cwd`, `unsafe_shell_template`) omits `keybinding`, which ADR-004 §9 correctly includes — this matters architecturally because a keybinding change alters which action runs on Enter, and the trust prompt must re-fire on that change; per HANDOFF item 3, replace the inline enumeration with a pointer to ADR-004 §9 so a reviewer comparing the two documents does not find a disagreement on what re-prompts. Cross-ADR tidy on §4 "Windows and macOS" citing `dirs` while ADR-002 Decision rejects `dirs` in favour of the hand-rolled resolver: already flagged by Intel, Surgeon, and Quartermaster; align on revision. Substance endorsement: the single-slot refuse-at-parse discipline ADR-004 §4 encodes is a stronger reading of this ADR's §2 seam posture than the prose here spells out — refuse the `"subl --wait {path}"` shape at load rather than hope no shell ever sees it, which is the shape that actually survives a decade. No blocker; commander's intent (portability, decade-longevity) served.

## Revision history

- 2026-04-24 — drafted by council-security.
- 2026-04-24 — signed by commander.
- 2026-04-24 — revised §3 to replace the inline hashed-bytes enumeration with a pointer to ADR-004 §9 (the inline list had omitted `keybinding`, which ADR-004 §9 correctly includes); revised §4 "Windows and macOS" to point the macOS `$XDG_DATA_HOME` citation at the hand-rolled XDG resolver at `src/config/paths.rs` rather than the `dirs` crate that ADR-002 Decision rejects. Status remains Accepted; revisions are doctrinal alignment, not decision changes. Per Wave 3 non-blocking review chorus (Intel, Surgeon, Quartermaster, Architect) and HANDOFF 2026-04-24 12:12 items 2 and 3.
- 2026-07-06 — revised §2 seam 1: the `print` step now POSIX-single-quotes **every** placeholder expansion, not only `{path}`/`{parent}`/`{home}`. A blind independent review (shadow-review, HANDOFF 2026-07-06) found that `{name}`/`{ext}`/`{query}`/`{env.*}` expanded literally onto a wrapper-`eval`'d line, so an attacker-influenced filename, query, or env value carrying shell metacharacters was a live injection vector — the class §1 exists to close. This is a threat-model tightening (strictly more quoting), commander-authorised 2026-07-06. ADR-004 §3/§4 revised in lockstep; enforcement in `src/config/template.rs`. Status remains Accepted.
