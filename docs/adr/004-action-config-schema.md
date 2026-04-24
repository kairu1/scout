# ADR-004 — Action & Config Schema

- **Status:** Accepted
- **Authored:** council-architect (2nd sitting)
- **Date authored:** 2026-04-24
- **Reviewers:** council-quartermaster, council-security, council-surgeon, council-intel
- **Signed by commander:** 2026-04-24

## Context

SCOUT ships a TOML config that is intended to live in a dotfiles repo and travel across machines. The schema this ADR fixes is the surface every downstream sector consumes: Engineers parse it, Security hashes and trust-prompts on it, Rifles dispatch actions against it, the UI renders step output. Getting the shape right now is cheap; revising it after Phase 3 costs a migration and a trust-prompt re-prompt on every field rename.

Position papers constrain the decision:

- Architect (`positions/council-architect.md` §3) committed to argv-level templating, a closed placeholder set, sequential-only composition, and an explicit-opt-in `sh -c` shape; named spawn / print / env / copy as the step kinds under consideration.
- Intel (`positions/council-intel.md` §3) pointed at broot's `[[verbs]]` TOML as the closest existing analogue; flagged unbounded shell templates, plugin interpreters, and shell-rc config surfaces as patterns to refuse.
- Quartermaster (`positions/council-quartermaster.md` §2) locked the parser set (`toml` + `serde`) and §3 rejected `arboard`, removing the clipboard surface that the `copy` step kind would have needed.
- Security (`positions/council-security.md` §3–§4) defined the first-run trust prompt over a canonicalised action set and named the two permitted shell-escape seams; made undefined `{env.FOO}` aborts normative.
- Surgeon (`positions/council-surgeon.md` §1d) flagged the typed-config failure modes this ADR closes (silent drop of unknown keys, line-47 parse errors without action context, schema-version drift).

Commander's Wave 2 directives (HANDOFF 2026-04-23 23:23) fix four calls on this ADR and this draft resolves all four: pin the discovery order at `$XDG_CONFIG_HOME/scout/config.toml → ~/.config/scout/config.toml → /etc/scout/config.toml`; stub a `keybinding` field on `[[action]]`; drop `copy` as a step kind; restrict step kinds to `spawn`, `print`, `env` with sequential-only chaining.

ADR-003 (Security) defers to this ADR for the closed placeholder set, step kind list, `unsafe_shell_template` attribute, and canonical-JSON hash-input schema. ADR-001 (Architect) depends on this ADR for the action-executor success semantics its visit-credit rule references. ADR-002 (Quartermaster) has already removed `arboard`, so no `copy` step.

## Decision

We ship a single TOML file with a fixed top-level shape (`schema_version` scalar, `[scout]` table for settings, `[[action]]` array-of-tables for user actions). Each action is a named ordered list of `spawn` / `print` / `env` steps. Argv-level templating over a closed seven-placeholder set plus `{env.NAME}` scoped-env lookup is the *only* substitution grammar; shell interpretation is confined to the two seams ADR-003 named. Sequential-only composition, `on_failure ∈ {"abort","continue"}`, no conditionals, no loops, no includes. The binary ships two compiled-in default actions (`edit`, `print-path`); a user config merges with user-wins-by-name. Config discovery walks the commander-pinned chain first-wins; a parse error halts rather than falls through. A canonical-JSON projection of the action set (definition below) is the exact byte sequence ADR-003's `SHA-256` trust hash consumes.

## Rationale

### 1. Top-level TOML shape

Exactly three top-level constructs are defined for v1. Anything else is a parse error.

```toml
schema_version = 1

[scout]
# reserved — empty in v1; placeholder for future non-action settings

[[action]]
name = "edit"
description = "Open the selection in $EDITOR"
keybinding = "enter"
on_failure = "abort"
unsafe_shell_template = false
steps = [
  { kind = "spawn", argv = ["subl", "{path}"], wait = true },
  { kind = "print", format = "cd {path}" },
]
```

**`schema_version`** — required integer. v1 accepts exactly `1`. Any other value is refused at load with a message naming the binary's supported version and the file's version. Surgeon §1d's "schema version newer than binary" drift is closed by refusal, not silent compatibility. A future schema bump is a versioned migration, not a header change.

**`[scout]`** — reserved table, empty in v1. Unknown keys inside `[scout]` are a parse error (not a warning, not a drop) under the same discipline: silent coercion is what produces year-three surprises. Reserving the table now means adding a top-level setting later does not force users to restructure.

**`[[action]]`** — the array-of-tables that holds everything SCOUT actually runs. Order in file is preserved for tie-breaking (§5) and for the action-menu display order.

The file has no `[[include]]`, no `[[profile]]`, no `[defaults]` table; the simplest shape that supports the v1 feature set is the shape we ship.

### 2. `[[action]]` fields

| Field | Type | Required | Default | Semantics |
|---|---|---|---|---|
| `name` | string | **yes** | — | Unique within the file. ASCII `[A-Za-z0-9_-]+`, length 1–64. The trust-prompt listing, the action-menu display, and the canonical-JSON hash sort all use this field; a non-ASCII or over-long name is a parse error. |
| `description` | string | no | `""` | One-line human-readable summary shown in the trust prompt and the action menu. **Not** part of the canonical-JSON hash (Security §3); a description edit does not re-prompt. |
| `keybinding` | string | no | unset | Stub for the action dispatch key (§6). v1 accepts `"enter"` only; any other value parses as a non-fatal warning and the binding is ignored for dispatch — the action remains invokable via the menu. |
| `on_failure` | string | no | `"abort"` | Chain-wide failure policy; see §5. Values: `"abort"`, `"continue"`. Any other value is a parse error. |
| `unsafe_shell_template` | bool | no | `false` | Attestation required for any step of shape `argv = ["sh", "-c", "…{placeholder}…"]`. See §4. |
| `steps` | array of step-tables | **yes** | — | ≥1 step, ≤32 steps. The 32 cap is anti-footgun; a 33-step action is an architecture smell. Step schema is §3. |

