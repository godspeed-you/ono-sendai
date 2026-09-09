# ADR-0673: An `Inference` belongs to the model broker and the causal engine refuses it structurally

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §7.1, §7.2, §16.1, §38.1, §38.2, §41, §55.8; `docs/ACCEPTANCE.md` §4.11.6
- Decided by: agent (autonomous)

## Context

v0.5 §38.2 requires a model's hypothesis to be represented as

```text
Inference { kind: hypothesis, model, inputs, confidence }
```

and states that it *"MUST NOT become a canonical `caused_by` edge without independent registered
evidence"*. §41 adds that *"AI-generated reasoning MUST NOT implement this trait in core v0.5"*,
and §16.1 and §55.8 require `why` to work with no model configured at all.

The open question is where the type lives. `docs/ACCEPTANCE.md` §4.11.6 names
`crates/ono-model-broker/tests/inference.rs` as the exit test, which answers it, but the causal
engine still has to be able to say what it does about the thing.

## Decision

### 1. `Inference` lives in `ono-model-broker`

It is a value the broker produces and attributes to a model, with the model id, the inputs it was
given and a confidence. `ono-temporal-query` does not define it, does not import it and has no
type that could hold one. `ono-model-broker` already exists in the workspace and already owns
model attribution, so putting the type there keeps the temporal crates free of a model dependency
— which is what §16.1 and §55.8 are asking for structurally rather than by policy.

### 2. The refusal on this side is the shape of the types, and it is tested here

Three facts, together, make an inference unable to become a causal edge:

- §7.1's nine evidence source classes are closed and `EvidenceSource::parse` refuses anything
  else, so a model cannot name itself as a source. `model:gpt`, `ai.hypothesis` and `inference`
  all parse to `None`, and `crates/ono-temporal-query/tests/causality.rs` asserts it.
- `EvidenceClaim` has no hypothesis variant. The seven claims are all statements a source made
  about the world at an instant or over an interval.
- Every link the engine emits carries evidence ids, and `CausalEngine::links` drops a link whose
  evidence does not resolve to an `Evidence` record in the `CausalContext` (ADR-0671). A claim
  with no evidence record behind it is not a claim this engine can carry.

So the refusal needs no check that could be forgotten: there is no value of any type in this crate
that a hypothesis could be encoded as.

### 3. §7.2 stays intact in the same direction

Even where a model's hypothesis coincides with something a rule found, nothing raises the
resulting strength. `EvidenceStrength` has `weakest_of` and no counterpart, and §38.1 makes AI a
consumer of causal explanations rather than a contributor to them.

## Consequences

- `why` is answerable with no model configured, which §55.8 makes a design criterion rather than a
  convenience, and `crates/ono-temporal-query` has no dependency on `ono-model-broker`.
- The broker may consume `CausalExplanation` values and produce `Inference` values beside them.
  Presenting the two together is a renderer's problem, and §15.6's wording rules apply to whatever
  it writes about a non-causal association.
- Agent K, or whoever implements the broker, owns `Inference` and the acceptance case
  `268-kuang-causality-is-bounded`. This ADR records that the causal engine's half is done and
  what it consists of.

## Alternatives considered

**Define `Inference` in `ono-temporal-core` beside `Evidence`.** Rejected: it would put a model
concept in the crate §39 reserves for the temporal vocabulary, and a type that sits beside
`Evidence` invites a future rule to accept it.

**Accept an `Inference` in `CausalContext` and refuse it at emit time.** Rejected: a refusal that
depends on a check is a refusal somebody can remove. Refusing by having nowhere to put the value
survives a careless edit.
