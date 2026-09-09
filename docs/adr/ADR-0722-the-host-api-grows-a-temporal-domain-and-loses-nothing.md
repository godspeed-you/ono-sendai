# ADR-0722: The host API grows a temporal domain and loses nothing

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §30.7, §37.2, §37.4, §37.5, §37.6; v0.2 §31.5, §31.12, §31.22, §31.23, §31.27,
  §31.62, §31.63; ADR-0022, ADR-0588, ADR-0600
- Decided by: agent (autonomous)

## Context

v0.5 §30.7 declares six temporal capabilities and one sentence about them: "A plugin with current
object read permission does not automatically receive historical access." §37.2 lists seven kinds
of thing a package may contribute about time, of which two need a place in the manifest and on the
wire — a temporal source and a causal rule — and §37.6 lets a package draw a timeline.

The capabilities had landed. Nothing a package could call reached them, and there was nowhere for a
package to declare a history provider before it ran.

## Decision

### 1. A domain of its own, and one call per capability

`protocol.v1.yaml` gains a `temporal` domain at `11.3` with six calls: `temporal.context`,
`temporal.query`, `temporal.evidence`, `temporal.contribute.events`,
`temporal.contribute.causality` and `temporal.recorder`. Each carries exactly one of §30.7's
capabilities.

The separation from `objects` is the point rather than tidiness. §30.7's sentence is true here by
construction: `object.read` reaches `objects.query`, `temporal.read.history` reaches
`temporal.query`, and there is no path from one to the other. It is also separate from `history`,
which is the shell's *command* history of v0.2 §31.19 and a different thing entirely (v0.5 §29.2).

`HostServices` gains the six as **defaulted** methods answering `provider.unavailable`. A host that
keeps no ledger says so; every existing implementation, including the shell's, keeps compiling and
keeps working.

### 2. `HOST_API` moves to `11.3`

`11.2` was `11.1` plus the permission layer (ADR-0600 §3). `11.3` is `11.2` plus this domain, the
two contribution shapes and the `Timeline` view component. Every addition is additive: a package
declaring `>=11.1 <12` still loads, and a package that asks for none of it is unaffected. §31.62
requires the version dimensions to move independently, and this moves one of them.

### 3. A package declares a temporal source and a causal rule the way it declares everything else

`ContributionSet` gains `temporal_sources` and `causal_rules`, both skipped when empty, so a
package that contributes nothing about time sends the frame it always sent. `ContributionPaths` and
the `deny_unknown_fields` `RawContributions` gain the matching manifest keys, so the two documents a
package writes — one on disk, one across the handshake — keep their one shape.

`TemporalSourceContribution` carries what §37.5 requires a package to be responsible for before it
runs: the canonical schema it maps into, the canonical event kinds it produces, whether its answer
ends by itself, and its coverage in prose. Boundedness is `Answer`, reused from ADR-0588 for the
same reason it existed there — the host cannot find out by reading, because a package that has not
sent a record and one that never will look identical from outside.

`CausalRuleContribution` carries §15.8's seven things reduced to what a package can state: the
namespaced id, the relation class, the strength, the inputs and the identity constraints. All three
of the id, the class and the strength are validated at load against `ono-temporal-core`'s own
vocabularies, so a rule claiming `ono.*`, emitting a sixth relation class or naming a sixth
strength is `package.invalid` before any package code runs.

### 4. `VIEW_COMPONENTS` gains `Timeline`

Thirteen becomes fourteen, and the length stays in the type. §37.6 lets a plugin contribute an
alternate view that "MUST consume canonical temporal schemas" and "cannot create causality that is
absent from its input"; without a component for a timeline, consuming a canonical timeline would
mean redrawing it out of `Table` rows, and the constraint would have nothing to attach to.

## Consequences

`crates/ono-kuang-protocol/tests/temporal_contributions.rs` holds the manifest keys, the wire
shapes, the six call ids, the component count and the version. `crates/ono-kuang-sdk/tests/
conformance.rs` runs all six against the example plugin under the deterministic test host, whose
fixed clock makes a temporal assertion exact.

`docs/contracts/kuang/contributions.v1.yaml` documents both new contribution shapes;
`manifest.v1.yaml` documents both new paths, and `xtask spec-check`'s manifest check compares the
contract's closed sections against the parser in both directions.

## Alternatives considered

**Extending `objects.query` with a time range.** Rejected outright: it would make historical access
a parameter of a call a package already holds, which is precisely what §30.7 forbids.

**One `temporal.contribute` call taking a discriminated payload.** Rejected: two capabilities with
two consent classes and two validation paths behind one call id would make a grant and a call
different questions, and the audit trail would record one action for two decisions.
