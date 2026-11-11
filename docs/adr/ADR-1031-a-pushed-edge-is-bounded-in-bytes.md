# ADR-1031: A pushed edge is bounded in bytes, so a package's edges are bounded in memory

- Status: accepted
- Date: 2026-11-11
- Spec refs: v0.2 §31.26, §31.33, §31.80; v0.4.1 §2 (resource boundedness); ADR-1014
- Issues: #203; v0.6.5 re-review finding F1
- Decided by: agent (autonomous)

## Context

ADR-1014 keeps the edges a package asserts through `relations.contribute` in the session's
spatial registry and bounds them at 1024 per package, so that "a package cannot grow the store
without limit". The bound counted edges only. An edge's `metadata` is a record of any size and its
keys are strings of any length; the one limit was the 1 MiB protocol frame. The re-review of
2026-11-11 granted `relation.write` and asserted 300 edges of about 900 KB of metadata each: the
shell grew to 1.7 GB of resident memory, far beyond the package's own `memory_max` (64 MiB), and
every later `look`, `map`, `follow` and `relations.query` copies the store again. At the
1024-edge bound that is several gigabytes per package.

## Decision

**An edge a package pushes takes at most 4096 bytes, measured as the JSON the package sent; a
call carrying a larger edge is `resource.item_limit`, and nothing of that call is kept.**

- The measure is the edge as written on the wire, so keys, metadata and every other field count,
  and the package can compute the same number before it sends.
- 4096 is the bound `history.append` already puts on a package's entry (ADR-1015): an edge's
  metadata is relationship detail, not a payload, and one page of it is generous.
- With the 1024-edge bound this caps what one package can make the shell hold at about 4 MiB,
  the same order as the package's own memory budget.
- The refusal is the existing `resource.item_limit` with the all-or-nothing rule ADR-1014 set for
  the count: a call is accepted whole or not at all.

## Consequences

- A package that used large metadata must keep it elsewhere and put a reference on the edge. No
  package in the repositories does; the example package's edges carry none.
- The contract `docs/contracts/kuang/protocol.v1.yaml` → `relations.contribute` states the byte
  bound beside the count.

Encoded by `crates/ono-cli/tests/host_services.rs::should_refuse_an_edge_larger_than_the_bound_and_keep_nothing_of_its_call`
(an 8 KiB edge refuses its call; a 1 KiB edge is kept) and, for the count,
`::should_bound_what_one_package_contributes_and_say_so`.

## Alternatives considered

- A total byte budget per package instead of a per-edge bound — rejected: it lets one edge take
  the whole budget, and the error would depend on what the package sent earlier rather than on
  the edge in hand.
- Cap only `metadata` — rejected: an unbounded key string is the same exhaustion by another field.
