# ADR-0780: A historical `get` is answered by a temporal seam in front of every provider

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §4.1, §4.5, §4.8, §7.4, §8.1, §8.2, §9.4, §9.6, §12.3, §14.5, §21.1, §28.2,
  §34, §55.2, §55.5, §55.7, §55.9; v0.2 §10.4; ADR-0012, ADR-0613, ADR-0653, ADR-0691
- Decided by: agent (autonomous)

## Context

v0.5 §9.6 is a MUST and nothing implemented it:

> `get process` in historical context MUST return the best supported process set for `T` and
> attach collection-level coverage. If the source cannot prove complete enumeration, the result
> MUST NOT imply that the returned rows are the complete process list.

Measured before the change, `ono -c 'at -0s; get process | count'` answered **354** — the live
process table, with today's `started` values — while `look` at the same coordinate answered from
the reconstruction. That is exactly §55.2's prohibited "rendering today's graph with an old
timestamp" and §55.9's "if `at -10m` changes the prompt but the data is present state, the
feature is invalid". `get process --at -2m` was refused outright with
`Ono-Sendai-E0202 type.unknown_field … has no option --at`, although §4.5 makes `--at` the
per-command spelling of the same coordinate.

The engine existed and had no caller. `ono_temporal_reconstruct::ReconstructedWorld::collection`
already answers §9.6 including `is_enumeration_proven`, and the spatial commands already read the
session coordinate through `crate::spatial::historical::active()`. What was missing was the seam
between the coordinate and `get`.

Where that seam goes is the decision. `get <target>` is not a command with an implementation of
its own: `ono_command::impls::producer::ProviderProducer` serves every `get` in the registry, and
it reaches the system through `ctx.providers().snapshot(&query)` (ADR-0012). The evaluator builds
that invocation in three places — the drained foreground segment, the streaming continuation and
the background job — and all three already resolve the coordinate through
`crate::temporal::invocation_context` for §4.7's read-only guard (ADR-0691), but the coordinate
they resolve reaches the *command table*, not the provider behind it.

## Decision

### 1. Every provider is registered behind one temporal seam

`crate::providers::registry_with_tables` wraps each registered provider in `TemporalProvider`,
which delegates every method of `ono_provider_api::Provider` untouched except `snapshot`. The
provider's id, targets, identity token, schemas, capabilities, availability and declared temporal
reach are the inner provider's, so `docs/contracts/providers/`, `spec-check` and the conformance
suites see the surface they saw before.

`TemporalProvider::snapshot` asks one question — where in time does *this query* evaluate — and
answers either from the inner provider (the present) or from the reconstruction (the past).

The wrapping happens once, over the assembled registry, rather than at each `register` call:
`ono_provider_linux::register_with_env` mounts several providers of its own, and a seam with a
hole in it is not one.

**Why the seam is at the provider rather than at the producer.** §4.5 forbids "a separate
historical code path", and the producer is one code path per *verb* while the provider is one per
*answer*. Teaching `ProviderProducer` about time would have put a historical branch in
`ono-command`, which is the library `ono-cli` is meant to compose — and §55.7 keeps the ledger,
the reconstruction and the causal engine out of the CLI integration crate, not the other way
round. Wrapping providers keeps the temporal decision in `ono-cli`, keeps the reconstruction in
`ono-temporal-reconstruct`, and gives the two spellings of the coordinate one seam to pass
through.

### 2. The coordinate comes from the same two places `at` does

`coordinate_of` reads, in order:

1. the query's `--at` option, resolved through `crate::temporal::coordinate::resolve` — the one
   function `at` itself calls, which parses the selector, resolves it against the session's zone
   and ledger, composes coverage and raises §12.3's refusals;
2. otherwise the session's coordinate, through `crate::spatial::historical::active()` — the
   evidence the temporal session installs, which is the single coordinate §4 gives a session.

Neither branch parses a selector or composes coverage here. An `--at` that reaches the provider
has already been resolved once by `crate::temporal::invocation_context` on the way in, so the
second resolution is the same function over the same state and cannot disagree with the first.

