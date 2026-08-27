# ADR-0080: `watch interface|route` and what `trace route|interface` relate

- Status: accepted
- Date: 2026-08-27
- Spec refs: §18.2, §22.2–§22.4, §23.2, §28.5; ADR-0034, ADR-0078
- Decided by: agent (autonomous)

## Context

network.yaml promises `trace route` shows "which interface, gateway and neighbour a route
depends on" and `trace interface` "the routes, addresses, neighbours and sockets bound to an
interface"; `watch interface` and `watch route` had their event schemas deferred since
ADR-0034. ADR-0078 fixed the envelope and cadence; this ADR records the network edge set and
the one inference in it.

## Decision

### The two event schemas leave `deferred.yaml`

`ono.interface-event/1` carries the interface under `interface`, identity the netlink index
(a renamed interface is the same object `changed`); `ono.route-event/1` carries the route
under `route`, identity the five fields of route.v1.yaml (a route re-pointed at another
gateway is `removed` + `added`). Both are polled from rtnetlink dumps at ADR-0034's cadence,
`--table` narrowing the route watch as it narrows `get route`. Subscribing to the kernel's
`RTNLGRP_*` multicast groups is the optimisation ADR-0034 left in *Next up*, and stays there.

### Edge set

| Subject | Relation | Target | Read from | Confidence |
|---|---|---|---|---|
| `ono.route/1` | `via` | `ono.interface/1` | the route's `interface` name | exact |
| `ono.route/1` | `gateway` | `ono.neighbor/1` | neighbour with the gateway's address on that interface | exact |
| `ono.interface/1` | `route` | `ono.route/1` | every table's routes over the interface | exact |
| `ono.interface/1` | `neighbor` | `ono.neighbor/1` | neighbours reached through the interface | exact |
| `ono.interface/1` | `bound` | `ono.socket/1` | socket's local address is one of the interface's | exact |
| `ono.interface/1` | `bound` | `ono.socket/1` | socket's local address is unspecified (`0.0.0.0`, `::`) | **inferred** |

A gateway the neighbour table has not resolved contributes no edge: there is no object to
point at, and an edge to a made-up neighbour is what spec §22.4 forbids. Addresses are not
nodes — spec §28.5 makes them a field of the interface, and the interface node's summary
carries them (`value.addresses`).

The wildcard binding is the one inference: a socket bound to the unspecified address is
reachable through every interface, which follows from how `bind(2)` works rather than from
anything the kernel records per interface. The edge is marked inferred with that sentence as
its evidence, so a drawing shows it as `+~~` and a consumer can drop it.

Routes are looked up in every table, not the main one only: the loopback routes live in
`local`, and an interface trace that could not find them would be wrong on every machine.

### Order

Targets are emitted in identity order (`ObjectId` text for routes and neighbours, inode for
sockets), so two traces of one machine draw the same graph.

## Consequences

- `trace route 127.0.0.0/8` has `lo` as a node and `via` as an edge on every Linux system;
  `trace interface lo` reaches the loopback routes and any socket bound to `127.0.0.1`.
- Every socket reached is expanded one hop further by `SocketOwners`, which scans `/proc` per
  socket; an interface with many listeners costs that many scans. The node cap bounds it.
- Tests: `crates/ono-cli/tests/network_missing.rs` (watch interface/route, trace route
  through selector and pipeline, trace interface with addresses, routes and a bound listener)
  and `crates/ono-graph/tests/relationships.rs` (route → interface and gateway, interface →
  sockets exact and inferred, against stated records).

## Alternatives considered

- **Treating wildcard sockets as exact.** Rejected: spec §22.2 requires the UI not to imply
  certainty the provider does not possess, and the kernel does not record that binding per
  interface.
- **Nodes for addresses (`ono.interface-address/1`).** Rejected for the trace: the address is
  already in the interface's summary, and a node per address would double the graph without
  adding a relationship anyone asked about.
