# ADR-009 — Keybinding Dispatch

- **Status:** Accepted
- **Authored:** chief-of-staff
- **Date authored:** 2026-08-14
- **Reviewers:** none convened (see §Reviews)
- **Signed by commander:** 2026-08-14 (v2 objective 5, AAR §6)
- **Realises:** ADR-004 §6, which reserved these bindings and shipped a loader warning promising them

## Context

ADR-004 §6 defined a `keybinding` field, reserved `tab`, `alt-e`,
`alt-c`, `ctrl-o` and the chord forms `alt-<letter>` / `ctrl-<letter>`,
and had the loader emit a v1-specific warning — *"binding `alt-e`
recognised but not dispatched in v1; track ADR-NNN"*. This is that ADR.

Only `enter` dispatched. Every other action was reachable solely through
the action pane, which was acceptable when the reference config carried
three actions and is not now that it carries eleven: the whole point of
binding `alt-d` to `diff` is not having to open a pane to reach it.

## Decision

**Dispatch `alt-<letter>` and `ctrl-<letter>` chords from the picker, in
addition to `enter`.**

A key press is matched against the loaded actions before any built-in
handling. `alt-e` fires the action bound to `alt-e`; nothing else
changes.

**`tab` remains reserved and undispatched, and this is now a permanent
decision rather than a deferral.** The picker owns `tab` — it opens the
action pane (ADR-007 rev 2), which is the surface every unbound action
is reached through. An action that stole `tab` would remove the only
route to the actions that have no binding. The loader's warning for
`tab` therefore changes from "not dispatched in v1" to a statement of
why it will not be.

**Reserved `ctrl-` letters that the picker already owns are refused at
load, not silently shadowed.** `ctrl-c` quits. A config binding an
action to `ctrl-c` is a mistake the user should hear about at load,
where the message can name the conflict, rather than discovering that
one of the two behaviours silently won.

**Conflicts between two actions are refused at load.** Two actions
claiming `alt-e` is a config error with no correct resolution; picking
the first would make the second silently dead.

## Rationale

The binding grammar was already designed, warned about, and closed to
new shapes; this ADR only connects it. That is why it is small — the
expensive decisions were taken in ADR-004 §6, and the loader has been
telling users to expect this for months.

Refusing conflicts rather than resolving them follows ADR-004's posture
throughout: the schema refuses ambiguity at load (unknown placeholders,
duplicate `enter` bindings, unsupported `schema_version`) instead of
choosing on the user's behalf. A silently shadowed keybinding is the
worst outcome available — the action exists, the config looks right, and
the key does something else.

Keeping `tab` for the pane is the same reasoning as ADR-007 rev 2's
column: the route to *every* action must not be capturable by *one*
action.

Against decade-longevity: no new dependency, no schema change, no new
placeholder. `crossterm` already reports `KeyModifiers`, so this is
matching data we were already receiving and discarding.

## Alternatives considered

1. **Dispatch `tab` too, and move the pane to another key.** Rejected.
   `tab` is the discoverable key for "what else can I do here", and
   moving it to make room for a user binding trades a universal
   affordance for one config's convenience.
2. **Let the last binding win on a conflict.** Rejected — see
   §Rationale. Order-dependent behaviour in a config file is a bug
   generator.
3. **Allow arbitrary key names** (`f1`, `home`, `alt-shift-x`).
   Rejected for now: ADR-004 §6 deliberately closed the set, and each
   addition is a compatibility promise. The chord forms cover the space
   users actually asked for.
4. **Show bindings in the action pane.** Not an alternative — adopted.
   A binding nobody can see is a binding nobody uses.

## Consequences

**Binds `ui` sector.** Key matching happens before built-in handling,
and the action pane displays each action's binding alongside its name.

**Binds `config` sector.** The loader gains two refusals (picker-owned
`ctrl-` letters, duplicate bindings) and its `tab` warning is rewritten.
ADR-004 §6's promised warning text changes because the promise is kept.

**Test obligation.** The refusals are the risky part, because they are
the paths a user hits while misconfiguring: a duplicate binding and a
`ctrl-c` binding must both fail the load with a message naming the
action, and `tab` must warn without failing.

## Reviews

_Appended by peer reviewers._

None convened. Commander-directed as v2 objective 5.

## Revision history

- 2026-08-14 — drafted and signed under the v2 opening objectives; realises ADR-004 §6.
