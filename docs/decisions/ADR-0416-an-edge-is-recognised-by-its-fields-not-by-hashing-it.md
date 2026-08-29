# ADR-0416: An edge is recognised by its fields, not by hashing it

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §33.2 (the providers are authoritative, the index is a cache), §3.5 (the fields
  of a relationship edge), §11.4 (`inspect relation @edge-17` — an edge has a name that survives
  being seen twice), §34 (performance budgets)
- Decided by: agent (autonomous)

## Context

`SpatialIndex::record_edge` files an edge under both of its ends. §33.2 makes a later observation
of the same edge replace the earlier one rather than sit beside it, so before pushing, the index
asks of every edge the place already holds: *is this the same edge?*

It asked that question by comparing `edge_id()`, and `edge_id()` is not a stored field. Every call
runs a SHA-256 over five strings and formats twelve bytes into a hex token. Both sides of the
comparison were inside the scan, so recording one edge cost two hashes per edge the place already
held — quadratic in a place's neighbour count, with a cryptographic hash in the inner loop.

Measured on this host with 920 processes, debug build, by timing the phases of `absorb`:

```text
absorb 6.5 s  →  settle 5.7 s  →  of that: settle itself 7.9 ms, record_edge 5.67 s
```

`find place --type process | count` took 6.16 s and a failing `enter` 23.9 s, which is past the
testkit's 20 s bound — so two tests in `spatial_contracts_missing.rs` failed for this, and were
being read as flaky.

## Decision

**Two edges are the same edge when the five fields that identify one agree** — source, target,
relation, direction, confidence — and `RelationshipEdge::same_edge_as` answers that question
directly. The index uses it; `edge_id` is unchanged and stays the edge's name in output, in
`inspect relation`, and wherever an edge must be referred to.

The two must never diverge, so `should_answer_the_same_as_the_edge_id_when_asked_whether_two_edges_are_one`
asserts, over identical, differing, inferred and attribute-carrying pairs, that `same_edge_as`
answers exactly what comparing the two ids answers. Attributes, validity and observation time are
not part of the identity: a re-observation that learned a file descriptor is the same edge, better
described.

## Consequences

Measured on the same host, same load, immediately before and after:

```text
find place --type process | count      6.16 s  →  0.59 s     (debug)
a failing `enter`                     23.93 s  →  4.76 s     (debug)
a failing `enter`                                  1.40 s    (release, 0.27 s of it CPU)
crates/ono-cli/tests/spatial_contracts_missing.rs   2 timeouts  →  27/27 green in 19 s
```

No test changed to accommodate this; the two that had been failing pass because the code got
faster. `same_edge_as` is public API and carries its own doc.

**What this does not fix, measured:** a selector that resolves to nothing walks §27.1 to its end,
and the last steps consult the whole index, so a miss projects all six domains where a hit stops
at the first. That is 1.40 s in release against 0.27 s of CPU — the rest is the cold read of 920
processes and their sockets, files and mounts. §34 permits it in as many words ("Cold provider
discovery MAY exceed these targets"), and its 50 ms figures are for `look` and `near` **cached**,
which are 0.10 s and below here. What §34 does require is that the shell stay interactive and
update progressively, which a one-shot `ono -c` never had to; the interactive path is where that
obligation lives, and it is worth measuring separately. Recorded on the board rather than fixed
here, because it is a design question (a persistent index, or a bounded last step) and not a
defect in this function.

## Alternatives considered

- **Hoist `edge.edge_id()` out of the scan.** Halves the hashing and leaves it quadratic; the
  measurement above is what ruled it out.
- **Store the id in the edge at construction.** Every constructor and every `with_*` builder would
  have to keep it current, and an edge whose stored id disagreed with its fields would be a defect
  nothing could see. The fields are the identity; deriving the answer from them cannot drift.
- **Key the edges of a place by id in a map.** Faster still, and it changes the order edges are
  answered in — which `look` and `near` render. Not worth a visible change for a scan that is now
  a field comparison.
