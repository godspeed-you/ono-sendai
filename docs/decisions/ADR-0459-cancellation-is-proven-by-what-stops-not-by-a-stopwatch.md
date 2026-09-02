# ADR-0459: Cancellation is proven by what stops, not by a stopwatch

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §23.3, §28.3, §37.1, §37.2, §37.3, §37.4, §61.5, Appendix A; v0.2 §18.5;
  ADR-0252, ADR-0431; issues #21, #71, #83, #84
- Decided by: agent (autonomous)

## Context

§23.3 states a behaviour and a measurement, and they are not the same requirement:

> Cancellation while capturing MUST stop upstream consumption promptly and release retained values
> as soon as the owning operation unwinds.
>
> ```
> p95 < 100 ms
> p99 < 250 ms
> ```
>
> The benchmark MUST measure from cancellation signal to cessation of additional captured-value
> growth.

The behaviour belongs to this phase. The measurement, by the specification's own arrangement, does
not: §37.2 requires a **named release reference environment**, and §37.4 requires the statistical
rule a benchmark reports under. Neither exists yet — they are issues #83 and #84, phase H7 — and a
p95 measured on whatever machine ran `cargo test` is a measurement of that machine.

This repository has the receipt for what happens when that distinction is ignored. ADR-0252
accepted a proxy for spec §34's 50 ms completion budget, with a comment saying a wall-clock
assertion "is flaky on shared hardware". Issue #21 is still open, and its title is *"the
first-completion budget is still measured by a proxy"*. ADR-0431 chose §33.3's thirty-second floor
over §33.2's 500 ms target for the same reason, and said so: the targets "would be a coin toss on
a shared runner the day they went green".

## Decision

**This increment proves that capture growth stops. It does not assert a millisecond figure, and it
does not leave an `#[ignore]`d test pretending to.**

### 1. The deterministic proof

`crates/ono-pipeline/tests/cancellation.rs::should_stop_a_capture_growing_when_the_scope_is_cancelled`
runs a source that counts what it has sent into a materializing operation with a ceiling far out
of reach, cancels, and asserts three outcomes, none of them a duration:

- the operation **unwinds** rather than running to its ceiling — so whatever ended it was the
  signal;
- once it has unwound the counter is read, a pause passes, and it is read again and is **equal** —
  which is §23.3's "stop upstream consumption" written as something a test can observe rather than
  as a threshold it might miss;
- the consumer is told why it ended, with `stream.cancelled` (spec §18.5).

The whole test is a fraction of a second, and it fails for exactly one reason: a cancellation that
does not stop the capture.

The user-visible half is
`crates/ono-cli/tests/resource_limits.rs::should_stop_capture_growth_within_the_cancellation_budget`:
a capture over a walk of the whole filesystem, Ctrl-C on a real terminal, `128 + SIGINT`, a prompt
that answers, and — the part that matters for "release retained values as soon as the owning
operation unwinds" — a *second* capture that succeeds afterwards with its whole allowance. A shell
still holding the first one could not do that.

### 2. What was measured, and deliberately not encoded

The p95/p99 test was written, run and removed. On the reference machine, 100 cancellations
complete in **0.07 s in total** — the latency from signal to cessation is under a millisecond, two
orders of magnitude inside §23.3's p95 target and three inside its p99. So the targets are met
today, comfortably, and the figure is recorded here rather than asserted anywhere.

It is not asserted because asserting it would be issue #21's defect repeated with a wider margin:
a threshold on a machine the specification does not name, in a test that would be the first thing
to go orange on a loaded runner, standing in for the benchmark §37.1 requires. §61.5 places
cancellation under load among the *performance* acceptance scenarios, not the resource ones, and
that is where it belongs.

### 3. Why it is not an `#[ignore]`d test either

`#[ignore]` would have been the obvious way to keep the measurement in the tree. AGENTS.md §7
requires an ignored test to carry a `// REASON:` **and** an entry under *Deferred* in
`docs/STATE.md`, and `spec-check` enforces both. A test whose bookkeeping cannot be completed is a
test that turns the gate red for everyone, and an ignored test that measures the wrong machine is
not worth that. The measurement is owed by #83 and #84, and this ADR is where the debt is written
down.

## Consequences

Easy: the cancellation behaviour is under test on every gate run, deterministically, and the suite
gains no flake.

Hard: issue #71's latency half is not closed by this increment. §4.8.6's box should say so —
the deterministic proof is green, and the distribution is H7's — and the H7 harness has a named
requirement waiting for it: measure from the cancellation signal to the cessation of captured-value
growth, on §37.2's reference environment, under §37.4's statistical rule, and report p95 and p99.

Also hard: the figure in §2 above is evidence, not a guarantee. Nothing fails if cancellation
latency regresses to 90 ms — the deterministic proof still passes, because growth still stops.
That gap is exactly what the benchmark harness is for, and pretending otherwise with a threshold
nobody trusts would close the gap on paper only.

Encoded by `crates/ono-pipeline/tests/cancellation.rs::should_stop_a_capture_growing_when_the_scope_is_cancelled`
and `crates/ono-cli/tests/resource_limits.rs::should_stop_capture_growth_within_the_cancellation_budget`.
Owed by issues #83 and #84.

## Alternatives considered

**Assert p95 < 100 ms and p99 < 250 ms in `cargo test`.** The margin today is enormous — a factor
of a hundred — which is the strongest case anyone will ever have for this option. It still measures
the wrong machine, and §37.2 exists because the specification wants the figure attached to a named
one. When H7 has that environment, the same 100 samples become a real benchmark rather than a test
that happens to pass.

**An `#[ignore]`d p95/p99 test.** Rejected in §3: the bookkeeping AGENTS.md §7 requires is not
this agent's to write, and a proof nobody runs is a proof of nothing (ADR-0428's subject in a
different costume).

**Bounding the values admitted after the signal instead of watching the counter settle.** The
cleanest-sounding deterministic form, and it races: between the test reading the counter and
calling `cancel`, an unthrottled source keeps producing, so the bound would have to be loose
enough to be uninformative. Watching the counter *after* the operation has unwound has no race in
it at all.

**Cancelling through the `Ctrl-C` path only.** The terminal proof is there and is the honest
user-visible one, but it cannot see what the capture did — only that the shell survived. The
in-process proof is what makes "capture growth stopped" an assertion rather than an inference.
