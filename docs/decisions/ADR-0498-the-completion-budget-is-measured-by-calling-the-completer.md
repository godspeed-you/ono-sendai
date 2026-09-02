# ADR-0498: The completion budget is measured by calling the completer

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §36.2, §37.2, §37.3, §37.4, §52.2, §55.1, §61.4, Appendix A; v0.2 §34;
  ADR-0252, ADR-0456, ADR-0490, ADR-0494; issues #21, #86
- Decided by: agent (autonomous)

## Context

Issue #21 is one sentence long in effect: §36.2's first-completion budget *"is asserted as a
1 000-iteration in-process proxy … and never measured in the container"*. The proxy is
`crates/ono-command/tests/completion.rs::should_stay_far_inside_the_first_completion_budget`, and
what makes it a proxy is one argument:

```rust
let _ = ono_command::complete(registry, &context, None);
```

`None` is where the value completer goes. So it measures registry metadata lookups, touches no
provider, and asserts a thousand of them in under a second — while the thing §36.2 budgets is a
provider read on the keystroke path. ADR-0252 said so when it accepted the proxy, and named what
was missing: a completion the container can invoke without a terminal.

ADR-0456 recorded a second half of the same debt. `limits.completion_soft_ms` (50) and
`limits.completion_hard_ms` (150) were declared in the catalogue, range-checked, mirrored into
`docs/spec/hardening/limits.yaml`, and **read by nobody**, while `complete.rs` carried a 40 ms
constant of its own. `enforced_by: ono-cli` overstated the truth by one increment.

## Decision

### 1. The two budgets are the catalogue's, and they do the two different things §36.2 gives them

> At the soft budget, completion MAY return a partial set marked incomplete. At the hard budget it
> MUST stop additional discovery work and return what it has.

`ProviderValues` now carries both, taken from `crate::limits::completion(settings)` where the line
editor builds it. The **soft** budget is how long a keystroke waits before answering with what it
has; the **hard** budget is a deadline the reading thread checks before asking a *further*
provider. One provider that has begun is allowed to finish — a read cannot be stopped halfway
without leaving the value half-read — and no other is asked.

The 40 ms constant is gone. There is one home for the number and it is the one §52.2 asks for.

### 2. The measurement calls the completer

`cargo xtask perf` now records `completion.first_candidate`: a cold provider-backed completion of
`get user <TAB>`, through `ono_cli::complete::ProviderValues` in the seam the editor installs it
in, on §37.2's named reference environment under §37.4's rule.

It is **one sample per process**, because the completer caches what a provider said for five
seconds and a second call in the same process is a cache hit — a different measurement under
§37.3. Twenty cold samples are therefore twenty processes, and the runner gets them by re-running
itself: `cargo xtask perf --sample-completion` performs one completion and prints its latency and
its candidate count. No new product surface, and every sample is genuinely first.

**What it says**, twenty iterations, release build, `ryzen-3900x-ubuntu-2604`:

```
completion.first_candidate   11.47 ms median   11.63 ms p95   50 candidates
```

against §36.2's 50 ms soft budget and 150 ms hard budget. The figure has been unknown since v0.2
§34 asked for it.

### 3. What the container proves, and what it cannot

Case `198` drives a real terminal: `inspect limits` reports both budgets with Appendix A's values
— so the number a user reads is the number the shell enforces (§54.3) — and `get user ro<TAB>`
still completes from the providers. It does not *time* anything, because §32.4 puts absolute
figures on the named reference environment rather than on whatever built the image, and a
millisecond budget measured on a shared CI runner is the flake ADR-0252 was avoiding in the first
place.

### 4. Two things this increment could not do, and why

**The non-interactive completion surface.** ADR-0252 named it and it is still what a container
measurement of the *latency* would need. Both ways to add one — a flag in
`crates/ono-cli/src/invocation.rs`, or a command registered in `crates/ono-cli/src/native.rs` —
are in files this branch is not permitted to edit, because they are being changed in parallel
elsewhere. It is reported rather than done.

**The incomplete marker.** §36.2 makes it a MAY — *"completion MAY return a partial set marked
incomplete"* — and it cannot be delivered from here in any case: both
`ono_command::ValueCompleter::complete` and `ono_command::complete` return a bare
`Vec<Candidate>`, and `ono-command` is likewise out of scope for this branch. `docs/ACCEPTANCE.md`
§4.8.8 named a test for it; the box now names the test for the budgets, which is the MUST.

## Consequences

Easy: §36.2's budget is a figure somebody measured, on a machine somebody named, by calling the
code the keystroke calls. A regression in it is a regression in the baseline rather than in an
impression.

Hard: the soft budget went from 40 ms to 50 ms, so a keystroke now waits ten milliseconds longer
for a cold provider before answering without it. That is Appendix A's number, and the 40 ms was
chosen in ADR-0252 to leave room inside a 50 ms *end-to-end* budget — a reading §36.2 replaces by
splitting the budget into two.

Also hard: `xtask` now depends on `ono-cli`, so building the automation builds the shell. It is
the price of measuring the shell's own code rather than a copy of it, and the gate builds both
anyway.

Encoded by `crates/ono-cli/tests/completion.rs::should_read_its_budgets_from_the_limits_catalogue`,
`::should_stop_discovery_at_the_hard_budget_and_answer_what_it_has`,
`xtask/tests/perf.rs::should_measure_the_completion_budget_directly_rather_than_through_a_proxy`,
and acceptance case `198`.

## Alternatives considered

**Keep timing the proxy and widen its assertion.** It measures registry lookups. No amount of
tightening makes that a measurement of a provider read.

**Measure through a pseudo-terminal from `xtask`.** It would include the editor's redraw, which is
arguably the figure a user feels, and it needs a PTY driver in the automation plus a way to tell a
completion's paint from the prompt's. Calling the completer measures the work §36.2 budgets, with
no terminal in the way.

**Assert the 150 ms hard budget in `cargo test`.** ADR-0252's comment — "a wall-clock assertion
that tight is flaky on shared hardware" — is still true. The deterministic outcome test asserts
the budget *works* (a one-nanosecond budget admits no discovery and answers with what it had), and
the millisecond figure lives on the reference environment where §32.4 puts it.

**Leave the 40 ms constant and change the catalogue to match it.** It would also give one home for
the number, and the home would be the wrong one: Appendix A is normative and 50/150 is what it
says.
