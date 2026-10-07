# ADR-0953: `measure` holds the distribution only when it is asked for it

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §11.1, §35.3, §53; v0.4.1 §2.5, §22.1–§22.4, §55.1, §65.8, Appendix E;
  ADR-0014, ADR-0221, ADR-0454, ADR-0455 (the classification), ADR-0460 (what `explain` shows)
- Decided by: agent (autonomous)

## Context

ADR-0455 placed `measure` in `explicit_collect` by its properties and named the improvement it
left for later: *"`count`, `sum`, `mean`, `min` and `max` are constant-state, but an exact
percentile is defined over the whole distribution and holds a sample per value … Splitting the two
halves would move the first to `incremental_aggregate`."* (issue #174). Every `measure` held every
sample — because it always reported a median — so a ten-value `limits.materialize_items` refused
a thousand values whose sum it never needed to hold, and an unbounded source was refused outright.

Two things are left open by the specification. §53 says `measure` yields statistics and lists none
of them as mandatory; and nothing says what a constant-state aggregate does with a stream that
never ends. ADR-0455's classes require a stage that needs the end to refuse before the beginning,
and a constant-state statistic does not need the end.

## Decision

### 1. The default `measure` is an incremental aggregate

`measure <key>` computes `count`, `skipped`, `sum`, `mean`, `min`, `max` and `stddev` in constant
state: a running sum, the extremes so far, and Welford's running mean and sum of squared
deviations for the population standard deviation. Nothing is held per value, so no
materialization budget is charged, and the contract places the command in
`incremental_aggregate`. `median` and `percentiles` are `null` in its answer — unknown because not
asked for, which is what `null` means (spec §35.3).

### 2. The distribution is held when it is asked for

`--median` reports the median, and `--percentiles [p, …]` the nearest-rank percentiles, each
between 0 and 100 (anything else is `type.mismatch`). Asking for either holds every sample, exactly
as `measure` always did: the invocation requires finite input, refuses an unbounded upstream before
reading a value, and materializes within `limits.materialize_items` and `limits.materialize_bytes`
(§22.1–§22.3). With the distribution held, every statistic is reported, the median included.

The class of an invocation is therefore not always the class of its command. The contract says so
where it is true: an option may declare `execution: <class>`, the class an invocation that sets it
is in (`ParameterSpec::execution`, validated against Appendix E's eight classes when the registry
loads), and `CommandContract::execution_for` answers the class of one bound invocation. `explain`
reports that — `execution  streaming` for `measure pid`, `execution  global materialization` and
`requires  finite input` for `measure pid --median` — so §22.4's plan describes the stage that would
run rather than the command in general.

### 3. Over an unbounded stream, the answer is the running answer

A constant-state `measure` over a stream declared unbounded emits an `ono.measure/1` record after
every value — the statistics of everything read so far — and its own output is unbounded. A stream
with no end has no end to answer at, and the running answer is what a consumer can use:
`tail file log --follow | … | measure latency | take 3` is answered after three values, while the
source is still open (§2.5), and `watch … | measure x` shows the statistics as they change. Over a
stream that ends nothing changes: one record, at the end.

### 4. One schema

`docs/contracts/schemas/measure.v1.yaml` publishes what `measure` produces, and the stage reads its
schema from the registry that embeds that contract instead of building a second copy in Rust, so
the two cannot drift. The entry leaves `deferred.yaml`.

## Consequences

Easy: the common statistics of a large or endless stream cost nothing to hold and are never
refused by a materialization limit; `measure` is usable on a live stream.

Hard, and stated for an upgrade: a script that read `.median` from a plain `measure` reads `null`
now and has to ask with `--median`. The default view (`count`, `sum`, `mean`, `min`, `max`) is
unchanged, so what a person sees at the prompt is too. The standard deviation is computed in one
pass rather than two; the two agree to floating-point rounding.

Encoded by `crates/ono-cli/tests/measure.rs`:
`::should_measure_constant_state_statistics_past_the_materialization_limit`,
`::should_still_refuse_a_percentile_past_the_materialization_limit`,
`::should_report_the_median_and_the_percentiles_when_they_are_asked_for`,
`::should_answer_a_measure_over_an_unbounded_source_after_every_value`,
`::should_explain_measure_as_streaming_unless_a_percentile_is_asked_for`; and
`crates/ono-pipeline/tests/budget.rs::should_measure_constant_state_statistics_without_charging_the_materialization_budget`,
`crates/ono-pipeline/tests/boundedness.rs::should_answer_after_every_value_when_constant_state_measure_reads_an_unbounded_stream`.
The collecting `Measure::new` keeps every test it had.

## Alternatives considered

**Keep the median in the default and hold the distribution while it fits.** A median that is a
value under the budget and `null` above it is a result that depends on how much input there
happened to be; §35.3 reserves `null` for what is unknown, not for what was discarded.

**A second command for the distribution statistics.** Two commands for one question, and a contract
whose `measure` silently changed meaning. An option says what is being asked for where it is asked.

**Refuse an unbounded source as before, constant state or not.** ADR-0455's refusal exists for
stages that need the end; refusing a stage that does not is the shell refusing to answer a question
it can answer.

**One record at the end of an unbounded stream** — there is none, which is the point of §65.8.
