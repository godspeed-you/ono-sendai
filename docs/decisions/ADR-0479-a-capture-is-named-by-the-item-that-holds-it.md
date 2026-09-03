# ADR-0479: A capture is named by the item that holds it

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §22.1, §22.4, §23.1, §24, §26.1, §65.7, Appendix E; AGENTS.md §7, §11
- Decided by: agent (autonomous)

## Context

v0.4.1 §26.1 asks for an inventory: *"The implementation MUST inventory every `Vec<Value>` or
equivalent capture in evaluator execution paths and classify it as: semantic materialization /
implementation convenience / history/cache."* §65.7 says why it is worth committing rather than
reviewing once — *"replacing a foreground `Vec` with an unbounded background queue is not a
streaming fix and is forbidden"* — and issue #78 states the exit condition: the inventory is a
committed artifact, each entry carries its class, and **the gate fails on an unclassified
capture**.

An inventory nothing checks decays in one increment. So the question was not what to write down
but what a machine can compare it against, given that the thing being classified — "a capture" —
has no syntactic form of its own. Three candidates were available: a line number, a marker comment
at each site, or a name the source already carries.

ADR-0455 had already settled the neighbouring question for *commands*: Appendix E's classification
is data in `docs/spec/hardening/streaming_classification.yaml`, compared by `spec-check` against
the `execution:` field of every command contract. That file answers "what may this operation do
to a stream?". §26.1 asks a different question — "what does the evaluator hold, and why?" — about
code that has no published contract at all.

## Decision

**The inventory lives in `docs/spec/hardening/streaming.yaml`, and a capture is identified by the
item that holds it: a function, a struct or an enum, named as the source names it.**

1. **The site is an item, not a line.** `crates/ono-cli/src/eval.rs` plus `run_each_block` is the
   key. Moving a function does not invalidate its entry; renaming one does, which is the right way
   round — a rename is a change a reader should be told about, a reflow is not.

2. **A capture is three markers.** A line of evaluator source containing `Vec<Value>`,
   `begin_capture(` or `end_capture(`, with comments and string literals blanked first. This is
   deliberately syntactic and deliberately over-inclusive: a parameter that passes a collection
   through, a struct field that stores one and a function that opens a capture buffer all count,
   because each of them is a place where pipeline values stop being in flight. Where a capture is
   real but untyped, the fix is to write the type — `let mut values: Vec<Value> = Vec::new()` —
   rather than to teach the scanner to guess.

3. **`xtask::scan::check_evaluator_captures` fails in both directions.** An unclassified site is a
   problem, and so is an entry no site answers to. The second half is what makes the artifact
   survive this tranche: removing the `each` capture removes its entry in the same increment, so
   the file can never become a description of code that used to exist.

4. **`implementation_convenience` requires an ADR field.** §26.1 permits that class only *"removed
   or bounded and justified by ADR"*, so an entry of that class with no `adr:` is refused by the
   gate. The three classes are closed: a fourth invented in passing is a problem, because a class
   the specification does not define is a classification that decides nothing.

5. **Scope is five files, stated in the artifact.** `eval.rs`, `native.rs`, `session.rs`,
   `report.rs` and `view.rs` — where a pipeline is assembled, driven, drained and retained. A
   *command* that materializes is classified by Appendix E instead, and the two files are not
   merged: one classifies published operations by their contract, the other classifies evaluator
   structure by what it holds.

## Consequences

Easy: `each` and function streaming (issues #75, #79) now have a place to record what they removed
and what they kept, and the per-invocation capture that §25.4 permits has somewhere to be declared
as bounded rather than merely small. A later reader can tell, for every collection in the
evaluator, whether it is intentional — which was the whole point of §26.1.

Hard: the marker set is syntactic, so it reports honest false positives — `seed_bytes(values:
Vec<Value>)` holds a collection for exactly as long as it takes to serialise it. Those are cheap
to classify and expensive to omit, so the scan does not try to be clever about them. And a capture
that never mentions `Vec<Value>` — a `BTreeMap` keyed model, a channel with unbounded capacity —
is not found by this rule. The bounded-channel requirement of §28.1 is what covers the second
case, and it has its own proof.

Encoded by: `xtask/tests/scan.rs::should_report_an_evaluator_capture_the_streaming_inventory_does_not_classify`,
`::should_report_an_inventory_entry_whose_capture_is_no_longer_in_the_evaluator`,
`::should_report_an_implementation_convenience_capture_that_no_decision_record_justifies`,
`::should_report_a_capture_whose_class_is_not_one_the_specification_defines`,
`::should_accept_an_evaluator_capture_the_inventory_classifies` and
`::should_report_this_repository_as_classifying_every_evaluator_capture`.

## Alternatives considered

**A marker comment at each capture site** — `// CAPTURE: semantic` beside every collection. It
survives renames and needs no name resolution, and it puts the classification where the code is.
Rejected because the classification then lives in twenty places and can be read only by reading
all of them; §26.1 asks for an *inventory*, and the value of an inventory is that it is one
document. The comment form also degrades exactly like a `TODO`: it is written once and never read.

**Line numbers as the key.** Every unrelated edit above a capture invalidates the file, which
makes it noise, which makes it ignored.

**Extending `streaming_classification.yaml` with the evaluator sites.** One file, one gate. But
Appendix E's classes are about what an operation may do to a stream, and the §26.1 classes are
about why the evaluator is holding one; a `history_cache` is not an Appendix E class and
`prefix` is not a §26.1 class. Sharing a file would have forced one of the two vocabularies to
bend, and ADR-0455's registry is load-bearing for `explain` (§22.4).

**Counting captures rather than naming them** — a ceiling the gate enforces. It fails the actual
requirement: §26.1 wants each one classified, and a number cannot say which of them is deliberate.