Unknown top-level keys on an `[[action]]` are a parse error, not a drop, per Surgeon §1d. The error names the file, line, action `name`, and the offending key. A top-level `argv = [...]` directly on `[[action]]` (i.e., the one-line-shortcut shape broot supports) is a parse error too — the only way to execute is via `steps`. One shape, one parser, one trust hash.

The `name` uniqueness check runs at parse time. A collision on `name` inside the same file is a hard parse error; a collision with a compiled-in default is resolved by user-wins (§7), not an error.

### 3. Step kinds — `spawn`, `print`, `env`

Each step is a table whose `kind` field selects the variant. Fields not listed for the variant are parse errors.

#### `spawn`

```toml
{ kind = "spawn", argv = ["subl", "{path}"], wait = true, cwd = "{parent}" }
```

| Field | Type | Required | Default | Semantics |
|---|---|---|---|---|
| `kind` | string | yes | — | literal `"spawn"` |
| `argv` | array of string | yes | — | ≥1 element. `argv[0]` is the program; `argv[1..]` are its arguments. Each element is a template string (§4). Single-string form (e.g. `argv = "subl {path}"`) is a parse error. |
| `wait` | bool | no | `true` | `true` blocks until the child exits; SCOUT inherits the child's stdio. `false` spawns detached, with `setsid` per ADR-003 §2, so the child does not steal the controlling terminal. |
| `cwd` | string | no | unset → `$HOME` | Working directory for the child. Template string; placeholders expand. Absolute-or-`{path}`-rooted preferred; a relative `cwd` is resolved against `$HOME`. An unexpanded-template or fs-resolution error aborts the step with `action.failed{kind="cwd"}`. |

Execution shape is exactly `execvp(argv[0], argv)`. No shell wraps this step unless the user has explicitly made `argv[0]` = `sh` (or another shell binary), which is the escape seam §4 governs.

The return-code contract: a `spawn` step is `success` iff the child exits with status `0`. Any non-zero exit, `execvp` error (`ENOENT`, `EACCES`, …), or signal termination is `failure`. The chain policy on failure is `on_failure` (§5).

#### `print`

```toml
{ kind = "print", format = "cd {path}" }
```

| Field | Type | Required | Default | Semantics |
|---|---|---|---|---|
| `kind` | string | yes | — | literal `"print"` |
| `format` | string | yes | — | Template string. `{path}`, `{parent}`, and `{home}` placeholders are POSIX-single-quoted on expansion (ADR-003 §2 seam 1); other placeholders expand literally; a path containing NUL or newline in any quoted placeholder aborts the step with `action.failed{kind="hazardous_path"}`. |

The expanded string is written to SCOUT's own stdout followed by a single `\n`. No other stdio discipline applies; the surrounding shell wrapper `eval`s SCOUT's stdout per the canonical "open-and-cd" workflow.

Success = the byte sequence reached stdout (i.e., `write` returned the full length). A short write or stdout-closed condition is `failure`.

A `print` step has no `wait` and no `cwd`; those fields on a `print` step are parse errors.

#### `env`

```toml
{ kind = "env", set = { EDITOR = "vim", SOURCE = "{path}" } }
```

| Field | Type | Required | Default | Semantics |
|---|---|---|---|---|
| `kind` | string | yes | — | literal `"env"` |
| `set` | map of string→string | yes | — | Binding names follow POSIX env-var convention (`[A-Za-z_][A-Za-z0-9_]*`, 1–64 chars); values are template strings. An invalid name or a zero-entry `set` is a parse error. |

An `env` step executes by evaluating each value template against the current action-scope env (§4) and overlaying the resulting bindings for subsequent steps in the same action. The action-scope env is seeded, at the start of the action, from a sanitised copy of SCOUT's own process env (PATH stripped of `.` and empty entries per ADR-003 §6; secrets not redacted per ADR-003 §1). Overlay is destructive: a later `env` step on the same name replaces the earlier binding for still-later steps.

Success = every template evaluated cleanly. A template that references an undefined `{env.X}` aborts the step with `action.failed{kind="undefined_env"}`; because no binding actually lands, **none of the step's would-be bindings are applied** — consistent with ADR-003 §5 ("env set by a failed step is undefined, not empty"). A subsequent step that references a would-be name aborts in turn.

An `env` step has no `argv`, no `wait`, no `cwd`, no `format`; those fields on an `env` step are parse errors.

#### Rejected step kinds (explicit)

- **`copy` — removed per commander directive.** `arboard` was dropped from the dependency roster (ADR-002 §Decision). No clipboard surface exists, and therefore no `copy` step. A future clipboard feature enters as a new ADR that reopens both the dependency and this schema.
- **`shell`, `script`, `function`, `include` — not defined in v1.** Each would either reintroduce an embedded interpreter (refused across council position papers) or convert SCOUT's action schema into a programming language. The declarative grammar defined here is the whole grammar.

