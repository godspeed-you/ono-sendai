# ADR-0670: A causal rule declares its seven fields once and behaves the way it described itself

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §15.8, §26.3, §36.4, §41, §47.1, §47.2; ADR-0627
- Decided by: agent (autonomous)

## Context

v0.5 §41 sketches the rule interface:

```rust
trait CausalRule {
    fn id(&self) -> CausalRuleId;
    fn relation(&self) -> CausalRelationClass;
    fn required_evidence(&self) -> EvidenceRequirements;
    fn evaluate(&self, candidate: &EventSet, context: &CausalContext) -> Vec<CausalLink>;
}
```

and adds two obligations: *"Rules MUST be deterministic for the same canonical input"* and
*"AI-generated reasoning MUST NOT implement this trait in core v0.5."* §15.8 separately requires
seven fields of every rule that emits a causal relation, and ADR-0627 put those seven fields into
`docs/contracts/temporal/causality.yaml` as prose rows. Nothing yet said how the code and the row
stay the same thing.

Three accessors on the trait and a row in a YAML file are two copies of one description. A rule
whose `relation()` says `caused_by` while its registry row says `triggered_by` is a rule an
explanation cannot be audited against, and nothing in the shape of §41's trait prevents it.

## Decision

### 1. One method carries the description; the rest are derived

`CausalRule::describe()` returns a `RuleDescription` with exactly §15.8's seven fields —
`rule_id`, `input_event_kinds`, `required_evidence`, `identity_constraints`, `time_constraints`,
`output_relation`, `source_constraints` — plus `window`, which a correlation rule declares and a
causal rule leaves `None` (§15.5). §41's `id()`, `relation()` and `required_evidence()` are
defaulted methods over `describe()`, so a rule states its seven fields once and cannot describe
itself differently from how it behaves.

`crates/ono-temporal-query/tests/causality.rs` reads `causality.yaml` and holds every row against
the matching `describe()` in both directions: the ids are one set, the output relation matches,
the input kinds match, every declared `required_evidence_strengths` entry matches, and every
`provider_source_constraints` entry matches. A rule that drifts from its row fails that test
before it reaches the gate.

### 2. Determinism is a property of the input, enforced where the input is made

`CausalEngine::links` puts the candidate events into `presentation_order` before it calls any
rule, so *canonical input* is a fact about the call rather than a hope about the caller. Inside a
rule, every collection walked is ordered: the events in presentation order, an event's evidence in
the order the event lists it, the context's evidence records by identity. `CausalFinding::emit`
sorts and deduplicates the evidence list, so the same firing produces the same link identity
whatever order the rule visited its evidence in, and `CausalLinkId::of` digests the rule, the
relation and both ends, so re-deriving causality over a window produces the same edge set rather
than more edges.

§26.3 makes presentation order a display convention rather than an ordering claim, and no rule
reads it as one: the only orderings a rule acts on come from `happens_before`, from a provider's
own sequence, or from an interval a source published.

### 3. Nothing that is not evidence reaches a rule

`CausalContext` gives a rule evidence records, the temporal capabilities each source advertises
and the composed coverage. It has no clock (§39.2), no ledger and no network. §41's prohibition on
AI-generated reasoning therefore needs no runtime check on this side: a model's output is an
`Inference` (§38.2, ADR-0673), an `Inference` is not an `Evidence`, and the nine §7.1 source
classes are closed, so a model cannot name itself as a source either.

## Consequences

- A new built-in rule is one `describe()` and one `evaluate()`, and the registry test tells the
  author immediately whether the row and the code agree.
- `inspect` and `why` can show a rule's seven fields from the engine itself rather than by
  re-reading a YAML file at runtime.
- `describe()` allocates on each call. It is called once per rule per `links()` invocation and
  once per rule for inspection, which is far below §32.3's 200 ms budget for `why`.
- The rules are pure functions of `(EventSet, CausalContext)`, so the falsification suite of §51's
  TEST-006 can sweep thousands of worlds in one test with no fixture beyond values.

## Alternatives considered

**Keep §41's three accessors as the primary methods and add a separate description.** Rejected:
that is the two-copies problem the decision exists to remove.

**Generate the rules from `causality.yaml` at build time.** Rejected: `identity_constraints` is
prose over spatial identities, cgroups and provider tokens (ADR-0627), and generating an
implementation from prose would mean inventing a join language now — the speculative generality
AGENTS.md §4 forbids. The test that compares the two is the cheaper half of the same guarantee.

**Trust rules to be deterministic and assert it in a test.** Kept as well, and made unnecessary as
the first line of defence: canonicalising the input at the call site means a rule cannot observe
insertion order to begin with.
