# ADR-0992: An invocation's stream carries its whole answer

- Status: accepted
- Date: 2026-11-10
- Spec refs: §16.5, §31.14, §31.15, §31.34, §31.74; `docs/architecture/external-system-provider.md`
  §21.4; `docs/contracts/kuang/protocol.v1.yaml` → `invariants` (`cancellation`,
  `partial-errors`), `streams.close`, `command.invoke`, `provider.query`; ADR-0022, ADR-0586,
  ADR-0590
- Decided by: agent (autonomous)

## Context

The supervisor answers an invocation on two channels: the value stream
(`RunningInvocation::next`) and the result (`RunningInvocation::finish`). On the result arm it
removed the output stream and dropped its sender, so a handler that returned `Outcome::Failed`
ended the stream exactly as a handler that completed did. Nothing in the protocol contract said a
consumer had to read both. `plugin_provider::stream_of` did not, for four days, and every refusal
from an unbounded target reached the prompt as a clean empty answer; ADR-0590 fixed that consumer
by making it read the result as well, and left the trap in place for the next one (issue #154).

Three endings were silent on the stream:

1. **a failed result while the stream was open** — before or after values;
2. **a clean `streams.close` followed by a failed result** — the stream ended at the close, and the
   failure came later on the other channel;
3. **a cancelled result nobody asked for** — the package stopped and said "cancelled", and the
   consumer saw a complete answer. `protocol.v1.yaml`'s `cancellation` invariant already says
   cancellation "arrives as a terminal `stream.cancelled`, not as a stream that simply stops".

## Decision

**The stream alone is sufficient. It ends without a terminal event only when the invocation
completed, or was cancelled because its consumer asked; every other ending is its last event.**

- On a `failed` result the supervisor sends the result's error on the stream before closing it.
  A failed result without an error gets `provider.unavailable`, "the package reported the
  invocation failed and named no reason" — the same words ono-cli's command path already uses.
- On a `cancelled` result the supervisor sends `stream.cancelled` unless the consumer asked for the
  cancellation (`RunningInvocation::cancel`). A cancellation stays a cancellation: its code is
  `stream.cancelled` (kind `cancelled`), never a package failure, and the one the consumer asked
  for ends the stream silently as before, because that consumer already knows.
- A package's clean `streams.close` on its output no longer ends the consumer's stream. The
  sender is held until the result arrives, then closed — with the failure first if there was one.
  A close *with* an error still ends the stream with that error at once, and nothing is added
  after it. Emitting after a close stays a protocol violation.
- An instance that dies sends its failure on every open stream, held ones included.
- The result channel is unchanged and still says how the invocation ended. A consumer that reads
  both must not report the failure twice: the stream's terminal event *is* the result's failure.

The rule is in the contract in words, as the invariant `one-channel-carries-the-answer`, and
`streams.close`, `command.invoke` and `provider.query` point at it.

`plugin_provider::stream_of` therefore returns at the end of the stream and no longer reads the
result — reading it as well reported every refusal from an unbounded target twice once the stream
carried it. `snapshot` is `stream_of` with a bound and follows. ono-cli's command path
(`plugins::delivered`) stops at the first `Failed` event and so reports the same failure once
without change.

## Consequences

A consumer written tomorrow cannot lose a failure by reading the obvious channel. Encoded by
`crates/ono-kuang-sdk/tests/conformance.rs`:
`should_end_the_stream_with_the_refusal_when_a_handler_fails_before_emitting`,
`should_end_the_stream_with_the_failure_when_a_handler_fails_after_emitting`,
`should_end_the_stream_with_the_failure_when_a_package_closes_it_cleanly_and_then_fails` (a raw
wire fixture, `--misbehave=close-then-fail`),
`should_end_a_stream_its_consumer_cancelled_without_a_failure`,
`should_end_the_stream_as_cancelled_when_a_package_stops_without_being_asked`; by
`crates/ono-cli/tests/plugin_targets.rs::should_report_a_refusal_from_an_endless_answer_once`;
and by acceptance case `129-kuang-unbounded-target`, which now also asserts the refusal is said
once. The example package's `emit` gained `then: fail | cancel`.

`should_refuse_an_invocation_beyond_the_negotiated_concurrency_ceiling` changed with the contract:
the refused invocation's stream used to be empty and now carries `runtime.concurrency_limit`.

**User-visible:** a contributed command or target whose package answers `cancelled` without
having been cancelled now fails with `stream.cancelled` after the values it sent, where it used to
succeed with a truncated answer. A package following the SDK reaches that only when its own
`emit` fails, which is a cancellation or a host that is gone.

## Alternatives considered

**Say in the contract that a consumer must read both channels, and change nothing else.** The
issue's other option. It leaves the trap armed; ADR-0590 is the evidence that a contract sentence
does not stop the next consumer from reading only the stream.

**Make the result the only channel for outcomes and drop terminal events from the stream.** The
stream already carries terminal failures (`fail-stream`, a schema violation, a close with an
error, an instance crash), and a consumer would then have to read both channels for every ending.

**A distinct `StreamEvent::Cancelled` variant.** Cleaner to match on, but `StreamEvent` is matched
exhaustively by consumers outside this crate, and `stream.cancelled` is already the shell's code for
a stream that stopped short (`ono-pipeline`'s own cancellation event).
