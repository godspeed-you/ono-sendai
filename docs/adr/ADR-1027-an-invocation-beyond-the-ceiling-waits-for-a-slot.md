# ADR-1027: An invocation beyond a package's concurrency ceiling waits for a slot, bounded

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §18.4, §18.5, §31.14, §31.15, §31.73, §31.74; `docs/contracts/kuang/protocol.v1.yaml`
  (`bounded-concurrency`, `limits.max_concurrent_invocations`); ADR-0586, ADR-0995, ADR-0998,
  ADR-1011; v0.6.5 review findings R-F2 and R-F3
- Decided by: agent (autonomous)

## Context

ADR-0586 let a package answer several invocations at once under a ceiling, with a default of one,
and made an invocation beyond it a refusal (`runtime.concurrency_limit`) "rather than a queue". At
the time an instance had one user at a time, so reaching the ceiling meant a deliberate burst.
Two later decisions made it an ordinary moment:

- **ADR-0995** runs a background job in the session's instance. A job streaming from a package
  with the default ceiling holds its only slot, and the shell's next `get <target>` of that
  package was refused at once (R-F2): `echo:count-forever | count &` and then `get echo-item`
  answered `K11207 … already has 1 invocation(s) open`. Before, the job had an instance of its own.
- **ADR-1011** streams a bounded answer instead of collecting it, so the upstream's invocation is
  open while later stages run. `get foo | each { get foo-detail … }` and even
  `get foo | take 1 | each { … }` at ceiling one were refused in the block (R-F3): `take` had
  cancelled the upstream, but the cancellation was still on its way when the block invoked.

The ceiling is real — a package that said one may not run two — but the refusal arrived exactly
where the user had done nothing wrong. And the host cannot always see the ceiling: the negotiated
contract says four when the manifest is silent, while the SDK's own default is one, which only the
package's refusal reveals.

## Decision

**The plugin still refuses beyond its ceiling; the host makes the invocation wait for a slot —
in order, bounded, cancellable — and refuses it to the consumer only when the wait expires,
naming what holds the slots.** The wire protocol and the SDK are unchanged.

1. **At the negotiated ceiling the host does not send.** An invocation that would exceed
   `max_concurrent_invocations` of the negotiated contract — or that arrives while others are
   already waiting — joins the instance's line of waiting invocations instead of being sent.
   `invoke`/`query` still return the `RunningInvocation` at once; its stream simply has nothing
   yet.
2. **A refusal from a ceiling the host cannot see is asked again.** When the package answers
   `runtime.concurrency_limit` while another invocation of the instance is open, the host does not
   hand the refusal on: the invocation goes back to the front of the line. If an invocation ended
   between the asking and the answer, it is asked again at once. Nothing of a refused invocation
   ran (ADR-0586 §3), so asking again is safe; its handles are the same.
3. **An invocation ending frees a slot.** Whenever an invocation's result arrives, the oldest
   waiting invocations are sent while the negotiated ceiling has room. This is what turns a job's
   end, a `kill %N` (ADR-0998) or the cancellation `take` sends upstream into the next invocation
   running — `take`'s cancellation does not need to complete before the next stage invokes; the
   next stage's invocation waits until it has.
4. **The wait is bounded.** `HostLimits::slot_wait_ms` (default 10 s, host-side, not part of the
   contract) bounds an invocation's wait from the moment it was asked for. When it expires the
   invocation is refused with `runtime.concurrency_limit` on its stream and in its result
   (ADR-0992), and the message names the open invocations holding the slots — `command:…`,
   `query:…` — with a help line saying what ends them (`kill %N`, `take`) and how a package raises
   its ceiling. The help carries the holders as metadata too.
5. **The wait is interruptible.** Cancelling a waiting invocation (`streams.cancel` through
   `RunningInvocation::cancel`: Ctrl-C, `kill %N`, a pipeline that stopped reading) ends it at
   once as `cancelled`, without anything reaching the package. A refusal that arrives for an
   invocation whose consumer had already asked to cancel is reported as that cancellation.
