# ADR-0671: A causal link comes through one door and the door demands evidence

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §2 (invariants 7, 8), §7.2, §15.2, §15.5, §15.8, §37.4, §48.5 scenario 27, §55.3
- Decided by: agent (autonomous)

## Context

v0.5 §15.2 ends with five words the whole causal model rests on: *"Temporal proximity is
insufficient."* §55.3 names the failure they exist against: *"A renderer or AI that says 'the
config change caused the outage' because it happened first violates the core contract."* §48.5
scenario 27 turns it into an acceptance criterion: every causal edge names its rule, its source
and its evidence.

`ono_temporal_core::CausalLink` is a plain struct with public fields, because §39 makes
`ono-temporal-core` the vocabulary crate that the ledger, the reconstruction engine and the
renderer all deserialise into. Any of those crates can therefore build a `CausalLink` with an
empty evidence list. What must be impossible is for the *causal engine* to produce one, because
the engine is where a causal claim is created rather than transported.

## Decision

### 1. A rule returns a `CausalFinding`, whose only constructor validates

`CausalRule::evaluate` returns `Vec<CausalFinding>`. `CausalFinding` wraps a private `CausalLink`
and the only way to make one is `CausalFinding::emit(rule, relation, cause, effect, evidence,
strength, source)`, which refuses with `LinkRefused::NoEvidence` for an empty evidence list and
`LinkRefused::SelfLink` for an edge from an event to itself. A rule that has nothing but two
timestamps has no expression that produces a link.

### 2. The engine re-checks every finding against the rule's own registry row

`CausalEngine::links` drops a link when any of the following holds:

- its rule id or relation differs from what the rule described (§15.8);
- an evidence id does not resolve to a record in the `CausalContext`;
- its strength exceeds the weakest evidence behind it — §7.2 has `weakest_of` and no counterpart,
  and this is where that sentence becomes a filter;
- its strength is below the minimum the rule's row requires for the kinds at its two ends;
- its source is not one the row's `provider_source_constraints` admit;
- either end is of a kind the row does not declare;
- it is a non-causal relation carrying a strength above `correlated` (§15.5, §7.2).

Two walls rather than one, because the first is a property of a type inside this crate and the
second is a property of the contract everybody can read.

### 3. No built-in rule has a time-only join

The seven causal rules join on an identity a source published: a systemd job path, an Ono
`ActionId` propagated into a provider transaction, a kernel-reported parent, a cgroup membership,
an explicit provider causal token, or the shell's own record of the process it forked. The three
correlation rules use a window, emit `correlated_with`, and each additionally demands a structural
association — a spatial relation, a shared scope, a connection identity — so that a window alone
never produces an edge either.

`crates/ono-temporal-query/tests/correlation.rs` sweeps every pair of event kinds the rules read,
crossed with every §7.1 source, every evidence strength, both subject-identity outcomes, the
systemd job subtypes and two plausible gaps — around ten thousand worlds — and asserts that no
rule emits a causal class for any of them. The same test then rebuilds each world with one
difference, a transaction token both events carry, and asserts that causal links do appear. A
silence that cannot be broken proves nothing; this one can.

## Consequences

- §48.5 scenario 27 holds by construction: an edge with no rule, no source or no evidence cannot
  leave the engine.
- A plugin rule (§37.4) is bound by the same two walls and additionally capped at `asserted` by
  `ono.provider-causal-token`, which weakens its links whenever the source `is_plugin()`.
- A rule cannot express a heuristic that "usually works". Where the evidence is missing the answer
  is `cause: unknown`, which §15.7 makes a complete answer.
- `CausalLink` keeps its public fields for the ledger and the renderer, so nothing outside this
  crate had to change. The guarantee is at the engine boundary, which is stated here so a later
  reader does not mistake the struct's shape for permission.

## Alternatives considered

**Make `CausalLink` itself unconstructable in `ono-temporal-core`.** Rejected for now: the ledger
decodes stored links out of CBOR rows and the renderer decodes them out of `RecordValue`s, so a
private constructor there would need a second decode-only door and would move the guarantee
without strengthening it. If `ono-temporal-core` later grows a validated constructor, this ADR is
the thing to supersede.

**Validate only in the engine.** Rejected: a rule that returns junk would then be a bug found by
the engine's filter rather than a program that does not compile.

**Assert the invariant in a review checklist.** Rejected outright. §55.3 describes a failure that
looks reasonable while it happens, and a checklist is exactly the control that fails against it.