### 3. The rows are the reconstruction's, and nothing live is reachable from them

`reconstructed` takes a `Query`, an instant and a `LedgerRead`. There is no provider, no
`SpatialIndex` and no registry in scope, so §55.2's prohibited answer is unreachable rather than
merely avoided — the same property `crate::spatial::HistoricalWorld` has and for the same reason.

The target word is joined to spatial types through `ono_spatial_core::types_of_target`, which is
already the only join between the v0.2 target vocabulary and the spatial one. The members of
`ReconstructedWorld::collection` are the rows; the query's selectors narrow them exactly as they
narrow a live set, so `get process 4419` means one thing at both coordinates (§28.2).

### 4. §9.6's collection coverage rides in `ono.temporal.collection`

ADR-0613 fixed where §9.4's per-object temporal metadata rides: the reserved `ono.temporal`
extension key, because this tree namespaces record extensions with dotted publisher-qualified
keys and has no `_temporal` field anywhere. §9.6's *collection*-level coverage is one more member
of that same map rather than a second attachment point:

```text
ono.temporal.collection {
    object_type          Process
    capability           process.existence
    completeness         complete | partial | point_sample | unknown | …
    enumeration_proven   Bool
    members              Int
    gaps                 List<ono.temporal-gap/1>
}
```

`enumeration_proven` is `ReconstructedCollection::is_enumeration_proven` carried verbatim. It is
a stated fact on the answer rather than something a reader infers from a row count, which is what
§9.6's second sentence asks for: a result that says `enumeration_proven: false` has told its
reader that the rows are not the whole list.

It is on every row rather than on a wrapper record because `get process` is a **stream** of
`ono.process/1` (§28.2 forbids a `HistoricalProcess` type and pipeline compatibility forbids a
wrapper). A stream has no header, so the collection's own claim travels with each member of it.

### 5. An enumeration nobody can prove and nobody observed is refused, not emptied

Three answers, and which one applies is decided by evidence rather than by row count:

| Evidence | Answer |
|---|---|
| the target names no spatial type — `package`, `env`, `dns`, `log` | `temporal.unsupported_source` (E1310) naming the target |
| the target is filesystem structure and nothing carries it | `temporal.unsupported_source`, worded as §14.5 words it |
| rows found, or absence provable | the rows, with the coverage of §4 above |
| no rows and no proof of absence | `temporal.not_recorded` (E1302), listing what each source can reach |

The last row is the one §55.5 is about. "There were no sockets at 12:17" is a claim §7.4 and §8.2
allow only where a source had coverage capable of proving it; an empty stream would make that
claim silently. So the refusal is `crate::temporal::coordinate::availability`'s source list under
`ono_temporal_core::error::not_recorded` — the same words `at -3d` already refuses with, because
it is the same question one level down.

Complete coverage that proves the class was empty still answers with an empty stream. Proven
absence is a real answer, and refusing it would be as dishonest as inventing it.

### 6. `find` is enumeration too, and rides the same seam

`ono.file.find` reaches the system through the same `ProviderProducer` `get` does, and it showed
the same defect: at `at -2s` it answered from the live filesystem, `accessed` and `modified`
included, under a `[PAST?]` prompt. ADR-0653 already rules that a current reading is not one of
§14.5's four supports for historical path structure, so the seam covers `find` as well as `get` —
both enumerate a class of objects, which is what §9.6 is about.

`resolve dns`, `test port`, `test host` and `tail journal` reach `snapshot` too and are **not**
intercepted. They probe a live system rather than enumerating a state, and what governs them is
§4.8's present-only rule, whose refusal is `temporal.present_only` and whose dispatch is not this
one. Widening the seam to them would answer a probe with a reconstruction, which is a different
mistake from the one being fixed.

### 7. `--at` is declared exactly where a spatial type exists

