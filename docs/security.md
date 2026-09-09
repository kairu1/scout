# Security

scout runs your own programs at your own privilege on paths it found on
your disk, from a config that travels in your dotfiles. The threats that
matter are an action you did not write arriving with the dotfiles, a
filename that becomes shell syntax, and a filename that lies on screen.
Recon adds a fourth question: who else can change the ground scout acts
on.

## The trust model

A config is hashed on a canonical projection of what it would *do*: each
action's name, keybinding, failure policy, shell attestation, steps and
`when` clause, plus the `[keys]` table. Descriptions are not hashed;
editing one does not re-prompt. The first time scout sees a config, and
every time the projection changes, it prints every action and asks for
`y`. Without a terminal it refuses to load an untrusted config rather than
trusting it silently; `scout doctor` prints the hash so you can record it
by hand for automation. Compiled-in defaults are outside the hash. The
hash format is versioned (`v2`); a change to the projection bumps it and
every user re-approves once, on purpose.

## Execution

Steps run as argv. A shell parses a scout-templated string at exactly two
seams: the `print` line the wrapper evals (every placeholder
POSIX-single-quoted; NUL and newline refused), and an explicit
`sh -c` in an action marked `unsafe_shell_template = true`. Unknown
placeholders are load-time errors. `{env.X}` resolves only against `env`
steps in the same action, never the process environment, and a missing
binding fails the step rather than expanding to nothing. Children inherit
the environment with `.` and empty entries stripped from `PATH`; secrets
are not stripped, because editors and build tools need them.

The shell wrapper evals only lines that begin `cd `, `printf `,
`${EDITOR` or `${VISUAL`, and reads them from a private temporary file
rather than a captured stdout.

tmux operations are argv too. The command for a new pane is passed as
separate arguments after `--`, so tmux runs it directly instead of through
the user's shell; an argument ending in `;` is escaped because tmux reads
a trailing `;` as a command separator; `send-keys` is never used. tmux is
detected by asking it, not by trusting the `TMUX` variable.

