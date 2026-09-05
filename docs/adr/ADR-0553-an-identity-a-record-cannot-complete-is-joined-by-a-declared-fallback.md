# ADR-0553: An identity a record cannot complete is joined by a declared fallback

- Status: accepted
- Date: 2026-09-03
- Spec refs: §10.4, §10.5, §27.3, §28.4, §28.6, §35.3, v0.4 §10.1, §42.1
- Decided by: agent (autonomous)

## Context

A schema declares one identity field list, and every record of that schema is identified by
exactly those fields. That is right for the records which carry them and silent about the ones
which do not:

- `ono.filesystem/1` identifies by `(uuid, source)`. A pseudo filesystem has no UUID, and three
  mounts on an ordinary desktop call their source `none`, so a pstore mount and a credentials
  tmpfs reduce to the one identity `{uuid: null, source: "none"}` (issue #11).
- `ono.socket/1` identifies by `inode` alone, and a `TIME_WAIT` connection has none (issue #10).

`ObjectId::of` already refuses to call two records the same object when *every* identity
component is null (ADR-0231), which is the honest answer where it applies. It does not apply
here: `source` is required and always present, so the two pseudo filesystems have a
half-populated identity that says "the same object" about two different things. The live
conformance harness has the matching hole — it exempts any record whose declared identity has a
null component from the "two objects may not share an identity" check, because until now there
was nothing better to say about them.

The spec fixes the field lists (§28.4, §28.6) and does not say what happens when a record cannot
fill them. §10.5 does say that a null is the absence of a value rather than a value, and v0.4
§42.1 requires repeated observations of one live object to resolve to one id — which an identity
that cannot distinguish two objects cannot deliver.

Changing the declared identity to `(uuid, source, device_number)` for every filesystem was the
obvious alternative and is worse: it puts a device number that a reboot or a re-plug can change
into the identity of filesystems whose UUID is stable and sufficient, so correlation across
observations would break for the records that were never broken.

## Decision

A schema MAY declare, beside `identity`, an ordered `identity_fallback` field list. The identity
of a **record** is then:

- the declared identity fields, when this record carries a non-null value for every one of them;
- the declared identity fields **followed by** the fallback fields otherwise.

The fallback joins the identity, it does not replace it: what the record does say about which
object it is stays part of the answer, so an identity is never weakened by the rule, only made
narrower.

`Schema::identity_for(record)` is the one place that decides, and `RecordValue::identity()` and
`ObjectId::of` both go through it. `Schema::identity()` keeps meaning the declared list, which is
what help, the reference pages and the conformance contract show.

`ono.filesystem/1` declares `identity_fallback: [device_number]` and gains a nullable
`device_number` field: the superblock's `major:minor` as `mountinfo(5)` prints it, including the
anonymous `0:N` that `device` deliberately drops because there is no block device behind it.

A change to `identity_fallback` is classified exactly as a change to `identity` is —
`SchemaChangeKind::IdentityChanged`, breaking — because both change which fields decide that two
observations are one object.

## Consequences

- Two pseudo filesystems are two objects. `crates/ono-provider-linux/tests/storage.rs::
  should_tell_two_pseudo_filesystems_apart_when_they_share_a_source` holds it, and
  `crates/ono-value/tests/schema_validation.rs::
  should_identify_a_record_by_its_declared_identity_alone_when_that_identity_is_complete`
  holds the other half: a record that can complete its declared identity is unaffected, so no
  existing identity moves.
- The live conformance harness now computes "is this identity complete" from
  `identity_for(record)`, so a schema with a fallback loses the null-component exemption exactly
  where the fallback fills the hole.
- `spec-check` checks that `identity_fallback` names declared fields, the conformance suite
  checks the loaded schema against the contract's fallback, and `docs/reference/schemas.md` shows
  it. A fallback in a contract that the loader ignored is therefore not possible silently.
- The identity key of a record with a fallback is longer than the key of one without. Keys are
  built by separating rendered values with `U+001F`, so a two-value key and a three-value key
  cannot collide.
- The remaining work this makes possible is issue #10: `ono.socket/1` needs
  `identity_fallback: [protocol, local, remote]`, which is now a contract line rather than a
  mechanism.

## Alternatives considered

- **Add `device_number` to the declared identity of every filesystem.** Rejected above: it
  breaks correlation for the filesystems that already worked.
- **Bump the schema to `ono.filesystem/2`.** The rule of §10.4 that a breaking change needs a new
  version is about the *declared* contract, and the declared identity does not change here — a
  reader that knows `(uuid, source)` reads exactly what it read before. A version bump would
  rename the type in every `ref<>`, every provider claim and every acceptance case to express a
  change no reader can observe.
- **Synthesise an identity where one is missing** (a hash of the whole record, a sequence
  number). Rejected: it invents identity the system does not have, which is the fabrication
  §35.3 forbids, and two snapshots would disagree about which object is which.
