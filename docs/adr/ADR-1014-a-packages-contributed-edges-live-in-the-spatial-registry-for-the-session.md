# ADR-1014: A package's contributed edges live in the spatial registry for the session

- Status: accepted
- Date: 2026-11-10
- Spec refs: §22.2, §31.5, §31.7, §31.12, §31.26, §31.80; v0.4 §11.5, §35.5, §36.1, §36.2, §53;
  K11P §7.1; `docs/contracts/kuang/protocol.v1.yaml` (`relations.contribute`, `relations.query`);
  `docs/contracts/schemas/graph-edge.v1.yaml`; issue #203; ADR-0568, ADR-0585, ADR-0600 §2,
  ADR-1006, ADR-1008
- Decided by: agent (autonomous)

## Context

`relations.contribute` answered `provider.unavailable`: there was no store for a package's edges
(issue #203). Since then the spatial workstream built a contributed-relation registry (ADR-1006,
ADR-1008): a package declares `<from>-><to>` shapes in `contributions.relations`, each becomes a
relation `<package>.<from>_to_<to>` once the package holds `relation.write`, and `map`, `look` and
`follow` ask the package's `spatial-relation` command for edges and resolve both ends through the
canonical providers. That is a *pull*. `relations.contribute` is the *push* the protocol declares
beside it, and the contract settles less than it seems: where the edges live, which relations a
package may push onto, how an end is named, what bounds the store, and what happens to the edges
when the package goes.

## Decision

1. **One registry, two doors.** A pushed edge is kept in the spatial contributions module, beside
   the pulled ones, and the one `merge` every spatial reader calls yields both, resolved through
   the canonical providers the same way, attributed the same way (`provider`/`origin` = the
   package) and held below `exact` the same way. `relations.query` reads the same store. There is
   no second edge model.
2. **Session-scoped.** The edges live as long as the session, and no longer than the package is
   loaded. Spec §31.26 asks for no persistence, and an edge a package observed in one session is
   no evidence of the next one; a package that wants its edges again asserts them when loaded.
3. **Only along the package's own relations.** An edge's `relation` must be one the package
   contributed from its manifest shapes. A bounded grant (ADR-0600 §2) already confines the ids
   at the broker; an unbounded `relation.write` grant would let any name through, and the host
   still refuses an edge along a relation the package did not declare — a package does not add
   edges to `process.parent` and have them read as the kernel's.
4. **Ends are object ids of the relation's kinds of place.** `from` and `to` are object ids whose
   schema the provider of the relation's source (target) kind answers with, carrying the identity
   field that kind is found by — `{schema: ono.process/1, pid: 4419}`, or the full
   `{schema, values}` a package received from `objects.resolve`. The host does not resolve them
   when it accepts the call; it resolves them where every contributed edge is resolved, so an end
   nothing answers to contributes no edge (§36.2's "uninspectable phantom edges").
5. **The host sets the provider and never vouches.** `provider` cannot be supplied: an edge that
   names one is refused (§31.80, "plugin forges provenance"). `confidence` is required, `exact` or
   `inferred` (§22.2: never defaulted); since the host observed nothing, `exact` is held one step
   below — `strong` on a map, `inferred` on `relations.query`'s two-valued vocabulary — and
   `inferred` stays `inferred`, which is §31.26's MUST.
6. **All or nothing.** An edge outside `ono.graph-edge/1` (an unknown field, a missing or unknown
   `confidence`, an end of the wrong kind or without its identity field, an `undirected` edge on a
   directed relation) refuses the whole call with `runtime.schema_violation`. So does a call whose
   `edges` is not a list; the supervisor used to read that as an empty list and answer `0`.
7. **Bounded per package.** A package holds at most 1024 edges at once. Asserting an edge again
   (same relation, same ends) replaces it rather than counting twice, so a package re-asserting
   what it sees each tick never fills the store; a call that would leave it holding more is
   `resource.item_limit`, naming the count, and nothing of that call is kept.
8. **Unload withdraws.** `unload plugin`, and `remove plugin`, which unloads first, forget the
   package's pushed edges and its pulled relations' contributor; before this the unloaded
   package stayed in the pull registry and was asked again for edges it could no longer answer.

## Consequences

- A package can say "this process feeds that one" and the operator can `look`, `follow` and `map`
  along it, attributed and never exact; another package reads it through `relations.query`.
- `trace` does not see contributed edges, pushed or pulled: it is assembled in `ono-command` from
  the kernel relationship sources alone. Reaching it needs a contributed-edge source on the graph
  boundary for both doors at once, which is its own change.
- `relations.query` walks from an object the v0.2 graph can identify (`ono_graph::Node::of`); a
  contributed kind of place as the starting point stays outside it, as it was.
- Tests: `crates/ono-cli/tests/host_services.rs` — `relations.query` answers attributed and
  inferred, `follow` walks a pushed edge, a forged `provider`, six malformed calls, a core
  relation under an unbounded grant, a denied permission, the bound and re-assertion, and the
  withdrawal on unload; acceptance case 176.

## Alternatives considered

- **A store of its own in `kuang_services`.** Rejected: `map` and `follow` would need to read two
  stores, which is the parallel model the spatial registry exists to prevent.
- **Persisting edges across sessions.** Rejected: nothing in §31.26 asks for it, a stale edge is
  worse than none, and the pull door does not persist either.
- **Resolving both ends when the call is accepted.** Rejected: it costs a provider snapshot per
  end per call, and the place it would prove to exist could be gone by the time anyone looks.
- **Evicting the oldest edges at the bound.** Rejected: the package could not tell which of its
  assertions were still believed. Refusing the call says so.
