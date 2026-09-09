# ADR-0717: A netlink event is dated when the kernel spoke, not when the table was re-read

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.3, §21.1, §21.3, §21.5, §22.4, §35.3; v0.2 §18.2, §35.6; ADR-0235, ADR-0710,
  ADR-0711
- Decided by: agent (autonomous)

## Context

`watch interface` and `watch route` subscribe to the rtnetlink multicast groups, and ADR-0235
fixed how they answer: the notification is a wake-up, and the answer is a fresh dump decoded by
the same pure functions a `get` uses, so one code path describes an interface however the question
was asked. `NetlinkSocket::drain` therefore reads the kernel's `RTM_NEW*`/`RTM_DEL*` messages and
discards them.

v0.5 §3.3 makes that discard visible. An event's time is when the change happened as far as the
source can say, and every event these providers emitted was dated by the *re-dump* — an instant
that includes a thread wake-up, a `spawn_blocking` hop, one or two round trips to the kernel and
the decoding of the whole table. That is a fact about this provider, not about the interface.

The brief asks whether to decode the kernel's own messages for a truer source time.

## Decision

**No: the kernel messages stay discarded, and the event is dated by the instant the notification
arrived.** `wait_for_changes` takes `jiff::Timestamp::now()` the moment the socket becomes
readable — before draining, before the re-dump, before any decoding — and carries it to every
event that re-dump produces, through `ObjectEvent::with_observed_at`.

Three facts decide it.

**An rtnetlink notification carries no timestamp.** There is no source instant inside the message
to recover. Decoding `RTM_NEWADDR` would yield the same arrival instant as not decoding it, so the
decoding buys nothing on the axis §3.3 is about.

**An interface record is composed from two dumps.** `ono.interface/1` merges `RTM_GETLINK` and
`RTM_GETADDR`; a lone `RTM_NEWADDR` names an address and an interface index and nothing else.
Building a record from it would report every other field as null — not because the kernel does not
know them, but because that message was not about them, which is precisely the confusion between
absence and ignorance §35.3 exists to prevent.

**The decoders are pure and fuzzed, and they decode dumps.** Reusing them per-notification means
either accepting the partial records above or writing a second family of decoders for the
notification shapes, which doubles the surface `fuzz/` has to cover for a benefit measured in
scheduler latency.

What is left after those three is the arrival instant, and taking it is three lines of plumbing
through the channel that already exists between the polling thread and the stream.

**`SO_TIMESTAMPNS` was considered and not taken.** It would replace the residual error — the
scheduler latency between the kernel queuing the message and `poll` returning — with the kernel's
own receive stamp. It changes the socket's read path, which `fuzz/` and
`crates/ono-provider-netlink/tests/malformed_messages.rs` cover, in exchange for sub-millisecond
accuracy on an event whose consumer is a human watching a terminal. It is recorded here as the
next step if a caller ever needs it, not as work v0.5 requires.

**§22.4's last line is a claim, not a comment.** "Socket connection history is not automatically
exhaustive merely because netlink is used elsewhere." `SocketProvider::temporal` therefore
advertises `live_events: false` and `exhaustive_events: false`, and
`crates/ono-provider-netlink/tests/kernel_providers.rs::should_not_claim_that_socket_history_is_
exhaustive_because_netlink_is_used_elsewhere` holds it. `sock_diag` dumps the sockets that exist
at the instant of the dump; a connection that opened and closed between two dumps was never
visible to it, whatever the interface providers can do.

**The neighbour table is not subscribed to.** §22.4 lists neighbour changes among what netlink can
provide live, and `RTMGRP_NEIGH` is one `bind` away. It stays unjoined, because the runtime keys
watchability on the existence of an `ono.<target>-event/1` contract
(`ono_command::is_watchable`), there is none for `neighbor`, and adding one is a change to
`docs/contracts/schemas/` and to `ono-value`'s embedded contract list — another package's. A
subscription no user-facing path can reach is a capability nobody has, and §21.1's rule cuts both
ways: the provider says `live_events: false` because that is what is true today.

## Consequences

- Every `watch interface` and `watch route` event is dated by the notification rather than by the
  answer, so two changes inside one re-dump are dated by the notification that caused the re-dump
  rather than by when the dump finished.
- Coverage semantics are unchanged: these are still `partial`, still not exhaustive, and a
  buffer overrun still loses messages the kernel will not replay.
- **The date change has no test of its own.** Producing a link or address change needs
  `CAP_NET_ADMIN`, and this host allows neither that nor an unprivileged user namespace to borrow
  it in — `unshare --user --net --map-root-user` is refused. The claims the providers make are
  tested; the instant they stamp is not, and it is three lines of plumbing between a `poll` and a
  channel send. A host that can create a dummy interface could test it, and a future increment on
  such a host should.

## Alternatives considered

**Decode `RTM_NEWLINK`/`RTM_DELLINK`/`RTM_NEWADDR`/`RTM_DELADDR` and emit from them directly.**
The truest reading of "live evidence", and it produces incomplete objects for the reason above.
It would also make `watch interface` and `get interface` describe an interface differently, which
ADR-0235 rejected for good reasons that have not changed.

**Leave the re-dump instant.** It costs nothing and it is wrong by an amount that grows with the
size of the tables — on a host with a large routing table, by the whole dump.
