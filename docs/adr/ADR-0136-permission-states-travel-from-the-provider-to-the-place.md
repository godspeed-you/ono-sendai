# ADR-0136: A provider's refusal travels to the place view as a state, never as an absence

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §2.17, §3.6, §12, §32.2, §35.1, §35.2, §42.4, §44.8, §50 Phase S2; v0.2 §10.5, §16.1
- Decided by: agent (autonomous, `S2`)

## Context

§35.2 fixes six states a neighborhood group may be in — `available`, `empty`, `unknown`,
`permission_denied`, `unsupported`, `stale` — and requires that "these states MUST remain
distinct", with its own example:

> `files       permission denied for 14 process FDs`
>
> is preferable to
>
> `files       0`

§42.4 makes the same rule a provider conformance test. `PermissionState` and
`NeighborhoodGroup::withheld` existed (S1); nothing connected them to what a provider actually
says. The v0.2 providers already say it: spec v0.2 §16.1 puts a structured `ono.error/1` **inside
the field** when a field cannot be read, which is why `ono.process/1.user` is an error value under
`hidepid` and `ono.process-detail/1.open_files` is an error value when the descriptor table is
closed. Reading those fields with "no text ⇒ no neighbour" turned every refusal into an absence.

## Decision

**A field that holds an error is the provider refusing, and refusing is not absence.**
`ono_spatial_index::bridge` reads each relation-bearing field through one seam that keeps the two
apart: a field carrying an `ono.error/1` produces a *withheld exit* rather than a fact, and a
field that is simply null produces neither, which arrives as an empty group.

**`PermissionState::of_refusal(&ErrorValue)` is the total mapping**, in `ono-spatial-core`:

| the provider said | the place view shows |
|---|---|
| `provider.unavailable`, `provider.unsupported` | `unsupported` |
| anything of `ErrorKind::Permission` (`io.permission_denied`, `spatial.permission_denied`, `adapter.capability_denied`, …) | `permission_denied` |
| anything else | `unknown` |

It is total on purpose: §42.4 says denied information must produce `permission_denied` **or**
`unknown`, so there is no error a place view can render as nothing.

**The index carries it.** `SpatialIndex::record_withheld(id, label, state, detail)` stores the
refusal against the exit label it refers to, and `relation_summary` emits
`NeighborhoodGroup::withheld` for that exit instead of counting members. The `detail` is the
provider's own message, which is what §35.2's example puts in the place of the count. A withheld
group has `total() == None` — §2.17: a count nobody could take is not zero — and makes the whole
neighborhood `Completeness::Partial`, which `Neighborhood::new` already derived.

**The label is the exit, not the field.** `open_files` refuses the `file` exit, `user` the `user`
exit, `container` the `container` exit, `ppid` the `parent` exit, `open_files` on a socket record
the `owner` exit. Each is stated explicitly beside the field it comes from, because two relations
between the same pair of types (`process.parent_of` gives a process both a `parent` and a `child`
exit) make deriving the label from the relation alone ambiguous.

## Consequences

- §44.8's permission-honesty scenario has its data half: a process whose descriptor table this
  user may not read shows `files — permission denied …`, and a process that genuinely holds no
  files shows an empty group. They are different values, not different renderings.
- §32.2's unloaded expensive exit (`unknown`, "available on request") and a denied exit are both
  `NeighborhoodGroup::withheld` and stay distinguishable by their state, which is what §35.2
  asks for.
- A provider that starts returning an error where it used to return null changes what a place
  view says, with no change in the spatial layer. That is the intended coupling: the provider owns
  the fact, including the fact that it could not read something (§2.16).
- Refusals are per exit and per object. A provider that fails wholesale — the D-Bus bus is
  unreachable, the container runtime is not installed — refuses at the *collection* level, which
  is `look`'s and `near`'s to render and therefore S3/S4's to wire.

## Alternatives considered

- **Treating an error-valued field as null.** Rejected: it is precisely the false empty collection
  §42.4 forbids, and it is silent, so nobody would find out.
- **Deriving the exit label from the relation spec.** Rejected: `process.parent_of` gives a
  process two exits, and a refusal to read `ppid` refuses one of them.
- **Mapping unknown error codes to `permission_denied`.** Rejected: it would claim the user was
  refused when the truth is that nobody knows. `unknown` is the honest state and §42.4 allows it.