§4.5 says read-only commands "that support historical evaluation" SHOULD accept `--at`. The
mechanical, greppable reading of that is: the `get` commands whose target
`ono_spatial_core::types_of_target` maps to at least one `SpatialType`, because that is exactly
the set the reconstruction can produce rows for. Eighteen commands:

`get process`, `get job`, `get service`, `get container`, `get socket`, `get connection`,
`get interface`, `get route`, `get neighbor`, `get filesystem`, `get mount`, `get device`,
`get file`, `get dir`, `get user`, `get group`, `get session`, `get host` — and `find file`, for
the same reason.

`get package`, `get env`, `get dns`, `get log`, `get plugin` and the rest declare no `--at`: their
objects are values the typed shell holds and §7 gives them no place, so there is nothing for a
reconstruction to be a reconstruction *of*. Asked at a session coordinate in the past they are
refused rather than answered live, which is the same rule stated from the other side.

`get file` and `get dir` declare `--at` and answer §14.5's refusal. That is deliberate: §4.5's
"supports historical evaluation" includes evaluating and saying, with the reason, that this
source cannot carry it — which is more useful than an unknown-option error and more honest than
listing today's directory.

## Consequences

- `at -0s; get process | count` no longer answers the live table. Where the session's evidence
  supports a process set it answers that set; where it does not it refuses, and in neither case
  does it consult `/proc`.
- `crates/ono-cli/tests/temporal_collections.rs` is the exit test. Ten cases: the reconstructed
  set excludes a process running now that nothing recorded (§55.2, §55.9); a reconstructed row is
  still an `ono.process/1` a `select` reads (§28.2); the collection states a proven and an
  unproven enumeration (§9.6, §8.2); an uncovered class and an unreconstructable target are
  refused with their own codes (§12.3, §34, §55.5); a historical `find file` is refused with
  §14.5's reason rather than read from the live tree (ADR-0653); `--at` answers what `at` answers
  and refuses what `at` refuses, without moving the session (§4.5).
- The interception covers `get` and `find`. `resolve dns`, `test port`, `test host` and
  `tail journal` are left as they were, for the reason in §6 above. `watch`/`subscribe` at a
  historical coordinate is the same open finding: a subscription to the past is not a thing, and
  the honest refusal for it is `temporal.present_only`, which nothing raises yet.
- Providers a KUANG/11 package mounts after the registry is built are registered directly by
  `crate::session::Session::mount_loaded_packages` and are therefore **not** behind the seam. A
  contributed target answered at a historical coordinate still answers live. Closing it needs a
  change in `session.rs`, which this increment did not own.
- The `ono.temporal.collection` key is reserved beside `ono.temporal` itself. Nothing else may
  claim it.
- A second resolution of `--at` happens inside the provider, after the evaluator's. Both call
  `crate::temporal::coordinate::resolve`, so they cannot disagree; the cost is one selector parse
  and one coverage composition per historical `get`.

## Alternatives considered

- **Teach `ProviderProducer` about the invocation's temporal context.** The natural place, and it
  is in `ono-command`. It would put the reconstruction call in the library that serves every
  `get`, and it would have to be repeated for `inspect`, `trace` and `watch` — one historical
  branch per verb, which is the "separate historical code path" §4.5 forbids in the plural.
- **Register one historical provider first, claiming every target.** `ProviderRegistry` picks the
  first *available* provider for a target, and availability is asked without a query. Such a
  provider could read the session coordinate but never a per-command `--at`, so `get process --at
  -1h` would have answered from the present while `at -1h; get process` answered from the past —
  two spellings, two answers, which is precisely what §4.5 prohibits.
- **Answer historically and attach the coverage only as a diagnostic on the stream's failure
  channel.** §34 is explicit that "partial reconstruction is normally a successful result carrying
  partial coverage, not an error", and a failure row would have made `get process --at -1h` fail
  the run (§16.5).
- **A `temporal` record field on all twenty-three reconstructable schemas.** Tried and reverted in
  ADR-0613 for reasons that have not changed; the collection member inherits that decision rather
  than reopening it.
