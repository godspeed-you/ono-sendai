# ADR-0651: A relation that was not observed is a third answer and not an absent one

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.4, §9.5, §9.7, §14.3; v0.4 §11.5
- Decided by: agent (autonomous)

## Context

§9.5 is two sentences and both are load-bearing. "A relation exists at `T` only if reconstruction
supports its existence at `T`" — so an edge is drawn from evidence about `T` and never from the
present. "Unknown relation state MUST be distinguishable from absent relation state" — so the
answer has three values and not two.

§14.3 states the consequence a user sees: an exit shown by `look` or `near` at `T` "MUST
correspond to a relation or hierarchy supported at `T`", and "current-only exits MUST not leak
into a historical neighborhood".

§9.7 asks the same question about places: a place that is not in the reconstruction reports
"place not known at requested time", "with coverage explaining whether this means known-absent or
simply unknown".

A `bool` cannot carry three answers, and an `Option<bool>` carries them in a shape every caller
is free to flatten with `unwrap_or(false)`.

## Decision

**`Presence` is an enum of three, and it is the type both objects and relations answer with.**

```rust
pub enum Presence { Present, Absent, Unknown }
```

- `Present` — reconstruction supports it at `T`, under ADR-0650's rule.
- `Absent` — a source that could have proven the absence did: either an observed removal that
  complete coverage carries to `T`, or `CoverageSummary::can_prove_absence` for that relation's
  capability with nothing supporting it (§7.4).
- `Unknown` — everything else. Nothing observed it and nothing could have proven it was not there.

`ReconstructedWorld::relation(from, to, relation)` answers for any triple, including one nothing
in the ledger mentions, so a caller asking "was this edge there?" gets a temporal answer rather
than a lookup miss. `presence_of(id, object_type)` is the same for objects, which is §9.7's
question with §9.7's two-way distinction in the answer.

`ReconstructedWorld::relations()` yields only the edges whose presence is `Present`. §14.3 is then
a property of the API rather than a rule a renderer has to remember: a caller drawing whatever
this iterator yields cannot leak a current-only exit into a historical neighbourhood.
`all_relations()` exists beside it for `inspect`, which is the one caller that wants to see an
edge that was there and is not.

An edge's `valid_from` is the earliest supporting observation at or before `T`; `valid_until` is
the instant an observed removal ended it, and null where no observation ended it. Null is "no
source ended it", never "it is still there now" — nothing in a historical answer speaks about the
present.

## Consequences

- A caller cannot accidentally render unknown as absent, because there is no boolean to collapse
  into. A caller that wants two answers writes `presence.is_present()` and says so.
- `Presence::Absent` is expensive: it needs coverage capable of proving it. In a session with no
  declared coverage every unobserved edge is `Unknown`, which is the honest reading of "nothing
  was watching".
- v0.4's `Confidence` travels beside `Presence` and answers a different question — how well the
  edge itself is known (§11.5) — so an `inferred` edge that was certainly there and an `exact`
  edge that may not have been stay two separate statements.
- Encoded in `crates/ono-temporal-reconstruct/tests/relations.rs` and `tests/absence.rs`.

## Alternatives considered

- **`Option<bool>`.** Three states in the type and two in every call site, because `unwrap_or`
  exists and is the shortest thing to write.
- **A separate `unknown_relations` list beside the present ones.** Two lists that must be read
  together, and a caller who reads one is silently wrong.
- **Treating "no supporting event" as absent.** The defect §7.4 exists to prevent, applied to
  edges: a sparse journal with no relation event is not proof that the relation never held.