A session outside tmux starts one. The launcher `exec`s the tmux client
with a fixed argv whose only user-influenced elements are the session
name (validated at config load to letters, digits, `_` and `-`, so tmux
never rewrites it and no `;` or control character reaches the parser),
the working directory (from the OS, escaped, dropped if it holds a
control byte) and the wrapper's own `--print-to` path; every session
target is written `=NAME`, because tmux matches a bare name by prefix.
The private server's socket (`tmux -L scout`) lives in a directory tmux
creates 0700 and owner-checks. Options are set on scout's own server when scout creates its session
there, in memory, never by writing a file; nothing is set on the user's
default server. While the picker runs on scout's own server its focus,
zoom, focus-picker and kill-pane keys are tmux root bindings, and they
are unbound when it leaves, which also unbinds a same-key binding the
user's `~/.tmux.conf` gave that server.
On re-attach the launcher hands its `--print-to` file to the running
picker through the session environment, which is same-user state, so
the picker accepts the path only as a regular file it owns, mode 0600,
opened without following symlinks; otherwise it keeps the file it was
started with, which is opened the same way (and created if the wrapper
already removed it). Under tmux the client
always exits 0, so the wrapper's exit-code check is not a control (it
never was: the allowlist is), and the file's content is the whole
contract: an exit action writes one line, anything else writes nothing.
Scout never kills a session or a server, and never a pane on its own:
`close-pane` kills the last pane scout opened and `kill-pane` the pane
the key was pressed in (never the picker's), both at the user's
keystroke; leaving is always a detach.

## Boundaries

The config is opened without following symlinks, capped at 256 KiB, and a
parse error halts discovery rather than falling through to another file.
The index refuses paths containing NUL or newline, paths over 4 KiB, and
anything under `/proc`, `/sys` or `/dev`. The index database and the
trust store are created 0600 under 0700, owner-checked, and refused when
symlinked; the trust store is never wider than 0600 at any instant.

## Display

Every string that reaches the terminal passes one strip rule: C0 and C1
controls, DEL, bidirectional overrides and isolates, zero-width
characters and tag characters are removed, so a directory name cannot
rewrite your terminal or display in a different order from the one an
action will receive. Piped output is byte-exact; only terminal output is
stripped. Layout is measured in columns; every glyph scout emits is
declared in one module and checked against East Asian Ambiguous width.

## Recon

`scout recon` answers: how safe are the paths scout indexes, who else can
edit them, and what can be done. It runs over the rows in the index and
reports at three severities, `low`, `high`, `critical`. Credential-shaped
names are recorded on every walk whether or not hidden files are
candidates and whether or not git ignores them, so a loose `.env` is a
finding without `--hidden`; such rows never appear in the picker.

| check | finds | severity |
|---|---|---|
| `world-writable-dir` | a directory anyone can add to (`mode & 0o002`); anyone can plant a Makefile there | critical; high with the sticky bit |
| `world-writable-file` | a file anyone can rewrite | high |
| `group-writable` | group write on a file or directory | low |
| `foreign-owner` | owned by another uid: a directory or an executable | high; a plain file is low |
| `suid` | a setuid file inside a project tree | critical |
| `sgid-file` / `sgid-dir` | setgid on a file / a directory | high / low |
| `secret-exposed` | a credential-shaped name (`.env`, `.env.*`, `*.pem`, `*.key`, `*.p12`, `id_rsa` and friends, `.netrc`, `.npmrc`, `.pypirc`, `.git-credentials`, `credentials.json`, `kubeconfig`, ...) readable by others; `.ssh` not 0700 | critical when world-readable, high when group-readable |
| `recent-foreign-write` | modified in the last 24 hours by another user | high |
| `symlink-escape` | a link in an indexed directory leading into `/proc`, `/sys`, `/dev` (high) or outside your home (low) | high / low |
| `acl-present` | a POSIX ACL is set, so effective permissions may be wider than the mode shows (presence only; the ACL is not parsed) | low |
| `entrypoint-changed` | a recorded entry point (`Makefile`, `justfile`, `package.json`, `Cargo.toml`, `pyproject.toml`, `setup.py`, `go.mod`, `.envrc`, root `*.sh`, git hooks) changed since `scout recon baseline` | high |

Recon also reports on scout's own files: a config others can edit, a trust
store or index wider than 0600, a shell wrapper others can edit, and the
shell rc files (`~/.bashrc`, `~/.bash_profile`, `~/.zshrc`, `~/.profile`,
`~/.config/fish/config.fish`): one that others can edit is a finding on
its own, and the one holding the installer's marker line is named with
its line number; nothing else in it is read or reported.

Recon judges permission bits and names, never file contents. Content
scanning for `password=` is a different tool with a different
false-positive profile, and reading credential files from a project
finder would be worse than the problem it looked for. Ports, processes,
logs, audit and mandatory access control are out of scope.

Findings are stored beside the index with a fingerprint of the fact
behind each one. `scout recon accept <path> <check> --reason ...` records
an exception that hides the finding while the fact (mode, owner, link
target, hash) is unchanged and lapses when it changes. In the picker, a
row with an unaccepted finding at `high` or above shows `❢`; pressing an
action's key on it shows the finding and asks for the key again (or `a`
to accept and run). Fix actions (`chmod 600`, `chmod o-w`, `chmod u-s`,
...) ship in the reference config, gated on the matching finding, and run
only when you choose them; recon itself never changes a file.

The stat-only checks run during every walk (under twice the plain
walk's cost on 100k paths; `scout index --no-recon` opts a tree out);
ACL presence, symlink escapes and the baseline comparison run only under
`scout recon`. `scout recon --since-last` shows only findings first seen
after the previous run, at one-second granularity, and exits 1 when one
of them reaches `--fail-on`.
