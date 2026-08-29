# ADR-0127: The kind of each of the fourteen spatial errors

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §40 (error model), §4, §33.2, §35.2, §35.4, §8.2, §37.1, §42.1;
  v0.2 §16.1 (the twelve kinds), §43 (the code taxonomy)
- Decided by: agent (autonomous)

## Context

ADR-0125 allocated the fourteen conditions of v0.4 §40 as the `spatial` family,
`Ono-Sendai-E1001`–`E1014`, in the order §40 lists them. It did not say which of the twelve
kinds of v0.2 §16.1 each one carries, and the kind is the field a script branches on: `catch e
{ if $e.kind == "permission" { … } }` must keep meaning the same thing across families, and
`docs/spec/errors.yaml` may only use a kind the registry itself declares.

§40 gives no kinds. Several of the fourteen have an obvious mate in an older family
(`spatial.permission_denied` beside `io.permission_denied`) and several have none
(`spatial.map_too_large`, `spatial.history_empty`).

## Decision

| Code | Name | Kind | Why |
|---|---|---|---|
| E1001 | `spatial.not_found` | `resolution` | A selector did not resolve to a place, exactly as `resolve.target_not_found` |
| E1002 | `spatial.ambiguous_selector` | `resolution` | The `resolve.ambiguous` condition, in the spatial vocabulary |
| E1003 | `spatial.not_enterable` | `type` | The object exists; its *type* is not a destination (§41.1's `enterable`) |
| E1004 | `spatial.no_relation` | `resolution` | A relation *name* did not resolve; a known name with no neighbour is E1001 |
| E1005 | `spatial.no_parent` | `resolution` | The canonical parent edge does not resolve (§11.1) |
| E1006 | `spatial.history_empty` | `conflict` | Nothing failed to resolve — the session state does not permit the move |
| E1007 | `spatial.destination_gone` | `resolution` | The trail entry no longer resolves to a live object (§20.3) |
| E1008 | `spatial.permission_denied` | `permission` | Same kind as `io.permission_denied`, so kind-branching scripts keep working (ADR-0125 §5) |
| E1009 | `spatial.unsupported` | `provider` | No provider can answer; the mate of `provider.unsupported` (§4) |
| E1010 | `spatial.stale` | `provider` | The provider's observation is too old to act on; the index is a cache (§33.2) |
| E1011 | `spatial.remote_unavailable` | `provider` | Same kind as `remote.unreachable`, which ADR-0006 already fixed as `provider` |
| E1012 | `spatial.scope_violation` | `permission` | A boundary this session may not cross (§35.4) — an authority decision, not a lookup |
| E1013 | `spatial.map_too_large` | `stream` | The mate of `stream.unbounded_operation`: the *result set* is unbounded for the budget (§8.2, §34.2) |
| E1014 | `spatial.identity_conflict` | `conflict` | Two asserted identities for one object cannot both hold (§37.1, §42.1) |

The rule behind the table: the kind describes *what class of thing went wrong*, not which
subsystem raised it. A spatial code therefore takes the kind its non-spatial mate already has,
and where §40 names a condition with no mate, the kind is chosen from the same twelve by the
question a script would ask — "did a name fail to resolve", "was I forbidden", "did a provider
fail to answer", "does the state disagree", "is the result unbounded".

## Consequences

- `crates/ono-core/tests/error_taxonomy.rs::should_carry_the_spatial_family_of_the_v04_specification_when_enumerated`
  pins the fourteen (code, name, kind) triples, and `spec-check` holds `docs/spec/errors.yaml`
  against `ono_core::ErrorCode` in both directions, so the table above cannot drift silently.
- A script that already branches on `permission` catches a denied neighborhood group without
  being taught a new kind; one that branches on `stream` catches a map that will not fit.
- `spatial.map_too_large` carrying `stream` is the one assignment a reader may find surprising.
  It is recorded here rather than left to be rediscovered: the alternative, `conflict`, would
  put "the map is bigger than the budget" beside "the state disagrees with itself", which is a
  different question.

## Alternatives considered

- **A new kind, `spatial`.** Rejected: §16.1's kinds are categories of failure, not subsystems,
  and the family name already carries the subsystem. Adding a thirteenth kind would also break
  every exhaustive match over `ErrorKind` for no gain.
- **`resolution` for all seven "there is nothing there" conditions.** Rejected: `history_empty`
  and `identity_conflict` are not lookups, and collapsing them would make `kind` less
  informative than `name`, which is the wrong way round.
