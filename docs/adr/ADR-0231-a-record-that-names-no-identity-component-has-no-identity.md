# ADR-0231: A record that names no identity component has no identity

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §2.17 (object identity), §27.3 (the schema declares the identity fields),
  §35.3 (unknown is `null`, never fabricated); v0.4 §37.1, §42.1 (one object, one place);
  ADR-0192 (a released socket is not a place)
- Decided by: agent (autonomous)

## Context

`ono.socket/1` declares `identity: [inode]`, and a socket in `time-wait` has no inode: the kernel
holds a remnant of a connection nobody owns any more. `ObjectId::of` built the identity from the
declared fields whatever they held, so **every** inodeless socket on a host reduced to the same
key — `ono.socket/1[null]` — and any two of them compared equal.

The visible consequence was in the spatial layer: two TIME_WAIT sockets were one place, which is
why `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
was green in the container, where the test's own connection is the only one closing, and red on a
developer machine, where dozens are. ADR-0192 removed that symptom by ruling that a socket in
`time-wait` or `close` is not a place at all.

The symptom is gone; the rule that produced it is not. `ObjectId` is the identity every layer
uses — `Selector::Identity` matching, `resolve`, the graph's node table, a live view's row
update, the remote client's retagging. In all of them, "these two observations are the same
object" was being answered `true` for two records that had said nothing about which object they
were. §35.3 is explicit that a null is the absence of a value rather than a value, and §2.17 makes
identity what the identity fields say; a record that supplies none of them says nothing.

## Decision

**A record whose every declared identity component is null has no identity.** `ObjectId::of` and
`ObjectRef::of` answer `None` for it, exactly as they already answer `None` for a schema that
declares no identity at all. The two cases are the same case: nothing was stated.

**One present component is enough.** `ono.route/1` identifies by `(table, family, destination,
gateway, interface)` and the default route has no destination, a directly attached route no
gateway; those routes are objects and keep their identity. The rule is *all* null, never *any*
null — a rule over "any" would delete the default route from the shell.

Providers are unaffected: `get socket` still lists a TIME_WAIT remnant with its state, because
providers own the facts (§2.16). What changes is that nothing downstream may claim two of them
are one thing.

## Consequences

- Two TIME_WAIT sockets are two records and never one object. `Selector::Identity` matches
  neither of them — which is right, because neither can be named — and `resolve` does not offer
  them as candidates, the same answer `ono-provider-netlink` already gives when asked to act on
  one ("a socket in time-wait has already been released").
- `Projection::project_as` now refuses such a record with `spatial.identity_conflict` rather than
  merging it into a phantom shared place. ADR-0192 keeps the spatial layer from reaching that
  point for a socket; the refusal is the backstop for any future schema with a fully nullable
  identity.
- The rule is stated over the record, not over any one schema, so a provider that adds a nullable
  identity field cannot reintroduce the collapse.
- Encoded by `should_refuse_to_identify_a_record_whose_every_identity_component_is_null`,
  `should_not_make_two_records_the_same_object_because_both_have_no_identity` and
  `should_keep_identifying_a_record_whose_identity_is_only_partly_null` in
  `crates/ono-provider-api/tests/contract.rs`.

## Alternatives considered

- **Synthesise an identity from the record's other fields** — the four-tuple for a socket, say.
  Rejected for the reason ADR-0192 gives: it makes a closed connection stand in the map as a live
  place, an honest-looking answer that is wrong, and it puts the spatial layer in the business of
  deciding what an object is, which §2.16 gives to the provider.
- **Rule that any null identity component removes the identity.** Simpler to state and wrong: it
  deletes the default route, a tmpfs with no UUID, and a process whose start time could not be
  read.
- **Leave it to each consumer.** Rejected: the collapse appeared in the spatial layer first only
  because that is where duplicates are visible; the identity is a v0.2 contract and belongs in one
  place.
