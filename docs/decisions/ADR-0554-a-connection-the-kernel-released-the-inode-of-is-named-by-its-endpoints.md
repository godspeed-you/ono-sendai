# ADR-0554: A connection the kernel released the inode of is named by its endpoints

- Status: accepted
- Date: 2026-09-03
- Spec refs: §28.4, §35.3, v0.4 §42.1; ADR-0553, ADR-0231, ADR-0420
- Decided by: agent (autonomous)

## Context

`ono.socket/1` identifies a socket by `inode`, which is spec §28.4's "provider-specific socket
ref/inode". A socket in `time-wait` has none: the kernel has already released it and reports
`idiag_inode` as zero, which the provider correctly reports as null rather than as zero (§35.3).
Three of sixteen connections in one live snapshot therefore carried a wholly null identity, and
`ObjectId::of` — rightly — answers `None` for those (ADR-0231), so the shell could say nothing
about which connection each of them was.

The user-visible symptom was fixed in `8db67f2`: `trace` roots at the first record it can relate
to instead of refusing the whole answer. That is the right behaviour for a record that genuinely
is not an object; it is the wrong answer for a `time-wait` connection, which is a real thing the
kernel is still reporting and which every network stack names by the same tuple.

## Decision

`ono.socket/1` declares `identity_fallback: [protocol, local, remote]` (ADR-0553). A socket the
kernel gave an inode is identified by that inode alone, exactly as before. A socket without one is
identified by the inode *and* the tuple — `{inode: null, protocol: tcp, local: …, remote: …}` —
which is the connection the kernel is reporting and nothing else.

Two supporting changes follow from putting a composite value in an identity for the first time:

- **`identity_key` renders composite values structurally.** It used to fall back to the `Debug`
  form for anything with no canonical text. A record's `Debug` carries its provenance, whose
  `observed` timestamp differs on every read, so the same socket seen twice would have produced
  two different keys — the opposite of what v0.4 §42.1 requires. Records are now written as their
  schema id and their fields, maps as their entries sorted by key, and lists in order.
  `ObjectId`'s `Display` uses the same rendering rather than printing `?`.
- **A generic label skips a null identity value.** `label_of` fell back to the first declared
  identity field, which for a `time-wait` socket rendered `socket/null`. It now takes the first
  value of the *record's* identity that says something.

## Consequences

- A `time-wait` connection is an object: it can be traced, related, referred to and correlated,
  and two of them from one local port to two different peers are two objects.
  `crates/ono-provider-netlink/tests/socket_decoding.rs::should_identify_a_time_wait_connection_by_its_endpoints_when_it_has_no_inode`
  and `::should_give_a_time_wait_connection_the_same_identity_on_a_second_observation` hold both
  halves, and case `202` holds it at the product against a `TIME_WAIT` the case makes itself.
- A conforming socket record now always has an identity, because `protocol` is required. The two
  tests in `crates/ono-command/tests/trace.rs` that used a null-inode socket as their stand-in for
  "this record is not an object" no longer have one, so they use what `ObjectId::of` documents as
  the other and clearer case: a record whose schema declares no identity at all — a reading, a
  measurement, a projection. The behaviour under test is unchanged; only the fixture that produces
  an unidentifiable record is.
- Acting on a socket is unaffected. `ono-provider-netlink/src/act.rs` reads the first identity
  value and requires an integer inode; the declared field keeps its place at the front of the
  identity, so a `time-wait` socket still gets the refusal that says the kernel has no socket left
  to act on.
- The fixture in `trace.rs` built its endpoints as maps where the contract says
  `record<ono.endpoint/1>`. It builds records now, which is what made the socket label
  `tcp/127.0.0.1:4001` rather than a fallback.

## Alternatives considered

- **Identify by `netlink.cookie`.** The kernel's own handle for the socket, and it is already
  carried as a provider extension. Rejected: it is not stable across a reboot, it means nothing to
  anyone reading it, and an identity in a provider-namespaced extension is an identity no other
  provider of the same schema could produce.
- **Add scalar `local_address`/`local_port`/`remote_address`/`remote_port` fields to identify
  by.** Rejected: it duplicates the endpoints the schema already carries, to work around a
  limitation in how identities were rendered rather than to say anything new.
- **Leave it, and treat a `time-wait` connection as a value.** Rejected: it is an object the
  kernel is reporting, v0.5's evidence ledger will correlate events on exactly these identities,
  and a connection nobody can name is a connection nobody can explain.