### 4. Template substitution

Every string field in an action that can contain a placeholder (`argv` element, `format`, `cwd`, `env.set` value, and `description` for human display only) is a **template string**. The substitution grammar is closed, deliberately small, and identical across every template field.

#### Closed placeholder set

Exactly eight placeholders are defined. Any other `{…}` sequence at template-expansion time is a **parse error**, not a silent empty expansion (ADR-003 §5). The loader pre-parses every template at config-load time and refuses the file if any template references an unknown placeholder.

| Placeholder | Value at expansion time |
|---|---|
| `{path}` | The absolute, canonicalised path of the selected candidate (ADR-003 §1). |
| `{name}` | The final path component of `{path}`. |
| `{parent}` | The absolute path of the directory containing `{path}`. If `{path}` is `/`, `{parent}` resolves to `/` (no-op; not undefined). |
| `{ext}` | The file extension of `{path}` without the leading `.`, or the empty string if none. Only valid when the selection is a file; on a directory candidate, `{ext}` is **undefined** and aborts the step. |
| `{repo_root}` | The nearest ancestor of `{path}` containing a `.git` entry (regular file or directory, so worktree-linked repos resolve). If no such ancestor exists up to the filesystem root, `{repo_root}` is **undefined** and aborts the step. Resolution uses `std::fs::metadata`; fs errors during the walk abort with `action.failed{kind="path"}`. No other marker (`.hg`, `.jj`, `.svn`) is consulted in v1. |
| `{home}` | The value of `$HOME` at SCOUT startup. Undefined-`$HOME` at startup is a SCOUT-level hard error (caught before config load), so this placeholder never resolves as undefined at expansion time. |
| `{query}` | The literal query-buffer contents at the moment the action was dispatched. May be the empty string on zero-query dispatch; empty is defined and valid (does not abort). |
| `{env.NAME}` | The value of `NAME` in the action-scope env (§3, `env` step). Undefined → abort per ADR-003 §5. The scope seeds from SCOUT's parent env (sanitised) and accumulates via `env` steps in the same action. `NAME` must match `[A-Za-z_][A-Za-z0-9_]*`, 1–64 chars; malformed references are a parse error. |

`{repo_root}` and `{ext}` are the two placeholders that can resolve to "undefined at runtime" despite a syntactically valid template. Undefined-at-runtime aborts the step with `action.failed{kind="undefined_placeholder", which="<name>"}` — consistent with the undefined-env discipline.

#### Single-slot rule

Every placeholder fills **exactly one argv slot**. A template `"subl --wait {path}"` is a parse error — that element, after expansion, would contain a space that `execvp` does not word-split and that a downstream shell (if any) would word-split incorrectly; either way, the user's intent is better expressed as two argv elements. The loader rejects at parse time any `argv` element that contains a placeholder *and* contains non-placeholder whitespace or non-placeholder shell metacharacters (`"`, `'`, `` ` ``, `$`, `\`, `|`, `&`, `;`, `<`, `>`, `(`, `)`, `*`, `?`, `~`, `#`). The rule applies to `argv` elements only; `format`, `cwd`, and `env.set` values allow prose.

A template element that is **pure literal with no placeholders** (e.g., `argv[0] = "subl"`) is accepted regardless of contents.

#### Escaping literal braces

A literal `{` or `}` inside a template is expressed as `{{` or `}}` respectively. Outside a placeholder, `{{ }} → { }`; inside a placeholder, braces are not nestable (no `{env.{NAME}}`). This is the only escape convention the grammar supports; backslash escapes, percent-escapes, and shell-style quoting are not template syntax.

#### The two shell seams (ADR-003 §2, recited)

Every expansion above is argv-level. A shell parses a SCOUT-templated string at exactly two points:

1. **`print` step output.** The `format` template expands, `{path}` / `{parent}` / `{home}` are POSIX-single-quoted, and the result lands on stdout for the wrapper shell to `eval`. The `print` step is responsible for its own quoting; other step kinds are not.
2. **`argv = ["sh", "-c", "… {placeholder} …"]`.** The loader refuses this shape unless the `[[action]]` carries `unsafe_shell_template = true`. The attestation is ceremonial: the user types the word "unsafe" to buy the behaviour. Placeholder expansion in this shape is literal substitution — SCOUT does not try to quote into a language it is not parsing. The user owns the quoting inside the `-c` string. An `sh -c` step without any placeholder (a pure user-authored string) does not require the attestation.

Every other template field is argv-level, no shell, no quoting dance.

### 5. Composition — sequential only

A `[[action]]` executes as a single synchronous chain: step `i+1` does not begin until step `i` finishes. The chain order is the file order of the `steps` array. There are no conditional branches, no parallel spawns, no loops, no early-return markers.

**`on_failure = "abort"` (default).** On first failing step, the chain halts; no subsequent step runs. The action is reported as `action.failed{name, step_index, kind}`. The action itself does not retry. Visit credit (ADR-001 §Visit credit) is granted iff at least one step succeeded before the abort — first-success-wins — and is suppressed if the *first* step failed.

**`on_failure = "continue"`.** On a failing step, the chain continues to the next step. A failed `env` step's bindings do **not** land (ADR-003 §5); a subsequent step that references those bindings aborts that subsequent step under the step-level undefined-env rule, which under `continue` lets the chain proceed to the step after. Visit credit is granted iff any step succeeded.