6. **No deadlock.** A wait ends in one of three ways, each bounded: a slot frees, the consumer
   cancels, or the deadline expires. For one pipeline, the cases decide as follows:
   - `… | take N | each { <same package> }`: `take` ends the upstream's invocation; the block's
     invocation waits for that cancellation to land and then runs.
   - `get <bounded> | each { <same package> }` at ceiling one: the block's invocation waits while
     the upstream answers. The upstream keeps answering into its credit window and the pipeline's
     buffer, so an answer that fits there ends and the blocks run; one larger than that cannot end
     while the block waits for it, and the block is refused when the wait expires, naming the
     upstream's `query:<target>`. Raising the package's ceiling, bounding the upstream with
     `take`, or collecting it first (`let xs = (get …)`) are the ways out the help names.
   - `get <unbounded> | each { <same package> }` without `take` at ceiling one: refused when the
     wait expires, naming the upstream — an endless answer never gives its slot back.

## Consequences

The shell, its jobs and later stages of a pipeline share a one-at-a-time package the way they
share any other resource: the second user waits for the first. A job holding the slot delays the
shell's invocation instead of failing it, and the shell's Ctrl-C ends that wait. A package that
opted into several invocations is unchanged below its ceiling.

What becomes harder: an invocation that waited is no longer instant to fail, so a script that
relied on the immediate refusal now waits up to the bound. The SDK conformance tests that held
an instance at its ceiling and asked for one more (ADR-0586) now see the refusal after a short
bound the test host sets (`slot_wait_ms: 200`).

The example package gained `echo-slow`, a bounded target that emits `count` items `pace-ms` apart
and ends: it holds a slot for a while and gives it back on its own.

Encoded by `crates/ono-kuang-sdk/tests/conformance.rs`:
`should_start_an_invocation_beyond_the_negotiated_ceiling_once_the_open_one_ends`,
`should_start_an_invocation_the_package_refused_for_its_own_ceiling_once_a_slot_frees`,
`should_end_an_invocation_waiting_for_a_slot_as_cancelled_when_its_consumer_cancels`,
`should_refuse_an_invocation_whose_wait_for_a_slot_expires_naming_what_holds_it`, and the updated
`should_refuse_an_invocation_beyond_the_negotiated_concurrency_ceiling` and
`should_keep_serving_a_package_that_answers_one_invocation_at_a_time`; by
`crates/ono-cli/tests/plugin_concurrency.rs` (the shell after a job's invocation ends, a waiting
job after the holder is killed, `take 1 | each`, a slow bounded upstream under `each` with and
without `take`); and by acceptance case `175-an-invocation-waits-for-a-packages-only-slot`. The
`bounded-concurrency` rule and the `max_concurrent_invocations` limit in `protocol.v1.yaml` say so.

ADR-0586 §3 still holds for the plugin: it refuses beyond its ceiling and never queues. What this
ADR changes is what the host does with that refusal.

## Alternatives considered

**Give each job an instance of its own again.** ADR-0995 rejected the second lifecycle model.

**Keep refusing at once and document it.** R-F2 and R-F3 are not misuses: every ceiling-one
package — the SDK default — would fail the second of two ordinary commands typed a second apart.

**Wait without a bound.** An endless invocation holding the slot (`count-forever` in a job) would
hold the shell; spec §31.15's own "a further call waits; it does not queue without bound" is the
rule this follows.

**Drain a bounded upstream into host memory when a later stage waits for its slot.** It would make
`get <large bounded> | each { <same package> }` work at ceiling one, at the cost of reading a
whole bounded answer into the host — the collecting ADR-1011 removed — whenever any invocation
waits. Not taken: the bound and the named holder are enough to act on, and the package can raise
its ceiling.

**Wait in the shell, retrying the whole command.** The shell does not know whether a refused
command ran nothing; the supervisor does, and it alone sees the result frames that free a slot.
