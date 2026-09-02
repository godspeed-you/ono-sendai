# ADR-0482: A bounded channel is a rule the gate can read

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §28.1, §28.2, §28.3, §28.4, §60.2, §60.3, §65.7; ADR-0013, ADR-0059,
  ADR-0252 and ADR-0459 (a stop is proven by what stops), ADR-0480, ADR-0481
- Decided by: agent (autonomous)

## Context

v0.4.1 §28 is the constraint the rest of phase H6 had to be built inside, and it is written as
three prohibitions rather than as a feature:

> The default pipeline data path MUST continue to use bounded channels. … Changing this number for
> tuning MAY occur through an ADR and benchmark evidence, but **replacing bounded flow with
> unbounded channels is forbidden.** … The `each`/function streaming changes MUST propagate
> downstream backpressure upstream. They **MUST NOT solve materialization by inserting an
> unbounded task queue.** … When cancellation and capacity availability race, cancellation SHOULD
> win.

§65.7 states the failure mode in one line: *"replacing a foreground `Vec` with an unbounded
background queue is not a streaming fix and is forbidden."* The trouble with all four is that they
are satisfied by absence. A reviewer confirms them by not finding something, and a reviewer who
does not look confirms them just as convincingly.

The cancellation half had its own problem. Issue #21 and ADR-0252 are this repository's record of
what a millisecond threshold on shared hardware costs, and ADR-0459 settled the rule that came out
of it: a stop is proven by what stops, never by a stopwatch.

## Decision

**The prohibition is a scan, the backpressure bound is arithmetic, and the cancellation race is
arranged rather than waited for.**

### 1. `xtask::scan::check_bounded_channels` fails the gate on an unbounded channel

Every source under `crates/ono-pipeline/src` and `crates/ono-cli/src` is read with comments and
string literals blanked, and `unbounded_channel(`, `UnboundedSender` and `UnboundedReceiver` are
refused wherever they appear. It is a blunt rule and deliberately so: there is no legitimate use
of an unbounded Tokio channel on the shell's data path, and a rule with an exemption list is a
rule that grows one more exemption per tranche. Prose about the prohibition — this ADR's own
sentences, the comment above the scan — is not the prohibition being broken, which is why comments
are blanked first.

The reference capacity itself is asserted rather than assumed:
`should_keep_the_reference_channel_capacity_the_specification_names` reads
`ono_pipeline::DEFAULT_CAPACITY` and requires 64. §28.1 permits that number to change through an
ADR with benchmark evidence, and this is what makes such a change visible rather than incidental.

### 2. The backpressure bound is stated as arithmetic, not as a threshold

`should_keep_the_retained_queue_within_the_bounded_channel_when_the_consumer_is_slow` puts an item
transform between a source that never stops and a consumer that reads one value at a time, and
asserts after each read that the source has produced at most `reads + 2 × capacity + 1` values.
Two channels stand around the transform and it holds one value while it maps it; that is the whole
derivation, and it is written into the test so the number can be checked rather than trusted. The
same arithmetic appears in `streaming_transforms.rs::should_hold_no_more_than_the_bounded_channel_and_one_in_flight_frame`,
which is §58.2's "memory stays within bounded channel plus per-item frame overhead" as a count.

### 3. The cancellation race is arranged, and proven by two equal readings

`should_stop_an_in_flight_block_when_the_pipeline_is_cancelled` sets the capacity to one and reads
nothing, then yields until the mapped count stops moving. At that point every task in the pipeline
is parked on a `send` that needs only one reader to complete — which is precisely the race §28.3
names. Cancelling then must stop them where they stand, and the assertion is that the count after
cancellation equals the count before it. No duration appears; the reading that would have grown is
the reading that proves it did not.

`StreamSink::send` and `StreamSink::fail` already select `biased` on the cancellation token ahead
of the channel, which is why the answer is "wins" rather than "usually wins". This ADR records
that the bias is load-bearing rather than stylistic.

### 4. A child a stage owns is a child the shell ends

`should_reap_the_child_process_of_a_cancelled_stage` runs `adapt find <scratch> / -type f |
take 1`. The decoder is line-oriented, so records reach the pipeline while the child is still
walking; the scratch directory comes first in the child's arguments so it produces at once, and
the rest of the filesystem comes after it so there is work left when `take 1` stops reading. The
scratch path appears in the child's command line, which is what makes it this test's child and not
somebody else's; `/proc` is read directly, so the test depends on the interface the shell's own
process provider depends on and on no other program.

The positive half is asserted too — the record really arrived — because a test whose subject is an
absence must first prove the thing was ever present (§65.10).

## Consequences

Easy: the three prohibitions of §28 are now things that fail rather than things that are reviewed.
A later tranche that reaches for an unbounded channel to make a stage simpler gets a red gate with
the sentence that forbids it, and a tranche that raises the channel capacity has to say so.

Hard: the scan is syntactic, so a hand-rolled unbounded queue — a `VecDeque` behind a mutex, a
`Vec` a task appends to — is invisible to it. That is the same shape §26.1's capture inventory
covers, and the two rules are complementary rather than overlapping: one refuses an unbounded
channel, the other refuses an unclassified collection. Between them the queue has nowhere to hide
that does not also fail `check_evaluator_captures`.

Also hard: `check_bounded_channels` covers `crates/ono-cli/src` whole, not only the evaluator. That
is deliberate — a channel between the shell and a reader thread is as much the data path as one
between two stages — but it means a future non-pipeline use of an unbounded channel in that crate
will need this decision revisited rather than an inline `#[allow]`.

Encoded by: `xtask/tests/scan.rs::should_report_an_unbounded_channel_on_the_pipeline_data_path`,
`::should_leave_a_bounded_channel_alone_when_scanning_for_unbounded_ones`,
`::should_ignore_prose_about_an_unbounded_channel_when_scanning`,
`::should_find_no_unbounded_pipeline_channel_in_this_repository`,
`crates/ono-pipeline/tests/backpressure.rs::should_keep_the_reference_channel_capacity_the_specification_names`,
`::should_keep_the_retained_queue_within_the_bounded_channel_when_the_consumer_is_slow`,
`crates/ono-pipeline/tests/cancellation.rs::should_stop_an_in_flight_block_when_the_pipeline_is_cancelled`,
`crates/ono-cli/tests/streaming.rs::should_reap_the_child_process_of_a_cancelled_stage`, and
acceptance case `194`.

## Alternatives considered

**Trust the review.** §28's prohibitions are exactly the kind that survive one tranche and not
three. §65.7 exists because this pattern has been seen before.

**Assert a cancellation latency.** The obvious reading of "cancellation wins" is "cancellation
wins within N milliseconds", and it is the reading ADR-0252 and issue #21 already paid for. The
latency figure is owed by phase H7's benchmark harness (#83, #84), where a number that moves is
the point; here it would be the flake.

**A clippy lint or a `deny` on the tokio import.** It cannot distinguish the data path from the
rest of the crate and cannot carry the sentence from §28.1 that explains why. A `Problem` with the
specification quoted in it is what a reader needs at the moment the gate goes red.
