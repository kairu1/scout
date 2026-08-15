# ADR-006 — Diagnostics (`scout doctor`)

- **Status:** Accepted
- **Authored:** chief-of-staff
- **Date authored:** 2026-08-14
- **Reviewers:** none (commander-directed; see §Reviews)
- **Signed by commander:** 2026-08-14 (direct order, "i like the idea of doctor")

## Context

When scout misbehaves, the user has no way to see the state scout is
reasoning from. The failures are not mysterious in hindsight — they are
almost always one of four things:

1. The config that loaded is not the config the user edited. The
   discovery chain has three links (ADR-004 §8) and takes the first that
   opens `O_NOFOLLOW` as a regular file, so a symlinked config falls
   through silently to a different one, or to compiled-in defaults.
2. The trust hash is stale, so the config is present and not applied.
3. The index is a snapshot from before the thing being searched for
   existed, or the last run never completed and an older generation is
   still serving.
4. `$EDITOR` is unset, or an XDG variable points somewhere unexpected,
   so an action does nothing visible.

Every one of these is a fact scout already holds and never shows. The
existing tools are indirect: `scout open-db` prints three numbers about
the index and nothing about config, trust, or environment, and
`SCOUT_LOG=debug` (added 2026-08-14) explains the walk but not the
state. Diagnosing case 1 currently requires knowing the discovery chain
by heart and stat-ing three paths by hand.

The user asked for this directly. It is also the cheapest possible
answer to "how do I debug scout when it fails", because it invents no
new machinery — every fact it prints is already computed somewhere.

## Decision

**Ship `scout doctor`: a read-only subcommand that prints the state
scout resolves at startup, grouped into sections, each line marked
`ok`, `warn`, or `FAIL`.** Exit code is 0 when nothing failed and 1 when
any check failed, so it composes in a script.

Sections: **config** (each discovery-chain link and what it resolves
to, which one won, load result, action count, warnings), **trust**
(store path, whether the loaded config is trusted, changed, or new),
**index** (DB path, schema version, path count, current vs
last-complete generation, journal mode, `PRAGMA integrity_check`),
**environment** (`$HOME`, the three XDG variables and whether each is
set or defaulted, `$EDITOR`, `$VISUAL`, `$SCOUT_LOG`, `$TERM`, whether
stdout and stderr are TTYs), and **logs** (path, size, last lines).

Three constraints bind the implementation.

**`doctor` never prompts, and never modifies, migrates or repairs.** It
loads config with `interactive: false`, so an untrusted config is
reported as a finding rather than triggering a trust prompt. It opens
the index read-only, so nothing is created, migrated or rebuilt. A
diagnostic that mutates the state it is diagnosing is not a diagnostic.
It follows that `doctor` works over SSH, in CI, and inside a pipe.

*Revised 2026-08-15 — this clause read "never prompts and never
writes".* That absolute was tried in implementation, by opening the
database `immutable=1`, and it bought three regressions for one
literal-truth: `PRAGMA journal_mode` reports `delete` for a healthy WAL
database, rows committed since the last checkpoint are invisible to the
connection, and the URI form makes any path containing `%`, `?` or `#`
fail outright. The worst of those is the second — a crashed `scout
index` would be reported as near-empty *and healthy*, which is the worst
possible answer from the tool you reach for when something is wrong.
Reading a WAL database lets SQLite create transient `-shm`/`-wal`
sidecars; that is a property of reading WAL at all, not a repair. **A
diagnostic that reads the truth beats one that writes no bytes.** The
promise is narrowed to the property that actually matters, and
`tests/doctor.rs` guards the narrowed one from outside the process: a
fresh sandbox comes back with no index created, and a corrupt database
is byte-identical afterwards with no rebuilt sibling.

**`doctor` prints a named allowlist of environment variables, never the
environment.** This is the decision in this ADR that is not obvious.
Diagnostic output exists to be pasted — into a bug report, a chat, an
issue — and the whole value of the command is that the user does paste
it. ADR-003 §Spawn deliberately does *not* strip `AWS_*`, `GITHUB_TOKEN`
or `SSH_AUTH_SOCK` from the environment handed to spawned actions,
because editors and build tools need them. That reasoning covers
*passing* secrets to a child process the user already trusts; it does
not extend to *displaying* them. A `scout doctor` that dumped the
environment would turn a debugging aid into a credential-exfiltration
convenience, one paste at a time. The allowlist is closed and adding to
it is a review decision.

**`doctor` collapses `$HOME` to `~` in every path it prints,** for the
same reason: the output is meant to leave the machine, and a username is
not diagnostic information.

