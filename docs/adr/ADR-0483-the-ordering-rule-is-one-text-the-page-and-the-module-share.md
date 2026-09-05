# ADR-0483: The ordering rule is one text the page and the module share

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §27.1, §27.2, §27.3, §27.4, Appendix E; spec §16.5, §35.5; ADR-0014,
  ADR-0221, ADR-0455 (Appendix E as data), ADR-0460
- Decided by: agent (autonomous)

## Context

v0.4.1 §27 does two things. It restates a guarantee that already held — per-channel order is total
(§27.1) — and it makes explicit one that had never been written down: **there is no cross-kind
order.**

> `StreamEvent` does not promise a total temporal ordering between independently produced value
> and partial-error channels unless a producer explicitly serializes them through one event
> source.

`ValueStream` merges two `mpsc` channels with a `tokio::select!`, so what a consumer observes
between a value and a partial failure is whatever was ready. Nobody had said so, which meant
nobody could be wrong about it — and §27.4 closes that in two directions: *"the stream module
documentation MUST state this rule, and concurrency tests MUST prove only the guarantees actually
promised. Tests MUST NOT accidentally hard-code a stronger cross-channel order that the
implementation does not guarantee."*

ADR-0455 had already settled where a contract about streams lives: `docs/contracts/hardening/
streaming_classification.yaml` is Appendix E as data, read by `spec-check` and by `explain`. The
question here was whether an ordering contract is a second registry or part of that one.

## Decision

**One registry, one page rendered from it, and one paragraph in the stream module that the gate
compares against it.**

### 1. The contract is a block in the classification registry, not a second file

`ordering:` sits beside `classes:` and `operations:` in
`docs/contracts/hardening/streaming_classification.yaml`. Both answer the same question — what may a
consumer conclude about a stream — from two directions, and a second file would mean two places to
look and two places to forget. The block names four rules (per-channel, cross-kind, causality,
total order) and the two channels, each with what it carries and whether it is ordered within
itself.

### 2. `docs/reference/streaming.md` is generated from it

`cargo xtask docs` renders the classes, the operation placements and the ordering block onto one
page, which joins the six pages already derived from the registries. A user reading the reference
and a maintainer reading the YAML read the same sentences, because there is only one copy of them.

### 3. The stream module states the rule, and a test compares the two texts

§27.4's MUST is satisfied by a section of `crates/ono-pipeline/src/stream.rs`'s module
documentation. Prose in two media cannot be byte-identical — one wraps at 100 columns behind `//!`,
the other is folded YAML — so
`xtask/tests/reference.rs::should_render_this_repositorys_ordering_contract_as_the_stream_module_states_it`
flattens both and requires the two load-bearing sentences to appear in each. A change to either
that the other does not follow turns the gate red.

### 4. `StreamSink::send_in_sequence` is §27.3's sequence-bearing path

§27.3 requires that a producer whose contract must express "the error occurred between A and B"
emit through one path rather than rely on scheduling between two channels. The crate had no such
path, so a producer that needed one had no way to be right. `send_in_sequence` places every event
on the value channel in the order the producer chose, carrying a failure as the error value it
already is.

It is not the default, and the documentation says what it costs: a consumer of such a stream reads
its failures out of the values, so `Collected::errors` is empty and `saw_failure` stays false. That
is the trade §27.3 describes — the order becomes part of the answer, and the separation of §16.5
is given up for it — and a producer takes it only where its own contract says so.

### 5. The audit §27.4 asks for found nothing to loosen

Every consumer of `ValueStream::recv` in the test suites was read. `partial_failure.rs::should_interleave_failures_with_values_rather_than_waiting_for_the_end`
sets two booleans and returns when both are true — it asserts that both kinds arrive while the
stream runs, which is §16.5's guarantee, and says nothing about which arrives first.
`cancellation.rs` and `budget.rs` count and classify events without asserting a merged sequence.
No test hard-coded a cross-channel order, so none was loosened. The audit is recorded here because
"we looked and there was nothing" is a result, and an unrecorded one gets repeated.

## Consequences

Easy: the guarantee is now something a consumer can look up rather than infer, and the new tests
in `crates/ono-pipeline/tests/ordering.rs` are a template for what a concurrency test in this
repository is allowed to assert. A producer that needs a total order has one, and it is documented
where a reader of §27.3 will look for it.

Hard: `send_in_sequence` gives up the value/failure separation, which is a real property of the
default path — `Collected::errors` is how §16.5's "what succeeded and what failed are both
reported" reaches the renderer (ADR-0221). Anything built on it will need to unwrap error values
itself. Nothing in the shell uses it yet; it exists because §27.3 requires the path to be there
before a provider needs it, and its first user should re-read this paragraph.

Also: the ordering block widens `streaming_classification.yaml` beyond Appendix E, so the file's
name now undersells it. Renaming it would break ADR-0455's `spec-check` wiring and every pointer
into it for no gain a reader can feel; the header says what it holds.

Encoded by: `crates/ono-pipeline/tests/ordering.rs::should_deliver_every_event_of_one_channel_in_the_order_it_was_produced`,
`::should_hold_the_documented_guarantee_between_values_diagnostics_and_status`,
`::should_produce_a_total_order_when_the_caller_asks_for_one`,
`xtask/tests/reference.rs::should_render_the_stream_ordering_contract_from_the_registry`,
`::should_render_this_repositorys_ordering_contract_as_the_stream_module_states_it`.

## Alternatives considered

**A second registry, `docs/contracts/hardening/stream_ordering.yaml`.** Tidier by one axis and worse by
the one that matters: a reader asking "what does this stream promise?" would have to know that the
answer is split in two.

**State the rule only in the module documentation.** It is what §27.4 literally asks for, and it
would drift from the reference page within a tranche — which is the failure `docs/reference/` is
generated to prevent.

**Make the merged order deterministic instead of documenting that it is not.** One channel, or a
sequence number on every event. It would make the contract stronger and easier to reason about,
and it would put every partial failure behind the value channel's backpressure — so a stage that
was reporting failures faster than the consumer read values would stall on its own diagnostics.
§27.2 chose the weaker guarantee deliberately; this ADR implements that choice rather than
reversing it.
