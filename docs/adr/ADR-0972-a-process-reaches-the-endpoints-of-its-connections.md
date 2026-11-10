# ADR-0972: A process reaches the endpoints of its connections

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §2.16, §11.2, §11.4, §11.5, §12, §14.4, §32.1, §35.2, §42.3; ADR-0135, ADR-0971
- Decided by: agent (autonomous)

## Context

`process.connects_to` — a process to the endpoint at the far end of a connection it holds — has
been a declared relation since the spatial contracts were written, and nothing served it. A
process place's `endpoints` exit answered `unsupported` (#206). The facts were there all along:
a socket record names the process that holds the socket (`process`) and the peer the socket is
connected to (`remote`), and the bridge already turns the second into `socket.connected_to`,
with the peer as a composed `Endpoint` place (ADR-0135).

Every relation composed so far ran from the object a record describes to a place the record
names. This one runs between two places a socket's record relates — its holder and its peer —
with the socket itself as the evidence in the middle. A socket record from the kernel's socket
table names its owner only when the owner scan was asked for (`get socket --process`), so the
holder is usually known from the other side: the process's descriptor table.

## Decision

**A process that holds a connected socket reaches that socket's peer**: `process.connects_to`
runs from the process to the endpoint place `socket.connected_to` leads to. Two observations
state who holds a socket, and each asserts the edge where it is made:

- **The descriptor table.** At a process place the provider `linux.process-sockets` reads the
  sockets the process holds. For each one whose record names a peer, the edge is recorded with
  that provider as its source. This is the path a native observation takes: a socket record
  from the kernel's socket table carries no owner unless the owner scan was asked for.
- **The socket record.** A socket record that names both its owner (`process`) and its peer
  (`remote`) — one read with the owner scan, or an adapter's (`ss -p`) — asserts the edge in the
  bridge. The assertion belongs to the socket record: while the owner is unobserved it waits, as
  every other reference to an unobserved place does (§42.3), and it is dropped with the socket.

In both cases:

- The edge is `exact`. The socket table states the peer, and the owner is the process whose
  descriptor table holds the socket; nothing is inferred. The contract's
  `exact_or_provider_declared` leaves room for a provider that only knows a name for the far end.
- The edge carries the attribute `socket`, the inode of the socket it was read through, which is
  the explanation §11.4 asks an edge to carry.
- A socket whose owner the caller may not see asserts nothing about a process. The connection
  still leads to its peer.

**At a process place the `endpoints` exit is filled by the read that fills `sockets`.** Both come
from the process's socket records, so one provider answers for both exits: where it reads, both
are lists (an empty list for a process with no connection); where it is refused, both carry the
refusal.

## Consequences

Easy: `near endpoints` and `follow endpoint` at a process place; `enter process <pid>; near
--type endpoint` answers "who does this process talk to" without a detour through each socket.
The inverse, an endpoint's `clients`, lists the processes of the sockets observed so far.

Hard: an endpoint is a place only as far as its name goes (ADR-0135) — `10.0.0.5:443` on two
networks is one place. The edge inherits that. A connection that closed leaves its edge until the
process is observed again, as every cached edge does (#216).

Encoded by `crates/ono-spatial-index/tests/relations.rs::
should_lead_from_a_process_to_the_endpoint_its_connection_reaches`,
`::should_join_a_process_to_its_endpoint_when_the_process_is_observed_after_the_socket`,
`::should_claim_no_endpoint_for_a_process_when_the_socket_names_no_owner`,
`crates/ono-cli/tests/spatial_relationships.rs::should_lead_from_a_process_to_the_endpoints_it_is_connected_to`
and acceptance case `133-process-endpoints`.

## Alternatives considered

**A relationship provider of its own (`linux.process-endpoints`)** that walks a process's sockets
and yields endpoints — rejected: it would read the socket table a second time for facts the
first read already carries, and its endpoint nodes would need the same composition the bridge
does for `socket.connected_to`.

**Derive the edge at view time by walking `owns_socket` then `connected_to`** — rejected: the
relation is declared as an edge of its own, `follow endpoint` needs something to traverse, and a
two-hop derivation would have to be repeated in `near`, `follow`, `map` and `trace`.

**Remove the relation** — the issue names it as an option. Rejected: §12 gives a process place
its network reach, and the data to answer it is already read.