**`doctor` calls the real code paths.** It resolves the config through
`loader::discover` rather than re-implementing the `O_NOFOLLOW` walk.
This is what makes it trustworthy: a `doctor` that reasons about
discovery with its own copy of the rules will eventually disagree with
the loader, and it will disagree exactly when the user is relying on it
to be right. `discover` is promoted from private to `pub(crate)`-plus-
export for this reason and no other.

## Rationale

The alternative to a diagnostic subcommand is documentation, and
documentation is the wrong instrument here. Every fact in §Context is
*machine-checkable and machine-known*; asking the user to check them by
hand asks them to simulate scout's startup in their head, which is the
same task they already failed at when the thing broke.

Preferring a subcommand over more log output is deliberate. Logs are
chronological and describe *what happened*; `doctor` is a snapshot and
describes *what is*. Case 1 above — the config that loaded is not the
config the user edited — produces no log line at all, because nothing
went wrong from the loader's point of view. It is invisible to any
amount of `SCOUT_LOG`, and visible immediately to a command that prints
the chain.

Against commander's intent: this serves *portable across machines*
most directly. The trust prompt is per-machine by design (ADR-003
§Gate alignment), the XDG resolution differs per machine, and the index
is per-machine. A new machine is exactly where scout's state most often
diverges from the user's expectation, and `doctor` is the tool that
makes the divergence legible in one command.

Against decade-longevity: no new dependency, no new file format, no
schema change, no persistent state. `doctor` is a pure function of
state other components own. If a future ADR changes the discovery chain
or the trust format, `doctor` follows automatically because it calls
their code rather than mirroring it.

## Alternatives considered

1. **Extend `scout open-db` instead.** Rejected. Its name and contract
   are about one file; config, trust, and environment do not belong
   under it, and widening it would make the DB-recovery path harder to
   reason about.
2. **A `--verbose` flag on every subcommand.** Rejected. It answers "what
   is this run doing", not "what state am I in", and it would have to be
   implemented five times.
3. **Dump the full environment, since the user chose to run it.**
   Rejected — see §Decision. Consent to run a diagnostic is not consent
   to publish credentials, and the user cannot audit what they do not
   know is there.
4. **Have `doctor` fix what it finds** (create missing directories,
   re-trust a changed config). Rejected outright. A repair tool that runs
   in the same breath as a diagnosis destroys the evidence, and
   auto-trusting a changed config is precisely the action ADR-003 §3
   exists to make a human decision.
5. **Emit JSON.** Deferred, not rejected. It is the natural request from
   anyone scripting against `doctor`, and it lands with the broader
   machine-readable output question (`scout query` has the same gap).
   `serde_json` is off the ADR-002 roster until a JSON mode ships, so
   this is one decision to take once, for both commands, not twice.

## Consequences

**Binds `ops`/CLI surface.** A fourth subcommand. README documents it in
the troubleshooting section as the first thing to run.

**Binds `config` sector (Engineers).** `loader::discover` becomes
public. It is the one API widening this ADR authorises.

**Diagnostic output is a support surface.** Once users paste `doctor`
output, its shape becomes something people read at a glance, and
changing the labels silently makes old pasted output ambiguous. Treat
the section names as stable.

**Testability.** `doctor` returns a structured report and renders it
separately, so the checks are unit-testable without a terminal, a
config, or a database. The severity mapping is the part worth testing:
a missing index is a `warn` (the user simply has not indexed yet, which
is the correct state on a fresh machine), while a corrupt index is a
`FAIL`. Conflating them would make the exit code useless on day one.

**Not closed by this ADR.** `doctor` cannot detect whether the shell
wrapper is active, because the wrapper only intercepts the bare `scout`
invocation and `scout doctor` passes straight through to the binary.
Detecting it would require the wrapper to export a marker variable,
which changes a product artifact pinned by a parity test. `doctor`
therefore reports the wrapper's *installation* facts it can see and does
not claim to know whether the function is loaded.

## Reviews

_Appended by peer reviewers._

**Commander, 2026-08-14 — three decisions ratified.** The environment
allowlist, the warn/FAIL severity split, and calling `loader::discover`
rather than re-deriving discovery were each put to the commander
explicitly and green-lit. That closes the gap flagged when this ADR was
first filed: it was authored in the same sortie as its implementation
with no council convened, and the allowlist in particular carries a
security consequence that had been read by nobody.

Ratification is not the same as review. A commander's green light
settles the *decision*; it does not substitute for council-security
reading the *implementation* against ADR-003. The narrower question
remains open and is carried to the AAR: whether four variables is the
right allowlist, and whether `TERM` — the one entry that is neither a
scout setting nor an XDG path — belongs there at all.

## Revision history

- 2026-08-14 — drafted by chief-of-staff under direct commander order.
- 2026-08-14 — the three decisions in §Decision (environment allowlist, severity split, discovery via `loader::discover`) ratified by the commander. Status remains Accepted.