The chain's exit code at the binary level is `0` if any step succeeded (mirroring the visit-credit rule) and non-zero otherwise; the non-zero value is the first failing step's exit code under `abort`, or a SCOUT-defined `2` under `continue` if every step failed.

**Parallelism, conditionals, loops — explicitly out of scope for v1.** A user who wants "run the test suite and the linter in parallel" composes outside SCOUT (a `Makefile`, a shell one-liner inside an explicit `sh -c`). Introducing a branch or parallel node inside the schema converts the action loader into a runtime; the boundary holds.

**Chaining across actions is not supported.** One `[[action]]` cannot invoke another `[[action]]` by name in v1. If a user wants "edit then cd", that is one action with two steps; it is not two actions with a dispatch relation. Reuse is by duplication (Architect §3). An `include` or `calls` relation is a future ADR.

### 6. Keybinding stub

Per commander's directive, every `[[action]]` accepts an optional `keybinding` field, but v1 dispatches only one value: `"enter"`.

**v1 semantics.**

- `keybinding = "enter"` on an action: pressing Enter in the UI with that candidate selected dispatches this action. If two actions carry `keybinding = "enter"` in the same config, the loader rejects at parse time (dispatch ambiguity).
- `keybinding` unset: the action is invokable via the action menu only, not via a key.
- `keybinding` set to any other string: the loader emits a non-fatal warning naming the action and the unknown binding, does not dispatch on that key, and otherwise loads normally. The warning is the contract — the user is not blocked, but is told their binding will not fire.

**Reserved future values (not dispatched in v1).** `"tab"`, `"alt-e"`, `"alt-c"`, `"ctrl-o"`, and the chord form `"alt-<letter>"` / `"ctrl-<letter>"` are reserved identifiers; the loader already recognises their *shape* and warns with a v1-specific message ("binding `alt-e` recognised but not dispatched in v1; track ADR-NNN"). This is distinct from a truly unknown binding, which gets a generic warning. The distinction is small but it tells the user "we know about this; we haven't shipped it yet" versus "we have no idea what you meant".

**Hash input.** `keybinding` **is** part of the canonical-JSON hash (§9). A keybinding change alters which action runs on Enter; Security §3's trust prompt must re-fire on that change.

**Dispatch priority.** If the user's config is merged with compiled defaults (§7) and the user defines `keybinding = "enter"` on some action, the user action binds Enter. The compiled default that previously held Enter is *not* removed from the action set — it is reachable via the menu under its `name` — but it loses the key.

### 7. Default action set

SCOUT ships two compiled-in default actions that apply when, and only when, the user's config does not define an action with the same `name`.

```rust
// conceptual — actual implementation is Rust-native, not TOML
[
  Action {
    name: "edit",
    description: "Open in $EDITOR (falls back to $VISUAL, then a vi-family binary on PATH)",
    keybinding: Some("enter"),
    steps: [ spawn_editor_on("{path}", wait = true) ],
    on_failure: Abort,
  },
  Action {
    name: "print-path",
    description: "Print the selection's absolute path (POSIX-quoted) to stdout",
    keybinding: None,
    steps: [ print("{path}") ],
    on_failure: Abort,
  },
]
```

**Rationale for compiled-in (not shipped-as-file).** A default `config.toml` dropped into `$XDG_CONFIG_HOME/scout/` at install time would *pre-trust itself* (no hash in the trust store → first-run prompt) but, worse, would corrupt the "nothing exists yet, configure me" signal — the user can never tell whether the file on disk is their own choice or the packager's. Compiling defaults into the binary means the defaults are as trusted as the binary itself (the user installed the binary; they consented). ADR-003 §Consequences already binds Pioneers to *not* drop a default config on install; this ADR is the reason.

**Rationale for Rust-native, not TOML-in-binary.** The `edit` default needs fallback logic (`$VISUAL` → `$EDITOR` → `vi`/`vim`/`nano` found on `PATH`) that the strict-undefined placeholder grammar correctly refuses. The default is therefore a Rust function, not a string the parser handles. User-written TOML stays strict; the "escape hatch" lives in compiled code that is trusted by virtue of being compiled.

**Merge semantics (user config present).** The effective action set is `compiled_defaults ∪ user_actions`, resolved by `name`: on collision, the user wins and replaces the compiled default entirely. A user who writes `name = "edit"` in their config replaces the compiled `edit` — no partial overlay, no field-wise merge. To retain the default behaviour and also add a custom step, the user copies the default's shape into their config (a `scout config init` scaffold is a Phase 3 Engineers deliverable, not in this ADR).

**Trust hash scope.** The canonical-JSON hash input (§9) includes only user actions, not the compiled defaults. A binary upgrade that changes the compiled `edit` default does **not** re-prompt — the user already consented to those defaults by installing the binary. A user config that *replaces* `edit` does hash that replacement and prompts on change.

**No-config state.** If no file in the discovery chain (§8) exists, SCOUT runs with the compiled defaults only and no trust prompt. The UI shows a one-line banner on first launch: `no config loaded — using built-in defaults; write $XDG_CONFIG_HOME/scout/config.toml to customise`. The banner dismisses on any keystroke; it does not re-appear until the next cold start of a SCOUT that finds no config.

### 8. Config discovery

Per commander's Wave 2 directive, discovery walks this chain in order, first-wins:

1. `$XDG_CONFIG_HOME/scout/config.toml` — consulted only if `$XDG_CONFIG_HOME` is set and non-empty.
2. `~/.config/scout/config.toml` — consulted unconditionally.
3. `/etc/scout/config.toml` — system-level fallback.

