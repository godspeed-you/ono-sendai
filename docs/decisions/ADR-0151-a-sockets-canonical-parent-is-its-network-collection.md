# ADR-0151: A socket's canonical parent is its network collection, not the process that owns it

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §6.6, §11.3, §14.1, §43.2, §44.6, §53; and ADR-0140
- Decided by: agent (autonomous, S4c)

## Context

`up` follows the canonical hierarchy and `back` follows history (§53), and §44.6 makes the
difference an acceptance scenario: after `service -> follow process -> follow socket`, "`back` must
return to the process" and "`up` from the socket must return to its canonical network hierarchy
parent, demonstrating the distinction."

The canonical-parent rule chain shipped with S1 read, for a socket:

```rust
const SOCKET: &[ParentRule] = &[rule("process.owns_socket", Containment)];
```

so `up` from a listener reached the process that owns it — the same place `back` reaches, and the
same place `follow owner` reaches. The distinction §44.6 is about did not exist, and §43.2's
property ("`up` never traverses arbitrary graph edges") was violated by the one rule that made
`up` traverse a graph edge on purpose.

## Decision

**A socket has no operational canonical-parent rule.** `parent_rules(Listener)` and
`parent_rules(Connection)` are empty, so `canonical_parent` falls through to the collection space
of the geography that holds the type: `network.listeners` for a listener, `network.connections` for
a connection. That is §14.1's own hierarchy — `NETWORK -> LISTENERS -> tcp/:443` — and it is the
path `enter network; enter listeners; enter :443` walks, so `up` is now the exact inverse of the
`enter` chain rather than a shortcut through the graph.

The owning process stays what it always was: a relationship edge, reachable with `follow owner`,
listed by `near`, and explained by `inspect relation`. §11.3's "the canonical parent does not claim
that other relationships are less real" cuts both ways — and so does its converse.

`docs/spec/providers/linux-netlink.yaml` declares the same chain, because `spec-check` holds the
provider's `canonical_parent` claim against `parent_rules` on every gate run.

## Consequences

- `up` from a socket lands under NETWORK; `back` from the same place lands on the process. §44.6
  is demonstrable, and `spatial_relationships_missing::should_leave_the_relationship_chain_with_up_after_following_a_socket_edge`
  and `spatial_navigation_missing::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`
  encode it.
- A socket's `place_path` becomes `local/network/listeners` rather than a path through its owner.
  That is what §27.2 wants of a path column: it disambiguates by where the object *lives*, and two
  listeners on one process were never distinguished by naming that process twice.
- A connection's parent moves from its accepting listener to `network.connections` for the same
  reason. Nothing depended on the old answer; the `socket.accepts_connection` edge is untouched
  and `follow` still traverses it in both directions.

## Test assertion changed with this ADR

`spatial_navigation_missing::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`
built its haystack from the place's `display_name` and `scope`. Under ADR-0140 the field that names
where a place sits in the canonical hierarchy is `place_path` (`local/network/listeners`), while
`scope` is the §3.2 boundary (`host:web01`) — so the test was reading two fields, neither of which
carries the answer it asks for. The haystack now includes `place_path`. The assertion is unchanged
in what it demands: the place `up` reaches lies under NETWORK, and is not the place `back` reaches.
The sibling test in `spatial_relationships_missing` already rendered the whole place record and
needed no edit.

## Alternatives considered

- **Keep the process as the socket's parent and weaken §44.6's tests.** Refused: the scenario is
  the reason §6.6 distinguishes the two verbs at all.
- **Make `network` itself the parent, skipping the collection.** Would make `up` disagree with the
  `enter` path that reaches the same place, and §14.1 spells the collection out.
- **Let the parent depend on how the place was reached.** §11.3 requires the canonical parent to be
  deterministic for a given view profile; a parent that remembered the last `follow` would be
  history wearing hierarchy's name.
