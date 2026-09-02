# ADR-0492: A live map that is waiting says so

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4 §25.1, §25.2, §29.4, §43.6; v0.4.1 §33.3, §35.1, §35.2, §35.5, §61.2, §61.3,
  §61.5, §65.9; ADR-0024, ADR-0431, ADR-0459, ADR-0491; issues #22, #87
- Decided by: agent (autonomous)

## Context

§35.1 makes issue #22 a release blocker in as many words:

> The known class of behavior where `map --live` produces no bytes for tens of seconds on a
> realistic host MUST be eliminated for v0.4.1.

The report is `map --live --json | take 3 | to json` at 30 seconds, zero bytes, 0 % CPU and a flat
4 MiB RSS on a 920-process desktop, against 0.4 s and 27 KiB in the demo container. ADR-0431 found
the shape, and it is not what the report assumed: **`take 1` answers in 0.2 s**. The first frame
is fine. What never arrives is the second, and `to json` writes one document when the stream ends
(v0.2 §12.2), so a pipeline waiting for a third frame writes nothing at all.

The reason the second frame never arrives is that at the root there is nothing to report. The root
projection is `SYSTEM`, six domains and eighteen collections — thirty nodes whose labels are fixed
geography — and `MapSnapshot` reduces a projection to what a change can be about. A picture made
of names that cannot change never reports a change, and §25.2 is emphatic that it must not:

> A live map that keeps emitting events while the topology is unchanged is showing activity the
> machine is not having.

`docker/acceptance/cases/098-spatial-live-map.case` asserts exactly that, by requiring
`map --live | take 5` to be *unable* to collect five values from a still system inside six
seconds, and it names a heartbeat as a violation rather than an implementation detail.

So two rules meet: §25.2 forbids a frame, §33.3 forbids thirty seconds of nothing.

## Decision

**Between a frame and silence there is progress, and that is what a live map produces.**

### 1. A note, on the diagnostic stream, after ten seconds of no value

`crates/ono-cli/src/spatial/live.rs` records when the caller last saw a value and, whenever ten
seconds pass without one, writes a note to standard error naming how many sources it is watching
and how long the picture has been still. §65.9 lists the three acceptable answers — *"results,
progress or a bounded refusal"* — and this is the middle one.

It is a **note**, not a value: `take 3` does not consume it, `to json` does not collect it, and a
script reading the stream sees exactly the frames it would have seen before. §25.2 is about what
the map *shows*, and nothing here shows anything.

Ten seconds is a third of §33.3's budget. It is after case `098`'s six-second quiet window and
well before the thirty seconds at which the shell would be in breach, which is the whole of the
interval both rules leave open.

### 2. The clock is the caller's, not the loop's

The first attempt timed each individual wait, and it never fired: on a host whose providers do
emit events, the loop wakes, re-projects, finds nothing changed, emits nothing and waits again, so
no single wait is ever long. **A system that produces events which change no picture is exactly as
silent to the caller as one that produces no events at all**, and it is the caller §33.3 is about.
So the stillness clock is reset when a *value is sent*, and by nothing else.

### 3. What this does not fix, and where it went

`take 1` at the root answers in 0.2 s; at NETWORK against a hundred thousand listening sockets it
takes 23 seconds in a release build and 79 in a debug one, because the whole graph is built to
draw thirty nodes. That is §35.2's bounded initial projection and §34.4's rule about local
questions, it is issue #87, and ADR-0491's `#[ignore]`d Profile L watchdog is the test that owns
it. This increment deliberately does not touch it: the two failures share a command and nothing
else.

### 4. What the un-ignored proof now proves

`crates/ono-cli/tests/spatial_first_output.rs::should_answer_or_refuse_the_live_map_within_the_interactive_watchdog_on_profile_m`
loses its `#[ignore]` unchanged. Its assertion is still only `!silent()` — ADR-0431 chose that
deliberately so the fix could pick any of §35.2's answers — and it passes because the note is one
of them. It costs a thousand processes and the full thirty seconds on every gate run, which
ADR-0431 anticipated and left to this phase to decide; it is kept in the suite because §61.3 makes
it a watchdog and because a proof nobody runs is a proof of nothing.

Its neighbour needed one repair to survive going green:
`should_show_a_placed_population_to_the_process_provider_when_a_profile_fixture_is_built` counted
`sleep` children by parent pid, and now that the watchdog runs it places a thousand of its own
from the same parent on another thread. It counts a difference instead, which is what the socket
fixture beside it already did.

## Consequences

Easy: `map --live` in a pipe can no longer look hung, at any place and at any cardinality, and the
message says what would end the wait. `docker/acceptance/cases/196-live-map-stabilization.case`
holds all three halves — first frame, progress while waiting, Ctrl-C — at Profile M in the
container, and case `098`'s quiet window is unaffected.

Hard: a long-running `map --live --json > file 2>&1` accumulates one note every ten seconds. That
is six lines an hour on the stream notes belong on, and the alternative is the blank terminal
§65.9 forbids.

Also hard: the note is written directly to standard error rather than through the pipeline's
diagnostic channel, because the live stream is produced inside a `ValueStream` task that has no
reporter. It is the same `Reporter::note` every other note in this shell uses, so the shape and
the sanitisation are shared; the channel is the one thing that is not.

Encoded by
`crates/ono-cli/tests/spatial_first_output.rs::should_answer_or_refuse_the_live_map_within_the_interactive_watchdog_on_profile_m`,
`crates/ono-cli/tests/watch_live.rs::should_answer_a_bounded_first_projection_before_any_update_arrives`,
`::should_release_the_query_task_promptly_when_a_live_map_is_cancelled`, and case `196`.

## Alternatives considered

**Compare more of the projection, so the root reports a change.** The obvious reading of the
diagnosis, and it does not fix the reproduction: the only aggregate at the root is the DEVICES
cluster, and devices do not come and go on a desktop. It would add fields to `MapSnapshot` for a
change that does not happen.

**Emit an unchanged frame on a timer.** §25.2 forbids it, and case `098` was written to catch
exactly this, naming it "showing activity the machine is not having".

**Refuse `map --live` where the projection cannot change.** §33.3 allows a deterministic refusal,
and this would be one — but the picture at the root *can* change (a container starting is a node
appearing), so the refusal would be a prediction rather than a fact, and it would take away a
working live view to avoid a message.

**Say it once, at subscription time, instead of every ten seconds.** It would break case `098`'s
quiet window, which requires a still system to produce nothing at all for six seconds — and a
message at t=0 answers the question "is it hung?" before anybody has asked it.
