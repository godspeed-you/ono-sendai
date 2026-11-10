# ADR-1011: A bounded contributed answer streams as a core provider's does

- Status: accepted
- Date: 2026-11-10
- Spec refs: §5, §16.5, §18.2, §31.14, §31.23; `docs/architecture/external-system-provider.md`
  §12.4, §21.4; `docs/contracts/kuang/contributions.v1.yaml` (`target.answer`); issue #224 (7);
  ADR-0582, ADR-0588, ADR-0590, ADR-0992
- Decided by: agent (autonomous)

## Context

`get <contributed target>` had two routes, chosen by the target's declared `answer` (ADR-0588).
An `unbounded` answer became a `ValueStream` the pipeline ran as it arrived. A `bounded` one was
collected: `plugins::query` awaited `RunningInvocation::collect()` and seeded the pipeline with the
finished `Vec`. So the first value of a bounded answer waited for its last. `get pod | take 1`
against a cluster with ten thousand pods read ten thousand pods before showing one, and a
`take`, `where … | take` or `first` downstream could never stop the package early, because by the
time the pipeline existed the package had already finished.

That was not a decision anybody took. ADR-0582 collected because the host had one shape for a
contributed answer; ADR-0588 split off the case where collecting *hangs* and left the other as it
was. Core providers already stream bounded enumerations, and §5 says a pipeline is a stream of
values; the collecting route was the only place a provider's answer did not reach the next stage
until it was complete.

Collecting had one thing going for it: `delivered` read the invocation's result as well as its
stream, so a failure after some values failed the command instead of reading as a shorter answer.
Since ADR-0992 the supervisor puts that failure on the stream itself, as its last event, and
`plugin_provider::stream_of` already forwards it as the stream's failure. The reason to collect is
gone.

## Decision

**A contributed target's answer is passed on as it arrives, bounded or not. The declaration
decides only the stream's boundedness, never whether it is collected.**

- `plugins::query` returns `Answered::Live(stream_of(…))` for every target, with
  `Boundedness::Bounded` or `Boundedness::Unbounded` from the declaration. `stream_of` stays the
  single door for one invocation of a contributed target (ADR-0588), now for the shell's `get` path
  in both cases as well as for `PluginProvider::snapshot`.
- A failure after some values is the stream's last event, so the pipeline reports it, the command
  exits non-zero, and the values that did arrive stand beside it as §16.5's partial answer. It is
  said once (ADR-0992).
- A downstream stage that stops reading — `take 1` — cancels the stream, and `stream_of` delivers
  the cancellation to the package (§31.14), exactly as it does for an unbounded answer.
- `Answered` keeps its two variants: `Values` is a contributed *command's* answer, read to its end
  by `command.invoke`'s route, and `Live` is a target's, whatever its boundedness. Renaming the
  variant would touch the contributed-stage dispatch in `eval/pipeline.rs` for no change in
  behaviour.

## Consequences

`get <contributed target> | take N` finishes on the Nth value and stops the package, and the first
row of a slow enumeration reaches the screen when the package emits it. A bounded answer with no
consumer is still drawn as a table, because the stream says it ends.

The example package gained `echo-hold`, a bounded target that emits one item and then, with
`--then wait`, nothing until the query is cancelled, or with `--then fail`, a failure. Without
`then` it ends after the item, so a search that reaches it by its schema
(`find place --type EchoItem`) is not held: a bounded target that never ends would hold a bounded
search, which asks for a prefix but still waits for its end or its bound.

The proof is an ordering rather than a clock: `echo-hold --then wait` can only end when `take 1`
cancels it, and `take 1` can only act on a value it was handed, so the command returning at all
shows the first value travelled on before the package had finished. Encoded by
`crates/ono-cli/tests/plugin_targets.rs`:
`should_hand_a_bounded_answers_first_value_on_before_the_package_has_emitted_its_last` (it hangs
on the collecting route, and the watchdog fails it) and
`should_fail_rather_than_answer_short_when_a_bounded_answer_fails_after_a_value`; and by
acceptance case `172-kuang-bounded-target-streams`. `contributions.v1.yaml` says that either kind
of answer is passed on as it arrives.

## Alternatives considered

**Keep collecting, and stream only when the pipeline asks for a prefix.** Rejected: whether a
later stage will stop early is not known when the head starts, and two routes chosen by what comes
after the head is the "two doors, two behaviours" ADR-0588 removed.

**Collect, but with a bound.** Rejected: it trades a late first value for a silently truncated
answer, which is the substitution §21.4 of the provider contract forbids.
