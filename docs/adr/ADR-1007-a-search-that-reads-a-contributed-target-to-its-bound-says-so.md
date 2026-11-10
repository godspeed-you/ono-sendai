# ADR-1007: A search that reads a contributed target to its bound says so

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.4 §6.8, §29.4, §42.4; v0.4.1 §2.17, §34.4; ADR-0576, ADR-0584, ADR-0588
- Issues: #224 part (5)
- Decided by: agent (autonomous)

## Context

`find place` asks a contributed target for at most `CONTRIBUTED_SEARCH_OBJECTS` (1024) objects
(ADR-0584): nothing a package declares says how many objects it has, and a target may never end
(ADR-0588). The cap is right — a search has to finish — but it was silent. A search over a cluster
with three thousand pods answered from the first 1024 and read as an answer about all of them.

ADR-0576 settled the same question for an orientation: a bounded read states the bound, and the
population where the provider states one; where it states none, the count is unknown and is never
reported as the number that happened to be read (§42.4, §2.17).

## Decision

When a search reads as many objects from one contributed target as its bound, it states it on the
diagnostic stream, once per target:

```text
ono: find place read `k8s-pod` as far as its search bound of 1024 objects; the package states no
total, so places beyond the bound, if any, were not searched
```

or, where the provider recorded a population on the stream (ADR-0576 §2), "the package states
<n> in all, so the places beyond the bound were not searched". A target that ended before the bound
was searched completely, and nothing is said.

The answer itself is unchanged: `find place` emits places, and a script that pipes it keeps
reading places (§29.4). The bound is not configurable here; that is a separate question.

## Consequences

- A user can tell a complete search from a sampled one, and is told what the sample was.
- The statement is at `crates/ono-cli/src/spatial/find.rs` (`state_the_bound`), on the one path
  every search of a contributed target takes, including the full-screen map's `/`.
- Encoded by `crates/ono-cli/tests/spatial_contributed_targets.rs`
  (`should_state_the_search_bound_when_a_contributed_target_reaches_it`,
  `should_say_nothing_about_the_search_bound_when_a_contributed_target_ends_before_it`) and
  `docker/acceptance/cases/167-contributed-search-states-its-bound.case`.

## Alternatives considered

**A field on every place record** (`bounded: true`). It would put a fact about the search into a
record about a place, change the place record's schema, and be repeated on every row.

**A trailing summary value in the stream.** A script reading places would meet a value that is not
a place; §29.4 keeps the object stream for objects.

**Read until the end when the target is declared bounded.** A bounded target may still hold more
objects than a search should read, and ADR-0584's reason for the cap — a network round trip per
object — holds for it too.
