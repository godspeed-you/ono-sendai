# ADR-0489: A benchmark result carries six numbers and the machine it was measured on

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §32.3, §32.4, §37.2, §37.4, §65.10, Appendix F.4; ADR-0488; issues #13, #83
- Decided by: agent (autonomous)

## Context

§32.3 lists six metrics and closes with the sentence that decides the shape of a result record:

> A single total runtime number is insufficient for streaming operations.

It is not an abstract worry. A streaming operation that answers in 200 ms and finishes in 40 s is
a different product from one that is blank for 39 s and then dumps everything, and a total runtime
cannot tell them apart. Issue #22's report — *"30 s wall clock, zero bytes of output"* — is a
*time to first value* of never beside a time to completion nobody measured, and the release cycle
it stayed open for is what §32.3 is buying back.

§32.4 supplies the second half, and it is about attribution rather than about content:

> Performance results MUST be stored in a machine-readable baseline file tied to the reference
> environment. CI MAY use percentage thresholds rather than exact wall-clock values on shared
> runners, but release qualification MUST run on a named reference environment with stable
> absolute targets.

Closed issue #13 established that §34's numbers were unmeasured. This is the infrastructure that
keeps them measured, and ADR-0488 has already put the cardinality a figure is measured at into
one declaration.

## Decision

**A benchmark result is six metrics, a profile, a commit and an environment. Anything less does
not parse.**

### 1. The six are required to be present, and three may be unknown

`xtask::perf::REQUIRED_METRICS` is §32.3's list in the specification's own order, each row
carrying the field name, the specification's wording, and which way the number gets worse.
Appendix F.4 permits different field names — *"Field names MAY differ, but the information content
is required"* — so the list is the information content and the field names are this repository's.

Three of them are qualified in the specification: RSS is required *"where practical"*, bytes
*"where available"*. Those, plus cancellation latency, may be `null`; the other three may not.
A missing field is always a problem, because v0.4.1 §2.6 keeps an unknown unknown rather than
letting it be absent and inferred as a zero. So `"peak_rss_bytes": null` is a result and a record
with no `peak_rss_bytes` key is not.

The refusal names the metric and quotes §32.3's wording for it, so the person who wrote the record
reads what is missing rather than a schema error.

### 2. A comparison that cannot honour §32.4's tie says so, and does not pass

`Baseline::compare` answers one of four things, and only one of them is a pass:

| Answer | When |
| --- | --- |
| `Held` | every metric is inside the tolerance |
| `Regressed(..)` | a metric moved the wrong way, naming metric, baseline, measured and allowed |
| `Unmeasured` | the baseline holds no record for that benchmark at that profile |
| `ForeignEnvironment` | the result names an environment the baseline does not describe |

The last two exist because §65.10's defect — a skip that reaches the summary as a pass — is
exactly what a benchmark comparison falls into by default. A result from a CI runner compared
against a reference-environment baseline is not a green run; it is a comparison nobody may draw a
conclusion from, and it has to be a distinguishable answer for a caller to refuse it.

`Tolerance` is a parameter rather than a constant, because §32.4 asks for two regimes:
`Tolerance::Percent` for a shared runner, `Tolerance::Absolute` for release qualification, where
the baseline figure *is* the target.

### 3. The baseline is checked where it is written

`cargo xtask spec-check` parses `docs/contracts/hardening/performance_baseline.json` on every gate run.
A record that dropped a metric would otherwise be discovered at comparison time, where the
comparison would skip that metric and report "held" — the regression detector losing a metric
without anybody being told is the failure this check exists to prevent.

### 4. The file lands with its environment named and no measurements in it

This increment delivers the record contract, the parser and the comparator. The runner that
produces records is §37.1's `cargo xtask perf`, which is issue #84 and the next increment; the
reference environment's §37.2 fields are documented there too. The baseline therefore names
`ryzen-3900x-ubuntu-2604` and holds an empty measurement set until #84 fills it by measuring.

Writing plausible figures into it now would have been worse than an empty file in exactly the way
issue #13 records: a number nobody measured, sitting where a measurement belongs, is a regression
detector that reports on nothing and says it is fine.

## Consequences

Easy: issue #84's runner has a record type to emit and a comparator to be checked against, and
`cargo xtask perf --compare` is a call rather than a design. A regression is reported with the
metric, the baseline, the measurement and the tolerance that was applied, which is enough to act
on without rerunning anything.

Hard: an empty baseline compares as `Unmeasured` for every benchmark, so the gate cannot fail on a
regression until #84 has measured one. That is a real gap for exactly one increment, and it is
visible — `Unmeasured` is an answer, not a pass.

Also hard: `Tolerance::Absolute` compares against the baseline figure exactly, so any noise at all
on the reference environment is a regression. §37.4's statistical rule is what makes that usable —
at least 20 iterations, median and p95 — and it lands with #84. Until then, absolute tolerance is
a mode nothing calls.

Encoded by `xtask/tests/perf.rs::should_record_all_six_required_metrics_for_every_benchmark`,
`::should_fail_when_a_benchmark_reports_only_a_total_runtime` and
`::should_compare_a_benchmark_result_against_the_baseline_for_its_reference_environment`.

## Alternatives considered

**Derive `values_per_second` from `values` and `time_to_complete_ms` rather than record it.** It is
arithmetic, and recording it is redundant — until a benchmark measures throughput over a window
that is not the whole run, which is the shape any streaming measurement eventually takes. §32.3
lists it as its own metric; recording it keeps the record able to say something the division
cannot.

**A JSON Schema for the record instead of a hand-written parser.** It would validate shape and say
nothing useful about *why* a field is required. The refusals here quote §32.3's wording for the
metric that is missing, which is what makes them actionable, and there is one document to validate.

**Store the baseline per environment in separate files.** §32.4 speaks of *the* baseline file tied
to *the* reference environment, and one file with one environment field is the direct reading. If
a second qualifying environment ever exists, a second file beside this one is the change, and
`ForeignEnvironment` is already the answer that makes the need visible.

**Let a foreign-environment comparison pass with a warning.** That is §65.10 with better manners.
CI comparing its own runners against their own baseline is legitimate; CI comparing itself against
the reference environment is not a verdict, and the code should not be able to produce one.
