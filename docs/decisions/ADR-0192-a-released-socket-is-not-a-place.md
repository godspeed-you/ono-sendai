# ADR-0192: A released socket is not a place

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §14.3, §14.4 (listener and connection places), §7 (canonical topology), §10.3
  (tombstones), §37.1 and §42.1 (one object, one place), §2.16 (providers own facts);
  v0.2 §28.4 (`ono.socket/1`)
- Decided by: agent (autonomous), repairing the referee (AGENTS.md §14)

## Context

`spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
failed non-deterministically — twice in three runs of its own file, and in two consecutive
`scripts/gate.sh` runs — on a tree where nothing about sockets had changed. AGENTS.md §14 puts
repairing the gate ahead of any feature, so this was diagnosed before the v0.4 S9/S10 work
continued.

The failure is not the test's timing. The map the third live value carried held **two** nodes for
one connection:

```text
ono:lifetime:6454d229…  state established  object_ref { inode: 99403976 }  127.0.0.1:33749 -> 127.0.0.1:42848
ono:lifetime:cf39b367…  state time-wait    object_ref { inode: null }      127.0.0.1:33749 -> 127.0.0.1:42848
```

When the test's connection closes, the kernel keeps a 2MSL remnant of it in `time-wait`. That
remnant has **no inode** — `ono.socket/1`'s identity field — so `Projection::project_as` gives it
an identity of its own, and the spatial index registers it as a second, brand-new connection place
beside the one that just ended. Whether the closing poll reported the disappearance or the remnant
first decided whether the test saw the connection vanish or a new one appear.

Two nodes for one connection is exactly the duplicate §37.1 and §42.1 forbid, and it is visible
outside this test: `map` and `near` around a busy listener grow a phantom connection for every
connection that closes.

## Decision

**A socket in `time-wait` or `close` has no place.** `spatial_type_of` returns `None` for it, the
same answer §7 already gives a package or an environment variable, and the spatial layer therefore
never registers it, never draws it and never names it as a neighbour.

These are the two states of `ono.socket/1` in which no application holds the connection any more:
the kernel keeps the remnant so a late duplicate segment cannot be mistaken for new data, and
`ono-provider-netlink` already refuses every action on one — "a socket in time-wait has already
been released". A tear-down state that a process still owns (`fin-wait-1`, `fin-wait-2`,
`close-wait`, `last-ack`, `closing`) keeps its place: a socket stuck in `close-wait` is something
an administrator wants to stand in front of, not something to hide.

Nothing about the provider changes. `get socket` still lists the remnant with its state, because
providers own the facts (§2.16); what changes is only that the *spatial projection* does not treat
the kernel's own tombstone of a connection as a connection.

## Consequences

- One connection is one place for its whole life, and the closing of a connection is a
  disappearance rather than a disappearance racing an appearance. The live map test above is
  deterministic: four consecutive runs of its file, green.
- `enter socket <inode>` cannot reach a released socket — it has no inode to be named by, which is
  the same reason `ono-provider-netlink` refuses to act on one.
- The rule is stated over the socket's *state*, not over a missing inode. A missing inode was the
  tempting rule and is the wrong one: the `ss` adapter supplies no inode for internet sockets
  (v0.3), so "no inode, no place" would have deleted every adapted socket from the map and broken
  §37 rather than serving it.
- Encoded by `should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`
  and by the whole of `crates/ono-cli/tests/spatial_relationships_missing.rs`, which stands green
  and unchanged.

## Alternatives considered

- **Give a connection the identity of its four-tuple instead of its inode**, so the remnant
  reconciles with the connection it came from. It removes the duplicate node, and it leaves a
  closed connection standing in the map as a live place for the 60 s the kernel holds it — the
  honest-looking answer that is wrong.
- **Leave the test to its race and record it as deferred.** Rejected: a referee that fails a third
  of the time makes every later claim of progress worthless (AGENTS.md §14), and the race was a
  symptom of a defect the product has with or without the test.
