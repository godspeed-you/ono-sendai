# ADR-1020: A package is asked only for the relations the caller wants

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §32.1, §36.1; v0.2 §31.22, §31.26; ADR-0194, ADR-1006
- Issues: v0.6.5 review finding F5
- Decided by: agent (autonomous)

## Context

ADR-1006 gave a contributed relation a cost, and `contributions.v1.yaml` promised that a relation
declared beyond `normal` "is asked for only when the caller asks for it" (v0.4 §32.1: "Default
`look` and `map` MUST avoid expensive relationships unless cached or already available"). The
host held that promise only when *every* relation touching a place was declined. As soon as one
was wanted — a `normal` relation beside an `expensive` one, from the same kind of place —
`contributions::merge` invoked every contributing package's `spatial-relation` commands with
empty arguments, and a package computed its expensive edges on every default `look` while the
exit said "available on request". `map` asked every package for everything, whatever its cost.

The contribution protocol (ADR-0194) gives a `spatial-relation` command no argument of its own,
but every command is invoked with an argument map, and a package reads only the arguments it
knows: adding one breaks no package.

## Decision

1. The host invokes a package's `spatial-relation` commands with one argument, `relations`: the
   list of that package's relation ids (`<package>.<from>_to_<to>`, `relations.yaml` →
   `contributed_relations.id`) the caller wants. A package whose relations are none of them is
   not invoked.
2. Which relations are wanted is the decision the core relations already follow: at a place,
   `look`/`near`/`follow` want a relation that is `cheap`/`normal` (`CostClass::is_eager`) or
   that the caller named (`--all`, `near --type`, `follow <word>`); `map` wants every relation
   where it reads its centre completely (`--all`, or a horizon deeper than one hop — the same
   rule its expansion of the centre applies), and otherwise the eager ones and those
   `--relations` names.
3. A package answers only the relations named. The host does not discard what a package answers
   beyond them: an edge it was handed is "already available" in §32.1's words, and discarding it
   would not refund the cost.

## Consequences

- With a `normal` and an `expensive` relation from one kind of place, a default `look` asks the
  package for the normal one only; `follow <name>` of the expensive one asks for that one.
- **Limitation:** the host cannot make a package compute less than it chooses to. A package
  written before this decision ignores the argument and still pays for everything it declares;
  the host's half — asking for less, and not invoking a package with nothing wanted — holds for
  every package.
- A map two hops deep (the default `map` at an observed object) reads its centre completely and
  so asks for expensive relations too — as it already did for the core's broad providers. That is
  the earlier rule of the map's expansion, unchanged here.
- `docs/contracts/kuang/contributions.v1.yaml` → `relation.manifest_shape.asked_with` states the
  argument. `ono_spatial_query::MapRequest::names_relation` answers whether `--relations` names a
  relation.
- The example package honours the argument and records each relation it computed in its private
  working directory, which is what the tests read. Encoded by
  `crates/ono-cli/tests/spatial_contributed_relations.rs`
  (`should_not_ask_the_package_for_an_expensive_relation_when_a_default_look_reads_a_normal_one`,
  `should_read_the_expensive_relation_of_a_mixed_package_when_it_is_followed`,
  `should_not_ask_the_package_for_an_expensive_relation_when_a_one_hop_map_is_drawn`).

## Alternatives considered

**One invocation per relation.** Each command would answer one relation, so a package could not
fail to filter — but a relation command answers for any number of shapes, and a package with
five cheap relations would be started five times per `look`.

**Filter on the host only.** Discarding the edges of declined relations after the package
answered makes the index consistent with the exits and spends exactly what the defect spent: the
cost §32.1 is about is the package's, not the index's.

**A new protocol field (a typed `relations` selector on the command contribution).** It would
let the conformance host check that a package declares it reads the argument, at the price of a
protocol version; the argument map already carries it compatibly.