**First-wins, not merge.** If (1) exists, (2) and (3) are ignored entirely. The system-level config in (3) is the *floor* — a site admin may ship a default, but any user who drops their own `config.toml` into their XDG tree supersedes it wholly. Merge semantics across the chain would create a three-way canonical-JSON problem (whose bytes do we hash when both user and system configs exist?) and violate the "one config, one trust prompt" invariant.

**Existence test vs. load test.** A discovery-chain entry is "taken" iff the final path component opens cleanly with `O_NOFOLLOW` (per ADR-003 §3) and the resulting file is a regular file. A path that exists but is a symlink fails the `O_NOFOLLOW` open; the loader falls through to the next entry. A path that exists, opens cleanly, but fails to parse (`toml` error, schema-version mismatch, any refusal rule in this ADR) **halts the loader**; it does not fall through. Falling through on parse errors would silently run the system config instead of the user's intended config — the worst possible behaviour.

**Undefined `$HOME`.** The loader refuses at startup if `$HOME` is unset or empty; discovery entry (2) cannot be resolved without it, and every template reference to `{home}` would fail besides. SCOUT exits with a clear message naming the missing env var.

**No implicit creation.** The loader does not create `~/.config/scout/` or any directory in the chain. A user who runs `scout` on a machine with no config gets the compiled defaults and the one-line banner (§7); the first write is a deliberate user act.

### 9. Canonical-JSON hash input (for ADR-003 trust prompt)

ADR-003 §3 requires a deterministic byte sequence over the "canonicalised action set" for its `SHA-256` trust hash. This ADR fixes that sequence exactly.

**Procedure.**

1. Parse the TOML into the typed `Config`. Apply every refusal rule in this ADR first; a config that fails any refusal rule never reaches hashing.
2. Take the `actions` list. **Drop** every compiled-in default (hashing them would re-prompt on binary upgrade, which is wrong per §7). Keep only user actions (i.e., those originating from the on-disk config file).
3. **Drop** the `description` field from every action. Descriptions are cosmetic; a description edit must not re-prompt.
4. Sort actions by `name` (byte-wise lexicographic on UTF-8 bytes).
5. For each action, serialise its fields in this fixed order:
   1. `"name"`: string
   2. `"keybinding"`: string or JSON `null`
   3. `"on_failure"`: string (`"abort"` or `"continue"`)
   4. `"unsafe_shell_template"`: bool
   5. `"steps"`: JSON array of step-objects, in declared order
6. For each step, fields in this fixed order by kind:
   - **spawn:** `"kind": "spawn"`, `"argv": [string, …]`, `"wait": bool`, `"cwd": string or null`
   - **print:** `"kind": "print"`, `"format": string`
   - **env:** `"kind": "env"`, `"set": {name: string, …}` with keys sorted byte-wise lexicographic
7. Strings are preserved **literally** — placeholders are not expanded (`{path}` hashes as the seven characters `{`, `p`, `a`, `t`, `h`, `}`, as it should, because a placeholder change alters semantics).
8. Emit canonical JSON: UTF-8 encoding, no trailing whitespace, `\n` between top-level elements of the actions array, `:` and `,` without padding spaces, no Unicode escapes other than those required (`\"`, `\\`, control bytes `\u00XX`). A single `\n` terminates the output.
9. Prepend a two-line header:

   ```
   scout/trust-hash-v1
   schema_version=1
   ```

   The header pins the hash scheme itself — a future change to the canonical-JSON projection is a header bump (`trust-hash-v2`) that intentionally re-prompts every user. The `schema_version` line echoes the config's declared version so that a future v2 config hashes differently from a v1 config with identical actions.

10. `SHA-256` over the full byte sequence (header + canonical JSON). Hex-encode for storage.

**What this does not include.** `[scout]` table contents (reserved, empty in v1); TOML comments; whitespace; field ordering in the source file; `description`; compiled-in defaults; keybinding-warning text.

**What this does include.** Every bit of state that changes *what runs* or *what key dispatches it*: `name`, `keybinding`, `on_failure`, `unsafe_shell_template`, every step's kind, argv/format/set, `wait`, `cwd`, and the order of steps within an action.

**Example.** A config containing two actions `edit` and `open-term` produces:

```
scout/trust-hash-v1
schema_version=1
[{"name":"edit","keybinding":"enter","on_failure":"abort","unsafe_shell_template":false,"steps":[{"kind":"spawn","argv":["subl","{path}"],"wait":true,"cwd":null}]},
{"name":"open-term","keybinding":null,"on_failure":"abort","unsafe_shell_template":false,"steps":[{"kind":"spawn","argv":["alacritty","--working-directory","{path}"],"wait":false,"cwd":null}]}]
```

(Line break between the two action objects is deliberate — the only whitespace in the canonical form, for diff legibility when a reviewer hexdumps a trust-store mismatch.)

### 10. Validation and loader behaviour

The loader runs in a fixed order; each stage is a hard gate, no fall-through.

