# ADR-0985: The link map states a round trip, a last seen and a shared host

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §2.6, §2.17, §14.5, §19.1, §19.3, §19.4, §35.2, §35.3, §35.4; ADR-0171, ADR-0964; issue #213
- Decided by: agent (autonomous)

## Context

§19.1 shows the link map as

```text
links
  prod/web01       connected     12ms
  prod/db01        connected     13ms
  home/nas01       disconnected  last seen 3h ago
```

The local root carried the links as `ono.link-place/1` records — a name, a host, a state — with
no field for a latency or a last-seen time, and the text `look` did not print them at all.
Issue #213 lists five gaps of §19; this decision covers what can be stated truthfully today and
records what cannot.

## Decision

**`round_trip`** — a connected link states how long its handshake's one exchange took, the
offer written and the answer read. It is the only round trip this build times: the link
protocol has no probe. It bounds the latency from above at the moment the link was made and
includes the far side beginning to answer, which the contract says in so many words. A link
that is not connected and followed, and one made without a handshake, state none: null, never
zero.

**`last_seen`** — when this session last held the link connected and followed it. A link the
session detached from keeps the moment it stopped following; a definition nobody connected was
never seen.

**`same_host_as`** — when a link is made, the far side's own `kernel` target is asked once
which boot of which kernel it runs (`ono.kernel/1`'s `boot_id`). Two links that answer with the
same boot reach one machine, and each names the other. The scopes stay two: a place is still
identified through the link it was reached by (§2.17 prefers the false distinction to a false
merge), so this is context beside the map, never identity.

**`look` at the local root prints the link map** — name, state, then the round trip, `last
seen <age> ago` or `never connected`, and the links it shares a host with.

Not delivered, and why:

- *Latency as a live measurement.* It needs a probe frame in the link protocol; the handshake's
  round trip is what stands in.
- *Two-sided cross-host correlation* (§14.5, §19.4): recognising that a local connection's
  remote endpoint is a socket on a linked host needs the far side's socket table read from a
  place on this side, and a spatial command reaches one host's providers at a time.
- *A second hop on `map links`* (§19.3's `web01 ----- db01`): a link between two remote hosts
  is in the far side's link table, which an agent does not hold.
- *A neighbour reached by hierarchy* keeps a null `confidence` and `provider`: there is no
  relationship to explain, and §2.6 forbids inventing one.

## Consequences

Easy: the first thing a user sees at the root says which hosts are reachable, how far they
were, and which two names are one machine.

Hard: making a link costs one more exchange, the read of the far side's kernel record. The
round trip ages: a link made hours ago reports the handshake of hours ago. A far side whose
kernel publishes no boot id, and the agentless fallback, share a host with nobody as far as
the map can tell.

Encoded by `crates/ono-cli/tests/spatial_remote.rs::should_say_how_long_the_handshake_took_and_when_a_link_was_last_followed`,
`::should_say_when_two_links_reach_one_machine`,
`::should_print_the_link_map_when_look_draws_the_local_root` and acceptance case
`142-link-map-round-trip-and-last-seen`.

## Alternatives considered

**Time every query and report the last one.** A number that depends on what was last asked — a
process listing against a single file — is no property of the link.

**Read the shared host from the handshake's clock identity.** The peer's `boot_id` field
exists for it, and the shipped agent announces no clock; the kernel record is a provider's
answer rather than a self-description, and it is there on every agent link.

**Merge the scopes of two links to one machine.** Every identity behind either link would
change the moment the second link was made, and a trail recorded through one name would point
at places that no longer carry it.
