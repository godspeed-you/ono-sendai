# ADR-0973: A file's owner is read when it is asked for

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §6.3, §6.4, §15.3, §15.4, §32.1, §32.2, §35.2; v0.4.1 §34.3; ADR-0149, ADR-0495; issues #206, #216
- Decided by: agent (autonomous)

## Context

A file or directory place prints `owner — available on request`, and no request filled it (#206).
The bridge reads the `owner` field of an `ono.file/1` record and asserts `user.owns_file` towards
the user that field names, and the fact stays pending until a user place with that identity is in
the index. Nothing at a file place asked the `user` target, so the fact never settled:
`follow owner`, `follow owner --resolve` and `near --type user` all ended at the same sentence.
v0.4.1 §34.3 forbids exactly that state — a relation described as available on request with no
request path.

`user.owns_file` is declared `CostClass::Expensive`. The cost belongs to the user's end of it:
every file a user owns is a walk of every filesystem. From the file's end the answer is one user
record (ADR-0149 made the same distinction for `process.opened_file`).

## Decision

**At a file or directory place the `user` target is asked when the caller's interest reaches the
owner**: `follow owner` (naming the relation is the request, ADR-0495 §2), `follow owner
--resolve`, `near --type user`, and `look --all` / `near --all`. A default `look` asks nothing
more than before and keeps printing `owner — available on request`, which is now a true
sentence.

The user records the provider answers are absorbed like any adjacent target's; the pending
`user.owns_file` fact settles on the one the file names. A refusal of the `user` target is
recorded against the `owner` exit, so the place says why there is no owner to show (§35.2).

The relation's declared cost stays `expensive`. The user's `files` exit remains unloaded: no
provider enumerates the files of a user, and this decision adds none.

## Consequences

Easy: `follow owner` moves from a file to the user that owns it, and `near --type user` lists the
owner — the `owned-by` neighbour #216 asks for.

Hard: the `user` target is asked whole — the account database — and the answer is filtered by the
pending fact; a host with a directory service behind NSS pays for that enumeration on the first
request within the cache's lifetime. An owner whose uid has no account (a file left by a deleted
user) names no user place, so the fact stays pending and the exit keeps reading `available on
request` after the request; `docs/STATE.md` carries that as found and unfiled.

Encoded by
`crates/ono-cli/tests/spatial_relationships.rs::should_lead_from_a_file_to_the_user_that_owns_it_when_the_owner_is_asked_for`,
`::should_leave_the_owner_of_a_file_on_request_when_a_default_look_reads_the_place` and acceptance case
`134-file-owner`.

## Alternatives considered

**Ask `user` on every look at a path place** — the "one line in `adjacent_targets`" the issue
mentions. Rejected: §32.1 keeps a default `look` away from a relation declared expensive, and
every `cd` in a spatial session would read the account database.

**Declare the relation `normal` from the file's end** — a relation has one cost class in
`docs/contracts/spatial/relations.yaml`; a per-end cost is a contract change larger than this
defect.

**Ask `user` with a selector on the owner's uid** — one record where the whole target is read
now. The owner field is read by the bridge, after the record is absorbed, and `observe` would
have to parse the reference a second time; the account database is small on the hosts the
native provider serves, and the selector can follow when a measurement asks for it.