1. **Discover** the config path (§8).
2. **Open** with `O_NOFOLLOW` (ADR-003 §3). Refusal on symlink, missing, or non-regular.
3. **Size cap** — read into a 256 KiB-capped buffer; over-cap refuses (ADR-003 §3).
4. **Parse TOML** via the `toml` crate into a `serde`-derived `ConfigRaw`. A TOML syntax error surfaces with the file path, 1-indexed line/column, and a one-line excerpt of the offending region.
5. **Schema-version check** — `schema_version == 1` or refuse.
6. **Typed validation** — every field on every `[[action]]` and every step is checked for type, range, and the closed-enumeration constraints in §2–§4. The first error halts; the message names the action `name` (if known at the error site), the step index (if applicable), and the failing field.
7. **Template pre-parse** — every template string is scanned for well-formed placeholders. An unknown placeholder or malformed brace is a parse error at this stage (before any action ever runs), consistent with ADR-003 §5 "unknown placeholder = parse error at load, not at dispatch".
8. **Single-slot rule** — §4's whitespace/metacharacter check for `argv` elements containing placeholders.
9. **Shell-template attestation** — any step of shape `argv = ["sh", "-c", "…{…}…"]` requires `unsafe_shell_template = true` on its action. Refuse if missing.
10. **Uniqueness** — `name` is unique within the file; `keybinding = "enter"` is unique within the file.
11. **Canonical-JSON projection** for the trust hash (§9). Hash and consult `$XDG_STATE_HOME/scout/trusted-config.sha256` per ADR-003.
12. **Trust prompt** if the hash is missing or mismatched; silent load if matching (ADR-003 §3).
13. **Merge** with compiled defaults (§7): user-wins-by-`name`.
14. **Ready.** The resulting `Config` is handed to the action executor.

A failure at any stage halts the load and exits non-zero with a message that names the stage and the offending element. The UI does not enter the main alt-screen on a config-load failure; the stderr message is the user's whole experience. This is deliberate: a broken config is a "fix your file, rerun SCOUT" moment, not a "boot into a degraded UI" moment.

### Gate alignment

- **Portability.** One file, one discovery chain, one schema version. A user commits `~/.config/scout/config.toml` to their dotfiles repo and it works on every machine that honours `$XDG_CONFIG_HOME` / `~/.config/` / `/etc/`. The trust prompt pays the per-machine portability cost exactly once.
- **Composability.** Actions compose **across** the action set (different `name`s for different workflows), not **within** a single action's steps (sequential only). The composition seam is the action menu and the keybinding dispatcher, not the schema.
- **Decade-longevity.** `schema_version = 1` is the only version that parses; v2 is a migration, not a silent extension. The closed placeholder set, the closed step-kind set, and the canonical-JSON hash header together close the three drift vectors Surgeon §1d flagged.
- **Commander's intent.** "Fast, composable, portable" — the schema is the smallest TOML that expresses the v1 feature set without dragging an interpreter, a DSL, or a network call into the config surface.

## Alternatives considered

1. **broot-style `[[verbs]]` with `invocation` / `execution` strings.** Intel §3 named this as the closest existing analogue. Rejected as under-powered for our `print` + `spawn` composition: broot's `execution` is a single string that the verb layer parses, and the `print`-step-for-shell-integration pattern ADR-001 and ADR-003 both rely on does not fit into an `execution = "..."` string. The `steps` array is strictly more expressive while staying declarative.

2. **Single-string `argv` with a SCOUT-owned word-splitter.** Broot and several Raycast-alikes accept `command = "subl {path}"` and word-split internally. Rejected: the split rules are either "like POSIX" (in which case we are implementing a shell minus the dangerous bits, and every edge case is a bug) or "naïve whitespace split" (in which case a path with a space breaks the command). A list-of-strings argv is the only honest shape.

3. **Shell-first with an `argv` opt-out.** The inverse of ADR-003's posture. Rejected on the grounds ADR-003 already argued: the dotfiles-on-a-fresh-machine threat plus the history of shell-template escape-bug lineage (Bash, Raycast, various IDE task runners) make shell-default unsafe. ADR-003 closed this door; this ADR is the shape that fits through the remaining opening.

4. **JSON or JSON5 instead of TOML.** Rejected. TOML is the commander's standing order ("Config is portable TOML", `ops/CAMPAIGN.md`). JSON's lack of comments would force a `#` line or a `"_comment"` convention; JSON5's trailing-comma leniency is a drift vector. `toml` is already on the Quartermaster roster (ADR-002 slot 9) and has a decade of stability.

5. **YAML.** Rejected on the specific-vulnerability grounds anyone who has maintained a YAML-configured tool will recognise: YAML's implicit-type coercion (`no` → false, `01:02:03` → seconds, `2.0` → 2), its multiple spec versions, and the historical remote-code-execution surface in several parsers make it the wrong file format for a config that invokes actions on the user's behalf. This is a non-starter independent of the commander directive.

6. **Shipping defaults as `config.toml` in the install path.** Rejected per ADR-003 §Consequences (Pioneers binding) and §7 above. Compiled-in defaults are the only shape that preserves the "no config, no trust prompt, works out of the box" invariant without a packager bypass of the trust prompt.

7. **Merge user config with system `/etc/scout/config.toml` (union semantics, user wins).** Rejected in favour of first-wins. A merged shape would need two separate trust-prompt tracks (one per source) or a combined hash that re-prompts when the site admin updates the system config — a pattern that trains users to rubber-stamp. First-wins keeps the trust model simple and is the standard Unix discovery idiom.

8. **Allowing `[[action]]` blocks to inherit from each other via an `extends = "other-action-name"` field.** Rejected for v1. Inheritance is a composition primitive that carries its own test burden (what does `extends` do on a step list? — prepend, append, override?); defer to a future ADR after real duplication pain is observed. Reuse is by copy-paste in v1 (Architect §3).

