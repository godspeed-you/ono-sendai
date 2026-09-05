# ADR-0235: A watch is told, where the kernel can tell it

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §18.2 ("providers MAY support event-driven updates"; polling must be explicit),
  §18.3, §31.14 (the event envelope), §31.15 (bounded event queues), §34 (latency is a product
  property); v0.4 §25.1, §25.3 (a live view says how it is kept current);
  ADR-0024 (watch semantics), ADR-0034, ADR-0078, ADR-0083
- Decided by: agent (autonomous)

## Context

ADR-0034 built every `watch` on one runtime loop that takes a snapshot, diffs it by identity and
sleeps. It was the right first answer — one loop, one set of semantics, and `source: poll` on
every event so the cost was never hidden — and it left two things true that spec §18.2 does not
require to be true:

- **Nothing could be reported sooner than the interval.** A file created a moment after a tick
  waited two seconds. On a machine where nothing changes, the loop still enumerated everything
  every two seconds.
- **`Provider::subscribe` existed and nothing implemented it**, so no watch anywhere could report
  `source: subscription`, and §18.2's distinction had exactly one value in practice.

The kernel offers the answer for both families this touches. `inotify(7)` reports what happens
under a directory; the rtnetlink multicast groups of `rtnetlink(7)` report links, addresses and
routes as they change.

## Decision

**The watch runtime subscribes where a provider can be told, and polls where it cannot.**
`watch_stream` asks `ProviderRegistry::subscribe` first; a provider that refuses is polled exactly
as before, and its refusal never reaches the user, because `watch` works either way and `source`
is what says which way it worked.

Three rules make the two paths one contract:

1. **The snapshot is the runtime's, never the provider's.** A subscription is opened *before* the
   first snapshot is taken — so a change during the read is queued rather than lost — and the
   runtime then emits the current state as `snapshot` events. `watch x | take 1` therefore means
   the same thing on both paths (ADR-0024).
2. **The runtime reconciles.** It keeps what the snapshot established, so an `added` for an object
   the snapshot already carried is reported as the `changed` it really is, and a `removed` for one
   it never carried is dropped. That is what makes the subscribe-then-snapshot order safe.
3. **A subscription narrows exactly as the listing does.** `watch interface lo` is about `lo`, and
   the netlink subscription filters with the same `keep` its `get` uses.

**`ono-provider-linux` watches files through inotify.** A watch of a directory watches that
directory (and, with `--recursive`, every directory beneath it, including the ones that appear
later); a watch of one entry watches its parent filtered by name, which is the only way the
creation of a file that does not exist yet arrives at all. `IN_CLOSE_WRITE` rather than `IN_MODIFY`
makes one `echo >>` one event; `IN_ATTRIB` makes a `chmod` a change, because `ono.file/1` carries
the mode. The entries present when the watch opens are read once, because the kernel says only
that `<name>` is gone and a `removed` event has to carry the object that went.

**`ono-provider-netlink` watches interfaces and routes through the multicast groups.** The kernel
says *that* something changed; the answer to *what* is a fresh dump through the same decoders a
`get` uses, diffed by object identity. One description of an interface in the crate rather than
two that can drift, and one dump per change rather than one per tick — on an idle machine, none.

Both readers wait on a file descriptor with a 200 ms timeout. That is not a polling interval:
nothing is re-read when it expires, and it exists only so a cancelled watch stops within it.

## Consequences

- `watch file src` reports a created file in milliseconds instead of up to two seconds, and says
  `source: subscription`. `watch interface` and `watch route` say the same.
- `watch process`, `watch service`, `watch socket`, `watch user`, `watch group`, `watch mount`
  and `watch container` are unchanged and still say `source: poll`: their providers implement no
  `subscribe`, so nothing about them moved. Acceptance case `046-live-system-semantics` asserts
  exactly that for `watch process`, and it is untouched.
- v0.4 §25.3's freshness follows: a live view over a subscribed class can now honestly report
  `event_driven` where it could only report `polled`.
- A subscription that cannot be opened is not an error the user sees. `watch file` over a
  filtered walk (`--name`, `--kind`, several roots), or a directory this user may not watch, or a
  kernel without inotify, all fall through to the poll loop with the same answers.
- The two readers are threads rather than async tasks, because `inotify` and a netlink socket are
  blocking file descriptors and `nix::poll` is the honest way to wait on one. Each stops within
  200 ms of its consumer going away, and neither can outlive it: the channel is what tells them.
- Encoded by `should_report_a_created_file_before_the_next_poll_would_have_come`,
  `should_watch_interfaces_through_the_kernel_rather_than_by_asking_it_again`,
  `should_watch_routes_through_the_kernel_rather_than_by_asking_it_again`, and by the whole of
  `watch_live.rs`, `files_missing.rs` and `network_missing.rs`, which stand green and unchanged
  otherwise.

## Alternatives considered

- **Decode the multicast messages themselves** instead of re-dumping. It is the smaller number of
  syscalls and the larger amount of code: a second decoder for links, addresses and routes beside
  the dump decoders, which would then have to be kept saying the same thing about the same
  objects. §2.16's rule that a provider owns the facts is easier to keep with one decoder.
- **Have the provider emit the snapshot too.** Then `watch x | take 1` would depend on whether a
  subscription happened to exist, and every provider would have to reimplement ADR-0024.
- **Subscribe after the snapshot.** Simpler to write and it loses every change that happens while
  the snapshot is being read — precisely the changes a watch exists to catch.
- **A shorter poll interval for files.** It buys latency with load on every machine, and it still
  cannot see a file that was created and removed between two ticks.
