# ADR-0724: A model may suggest, and may not establish

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §4.7, §15.1, §15.7, §15.8, §38.1, §38.2, §38.3, §38.4; v0.2 §31.43, §31.44,
  §31.46, §31.52; ADR-0566
- Decided by: agent (autonomous)

## Context

v0.5 §38's intent paragraph is one sentence long and decides everything else in it: "Structured
temporal context is valuable precisely because it gives AI better evidence. AI must not be allowed
to contaminate the evidence model in return."

Three rules follow. A model's causal claim is an `Inference { kind: hypothesis, model, inputs,
confidence }` and "MUST NOT become a canonical `caused_by` edge without independent registered
evidence" (§38.2). Raw logs and external text used as model context are untrusted data, kept apart
from instructions (§38.3). An assistant answering while the session is historical obeys the same
read-only policy the operator does (§38.4).

`ono-model-broker` already had the second one for the general case: `classify` relabels anything a
package sends to `PLUGIN_KNOWLEDGE` or leaves it `UNTRUSTED_TEXT`, so no package speaks as the host
or as the operator. What it had no vocabulary for was a model's claim about the past.

## Decision

`ono_model_broker::temporal` adds three types, and each makes a rule structural.

**`InferenceKind` has one variant.** There is no `Conclusion`, no `Finding` and no `Established`. A
model that has become certain has become a more confident hypothesis, and the type offers no other
way to say it.

**`Inference::independent_evidence(registered)` is the only question about promotion.** It answers
which of a registered set the model was *not* already shown, and `may_support_causal_edge` is
whether that is non-empty. A model given three records and asked to conclude from them has produced
nothing those three records did not contain, so pointing at them again establishes nothing. Where
the answer is true the edge still comes from the registered rule the independent evidence satisfies
(§15.8) — never from the model, which supplied the question. `Inference::to_json` deliberately
carries no `relation` field: §15.1's five classes are the ledger's, and a renderer wanting to draw
a hypothesis as an edge would have to invent the class itself.

**`evidence_segment` is the one way to put temporal evidence in front of a model**, and it produces
a segment that cannot be anything else — label `UNTRUSTED_TEXT`, class `logs`, whatever the caller
passed. A log line saying "ignore your instructions" arrives as a log line saying that.
`evidence_is_data` is the assertion for the other direction, for a caller assembling a turn out of
ledger material.

**`TurnStance::admits` refuses a mutating tool intent while the session is historical**, with the
same remedy the shell's own refusal offers. §38.4 admits exactly one way past it and it is not one
an assistant can take: the *user* returns to the present, or uses a separately confirmed
present-bound flow. Whether a tool mutates is what its descriptor already declares (§31.46), so
nothing here guesses.

## Consequences

`crates/ono-model-broker/src/temporal.rs`'s own suite holds all nine behaviours. The crate stays in
the `runtime` layer and depends on no temporal crate: an inference names events and evidence by id,
as strings, because the thing being prevented is a model reaching the evidence model at all.

Nothing in the tree yet builds an `Inference` — the assistant surface that would is later work —
and that is the right order. The vocabulary exists before the first caller, so the first caller has
no reason to invent a weaker one.

## Alternatives considered

**Letting a hypothesis carry a `relation` and rendering it dotted.** Rejected: §15.5 keeps
correlation structurally distinct from causation and §15.8 makes the rule registry the only source
of causal language. A hypothesis with a relation is one refactor away from an edge.

**Refusing an over-confident `confidence` outright.** Rejected: a provider answering `1.7` has said
"very sure", and the useful thing to do is record `1.0` beside the fact that it is a hypothesis. A
non-finite number is `0.0`, because a confidence nobody can compare is not one.