9. **Step-level `on_failure` overrides.** Rejected for v1. Per-step failure policy composes with action-level policy in non-obvious ways (a `continue` on the action plus `abort` on a step — which wins?); the single action-level knob is sufficient for every workflow in the position papers. A step-level knob enters when a concrete use case demands it.

10. **Dynamic placeholders (`{cmd:git rev-parse --show-toplevel}`, `{shell:$(pwd)}`).** Rejected. Any dynamic placeholder is shell-as-template-syntax through the back door, and the loader's static-analysis guarantees (unknown placeholder = parse error, §4) collapse the moment the set becomes extensible at load time. If a user needs git's own repo-root resolution, they write a two-step action (one `env` step that shells out, one `spawn` step that uses `{env.REPO_ROOT}`); that is visible, attested, and bounded.

11. **Binding the `keybinding` value set in v1.** Rejected per commander's "stub a keybinding field — even if only one binding ships in v1, the schema must allow it". The schema accepts any string; the loader warns on unrecognised bindings and dispatches only `"enter"`. Wave 3 reviewers may argue for a stricter accepted set; the commander's directive is the normative source.

12. **Live-reloading config on file change.** Rejected for v1. `notify` is off the dependency roster (ADR-002 §Rejected). A config change requires a SCOUT restart, which also re-runs the trust prompt — a reasonable outcome for a tool that launches in ~100 ms cold (ADR-001 §Performance).

## Consequences

**Binds Engineers (Actions/Config sector, Phase 3).** The TOML parser, the typed validation pipeline, the template pre-parser, the canonical-JSON projector, the trust-prompt hook, and the discovery walker all live in `src/config/`. The action executor (`src/actions/`) implements the `spawn` / `print` / `env` dispatch, the action-scope env model, the `on_failure` policies, and the first-success-wins visit-credit hook (ADR-001 §Visit credit). Engineers owns `scout config init` scaffolding as a Phase 3 deliverable.

**Binds 1st Rifles (Search sector, Phase 3).** `{query}` expansion takes the raw query buffer at dispatch time — the UI hands the buffer to the action executor alongside the candidate. The `--print` flag renders a `print` step's output on stdout, with the POSIX-quoting discipline §3 specifies.

**Binds 3rd Rifles (TUI sector, Phase 3).** The action menu renders actions ordered by file appearance (compiled defaults last unless a user override hoists them). The `enter` keybinding dispatches the single action carrying `keybinding = "enter"` in the resolved (merged) action set. The trust prompt renders **outside** the main alt-screen — either before `enable_raw_mode` or inside a raw-mode gate that the TUI's panic guard (Surgeon §3) tears down on abort. Unknown-binding warnings surface on stderr at startup; they do not open a modal.

**Binds 2nd Rifles (Index sector).** None directly. The `{path}` placeholder consumes the canonicalised path the index already stores (ADR-001 §Consequences); no new column, no new migration.

**Binds Pioneers (Ops sector, Phase 4).** The install script must not drop a `config.toml` into any discovery-chain location (restated from ADR-003 §Consequences; this ADR is the reason). The compiled defaults ship inside the binary and require no installer action. `/etc/scout/config.toml` is supported as a site-operator drop-in; the installer does not create it by default.

**Dependencies on other ADRs.**

- **ADR-001 (Ranking, accepted).** The action-executor success contract defined here (first-success-wins per action) is exactly the event ADR-001 §Visit credit credits. No conflict; the two ADRs are mutually consistent.
- **ADR-002 (Dependency roster, draft).** `toml` and `serde` carry the parser; no new crate enters via this ADR. The removal of `arboard` is honoured — no `copy` step.
- **ADR-003 (Threat model, draft).** This ADR defines the closed placeholder set, the step-kind set, the `unsafe_shell_template` attribute, and the canonical-JSON hash projection that ADR-003 §3 and §5 reference by name. ADR-003 is the enforcement document; this ADR is the shape document. Lockstep revision is required if either changes.

**Explicitly out of scope.**

- **Live config reload, multi-file config, profile selection, operator-managed fragments.** A monolithic `config.toml` is v1.
- **Clipboard integration (`copy` step).** Dropped per commander directive; `arboard` is off the roster.
- **Network-borne config (`include = "https://…"`).** Refused at parse forever (ADR-003 §Refused outright).
- **Conditional or parallel step composition.** Sequential-only per commander directive.
- **Non-TTY first-run auto-trust.** Covered by ADR-003 §3 (non-TTY refuses; this ADR does not re-litigate).
- **Windows path semantics.** ADR-002 §Cross-compile defers Windows to a later ADR; this ADR's path placeholders assume POSIX rules throughout.
- **Action inheritance / includes / references.** Deferred per §Alternatives 8.
- **Step-level `on_failure`, retry, or timeout knobs.** Deferred per §Alternatives 9.
- **Dynamic placeholders.** Refused per §Alternatives 10.
- **Keybinding dispatch beyond `enter`.** Schema allows, v1 does not dispatch; named future values are reserved (§6).

## Reviews

_Appended by peer reviewers._

