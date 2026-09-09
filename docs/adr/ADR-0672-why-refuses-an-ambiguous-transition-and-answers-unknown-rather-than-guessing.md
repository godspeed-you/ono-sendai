# ADR-0672: `why` refuses an ambiguous transition and answers unknown rather than guessing

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §1.4, §15.7, §16.2, §16.3, §16.6, §16.7, §34 (E1306), §35.5; ADR-0610
- Decided by: agent (autonomous)

## Context

v0.5 §16.3 says what `why service nginx` selects — *"the most recent causally explainable notable
state transition relevant to the service at the active temporal coordinate"* — and what it does
when several qualify: *"Ono MUST refuse ambiguity and list event references rather than choose
arbitrarily."* §15.7 and §16.6 say that a transition with no cause is still a complete answer.

Two questions are left open. First, what "causally explainable" means: read as "has a causal
edge", §16.6's own worked example becomes unreachable, because that example is a failure with no
cause and a config change beside it. Second, what "equally relevant" means, which decides how
often E1306 fires.

## Decision

### 1. Selection, then explanation, in that order

`why <target>` selects the most recent **notable state transition** about the subject at or before
the active coordinate — an `object.appeared`, `object.changed`, `object.disappeared`,
`action.failed` or `action.completed` carrying at least one evidence id — and then explains it.
Whether a registered rule finds a cause for it is the answer, never the filter. This is the
reading that keeps §16.6 reachable, and it is what makes "unknown cause" the ordinary result of a
conservative rule set rather than a special case.

`why field <name>` selects the most recent change to that field whose `ChangeCertainty` is
`observed` or `derived`. §6.2 keeps the reason a side is null with the change: an `unknown`
certainty means a side has no evidence and an `inferred` one was filled from an interval by
reconstruction (§9.2), so neither is a change a source stated and neither is what §16.2's "most
recent supported change" asks for.

`why event <ref>` explains exactly the event named, at or before the coordinate. A reference to
something later is `temporal.not_recorded`: §4.7 makes a historical session read-only about the
past, and answering about the future of the coordinate would be answering a different question.

### 2. Equally relevant means the same presentation instant

Where more than one selected candidate shares the latest presentation instant, `why` raises
`temporal.ambiguous_event` (`Ono-Sendai-E1306`, ADR-0610) with every candidate's reference in the
`candidates` metadata. Relevance ranking exists elsewhere in this crate for timelines; using it to
break a tie here would be choosing arbitrarily with extra steps, which is the sentence §16.3
forbids.

### 3. Unknown cause is a success, and the arrays never merge

The answer is one `CausalExplanation` with §16.4's fields. `cause` is `None` for §15.7's unknown
cause, and the explanation still carries the chain (empty), the correlations, the preceding
events, the gaps and the coverage. `causal_chain`, `correlations` and `preceding` are three
separate fields, as §35.5 requires, and no code path moves an entry between them.

`preceding` is filled from `happens_before` alone, so an event that merely *looks* earlier by wall
clock is absent from the answer entirely (§26.1, §55.6). Its entries carry no rule id, because
order comes from the ordering model and §15.8's registry is about causal language.

### 4. Depth bounds the answer and not the graph

The walk backwards along causal edges stops at `depth`, defaulting to three (§16.7,
`temporal.timeline.default_depth`); `--depth N` widens it. The candidate set is bounded by
`temporal.why.max_candidates`, 1000 by default, taking the most recent events at or before the
coordinate. The graph the engine can derive holds whatever the evidence supports; the typed answer
carries what was asked for.

Where several edges reach the explained event at depth one, `cause` is the strongest, then the
more direct claim — §15.2's `caused_by` before §15.3's `triggered_by` — then the lower rule id.
Every part of that order is a fact about the edges, so the choice does not depend on the order the
rules happened to run in.

## Consequences

- §48.5 scenario 26 and release criterion 14 hold: `cause: null` with evidence, correlations and
  gaps, and no error code.
- E1306 from `why` is rare in practice and deterministic: it needs two recorded transitions of the
  same subject at the same instant.
- A question about a subject with nothing recorded answers successfully with no explained event
  and a sentence saying so, rather than raising an error the specification does not define.
- The renderer receives a value in which causal and correlated are already separate, so §15.6's
  wording prohibition is a property of the input rather than a rule the renderer must remember.

## Alternatives considered

**Read "causally explainable" as "has a causal edge".** Rejected: it makes §16.6 unreachable and
turns the honest answer into an error.

**Break ties by relevance ranking.** Rejected by §16.3's own sentence.

**Raise an error when nothing is recorded about the subject.** Rejected: §1.4 prefers "I do not
know" to a confident fiction, and an empty window is a fact about coverage rather than a bad
question. The coverage summary and the gaps that come with the answer say why.