> **council-intel, 2026-04-24 — endorsement**
>
> Schema fills the empty quadrant Intel mapped in position §2 — broot has verbs but no cross-machine trust model; Raycast has actions but is macOS-only and closed; fzf has keybindings but no persistent config. The `[[action]]` + `steps` array is a strict improvement over broot's single-string `execution`: the list-of-argv split kills the word-splitter-or-shell dilemma Intel named in position §3, and there is no honest third option. Compiled-in defaults with user-wins-by-name merge is the right choice over shipping a default `config.toml` — a shipped file would pre-trust itself via the packager and corrupt the "no config, no prompt" signal, mirroring the fzf-config-in-shell-rc footgun Intel warned against in position §4. Rejection of dynamic placeholders (§Alternatives 10) closes the Raycast-style injection door by construction; the two-step `env` + `spawn` pattern replaces it with something attested and bounded. One non-blocking tidy per HANDOFF 2026-04-24 12:12: §Dependencies cites ADR-001 as "accepted" — it is Draft.

> **council-surgeon, 2026-04-24 — endorsement**
>
> Tidying per HANDOFF 2026-04-24 12:12 item 1: §Dependencies cites ADR-001 as "accepted" — it is Draft. Substance endorsed end-to-end. §10's staged loader closes every failure mode position §1d named: file-with-line-and-column parse errors at §10.4 (no more "line 47" without context), `schema_version` refusal at §10.5 (no silent forward-compat drift), unknown-key parse error at §2 (no silent field drop), and first-wins discovery at §8 (no system-vs-user race). §3's `env`-step contract — an undefined `{env.X}` aborts with `action.failed{kind="undefined_env"}` and *none* of the step's would-be bindings land, not partial, not all-but-one — is the clean invariant ADR-003 §5 requires, and it pairs correctly with §5's first-success-wins visit-credit rule to close the ambiguity ADR-001 still carries in its own wording. §3's `print`-step success contract (short write or stdout-closed = failure) correctly propagates to visit credit when the wrapper shell dies mid-eval, preserving the "no credit for workflows that did not reach the user's shell" invariant. §2's 32-step cap is the right anti-footgun — a 33-step action is a shape problem, not a schema gap. No blockers; decade-longevity and commander's intent served.

> **council-quartermaster, 2026-04-24 — endorsement**
>
> Tidying per HANDOFF 2026-04-24 12:12 item 1: §Dependencies cites ADR-001 as "accepted" — it is Draft; one-word fix, seconding Intel and Surgeon. Supply-line substance endorsed: the schema parses with `toml` (ADR-002 slot 9) and `serde` (slot 7) and adds no new direct crate — the `copy` step's removal honours the `arboard` drop recorded in ADR-002 Rejected, and §Alternatives 12's rejection of live reload honours the `notify` AVOID. `{repo_root}` resolution via `std::fs::metadata` walking ancestors (§4) is the right call — a `git2`-backed alternative would drag a C library with a churn history through gate C for a feature the stdlib covers in ~30 lines. §Alternatives 10's refusal of dynamic placeholders (`{cmd:…}`, `{shell:…}`) closes the single back door through which an interpreter or shell-as-template-syntax could re-enter the roster after ADR-002 refused them explicitly — the two-step `env` + `spawn` replacement is visible, attested, and bounded. The canonical-JSON projection in §9 with its `scout/trust-hash-v1` header gives ADR-003's trust prompt a stable byte sequence without conscripting any hashing crate beyond what the binary already carries (stdlib + `serde_json` slot 8). No blocker; commander's intent and decade-longevity served.

> **council-security, 2026-04-24 — endorsement**
>
> Tidying per HANDOFF 2026-04-24 12:12 item 1: §Dependencies cites ADR-001 as "accepted" — it is Draft; one-word fix, fourthing the review chorus. Every security-load-bearing contract ADR-003 relies on is realised here as the enforcement shape: §4's closed eight-placeholder set exactly matches ADR-003 §5's "unknown placeholder = parse error"; §4's single-slot rule with the explicit metacharacter reject list (`"`, `'`, `` ` ``, `$`, `\`, `|`, `&`, `;`, `<`, `>`, `(`, `)`, `*`, `?`, `~`, `#`) is a stronger reading of ADR-003 §2's seam discipline than ADR-003 spelled out, and I endorse it — it refuses the `"subl --wait {path}"` shape at parse time rather than hoping no shell ever sees it. §3's `env`-step contract — that an undefined `{env.X}` aborts the step *and none of the step's would-be bindings land* — is the clean invariant ADR-003 §5 requires; partial-overlay semantics would have smuggled a silent-failure mode back in, and the all-or-nothing rule closes it. §3's `print`-step POSIX-single-quoting of `{path}`/`{parent}`/`{home}` plus NUL/newline refusal is the exact shape ADR-003 §2 seam 1 specifies; §10 step 9's `unsafe_shell_template` enforcement is ADR-003 §2 seam 2. §7's compiled-in-defaults-with-user-wins-by-name resolves the "pre-trust via packager bypass" hazard ADR-003 §Consequences (Pioneers binding) named — binary-trust flows from install consent, user actions flow through the trust prompt, no TOML file is silently authoritative. §9's canonical-JSON hash including `keybinding` (and excluding `description`) is the correct semantic cut — a keybinding change alters which action runs on Enter and must re-prompt, which ADR-003 §3's inline field list missed and ADR-004 §9 corrects (the HANDOFF item 3 fix lands on my revision of ADR-003). §Alternatives 10's refusal of dynamic placeholders closes the shell-as-template-syntax back door ADR-003 §Alternatives 5 argued against. No blocker; commander's intent (portability, decade-longevity) served.

## Revision history

- 2026-04-24 — drafted by council-architect (2nd sitting).
- 2026-04-24 — signed by commander.
