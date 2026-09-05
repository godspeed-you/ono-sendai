# ADR-0490: The benchmark command names its machine, its temperature and its iterations

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §32.2, §32.3, §32.4, §36.1, §37.1, §37.2, §37.3, §37.4, §61.1, Appendix F;
  ADR-0252, ADR-0431, ADR-0459, ADR-0488, ADR-0489; issues #8, #21, #71, #84
- Decided by: agent (autonomous)

## Context

§37 asks for three things that are usually treated as one. §37.1 wants benchmark execution to be
*discoverable and reproducible*. §37.2 wants the machine named — CPU and cores, RAM, kernel,
distribution, toolchain, release build flags — because §32.4 puts release qualification on a named
environment with absolute targets. §37.3 wants cold, warm and cache-hit told apart, and states the
consequence: *"A warm-cache number MUST not be advertised as cold performance."* §37.4 wants at
least twenty iterations and says *"Single-run best-case timings MUST NOT define release success."*

ADR-0489 built the record and the comparator and left the runner and the environment to this
increment. Three standing debts in this repository were waiting for exactly this file:

- **ADR-0252** accepted a 1 000-iteration in-process proxy for §34's 50 ms completion budget,
  because *"a wall-clock assertion that tight is flaky on shared hardware"*. Issue #21 is still
  open on it.
- **ADR-0431** chose §33.3's thirty-second floor over §33.2's 500 ms target, saying the targets
  *"would be a coin toss on a shared runner the day they went green"*.
- **ADR-0459** measured cancellation — a hundred cancellations in 0.07 s — and asserted none of it,
  naming issues #83 and #84 as what would unblock the figure.

All three are the same sentence: a number is only a measurement when the machine is named.

## Decision

**`cargo xtask perf` runs declared benchmarks against a declared cardinality on a named machine,
and every figure it writes says which machine, which temperature, and how many iterations.**

### 1. The environment is a registry, not a paragraph

`docs/contracts/hardening/performance_environment.yaml` carries §37.2's six facts for
`ryzen-3900x-ubuntu-2604`, and
`xtask/tests/perf.rs::should_name_the_reference_environment_on_every_recorded_figure` requires
every one of them to be stated and requires every record in the baseline to name that environment.
§37.2 says *"the release documentation MUST name"* the environment; documentation a test reads is
documentation that cannot go stale, which is the same argument §50.1 makes about repository
metrics.

The registry states one thing beyond §37.2's list, because it changes how the figures should be
read: this is eight cores of a twelve-core part, virtualised, and shared with the developer's
ordinary work. So the baseline records the one-minute load average of the run that wrote it. A
figure measured at load 5 and compared against a figure measured at load 0.5 is a comparison
somebody should be able to notice.

### 2. A benchmark holds the host at a declared cardinality

Each declared benchmark names a profile, and the runner builds ADR-0488's process and socket
populations for it, runs the benchmark, and drops them — killed and reaped — before the next one.
§32.2 permits synthesis and forbids bypassing the code under measurement; the benchmark runs the
real `ono` binary against a real host, so nothing is bypassed at all.

### 3. Warm is measured, not asserted

A warm benchmark declares a warm-up, and the runner builds the script as
`<warmup> | count; echo ONO-PERF-MARK; <script>`. The clock for time-to-first-value starts on the
byte carrying the marker. That is the only point at which "the process is now warm" is observable
from outside the process, and measuring from outside is what keeps the figure about the product
rather than about an instrumented build.

It works, and the first run says so: `spatial.look` is **31.5 ms cold and 0.98 ms warm** at
Profile S. Two numbers thirty times apart, which is precisely why §37.3 forbids advertising one as
the other.

Temperature is part of the baseline key rather than a field beside it
(`Baseline::record_at`), so a warm figure has no cold record to be compared against and the
comparison answers `Unmeasured` instead of holding.

### 4. §37.4's floor is enforced where a release is qualified

`Tolerance::Absolute` is release qualification, and a record below `MIN_ITERATIONS` compared at
that tolerance answers `Comparison::Underpowered` rather than `Held`. *"Single-run best-case
timings MUST NOT define release success"* is a MUST, so it is a code path rather than a habit.

`--write-baseline` refuses a debug build for the same reason: §37.2 names the release build flags
as part of the environment, so a debug figure is a figure about a different build.

### 5. What the first run measured

Twenty iterations each, release build, Profile S, load average 4.97 — recorded in the baseline:

| Benchmark | Temperature | first (median) | p95 | complete |
| --- | --- | ---: | ---: | ---: |
| `shell.cold_start` | cold | 26.2 ms | 28.6 ms | 27.7 ms |
| `spatial.look` | cold | 31.5 ms | 33.7 ms | 34.1 ms |
| `spatial.look` | warm | 0.98 ms | 1.1 ms | 38.3 ms |
| `spatial.map_first_frame` | cold | 31.5 ms | 33.8 ms | 34.1 ms |
| `spatial.selector_miss` | cold | 1 087.6 ms | 1 153.6 ms | 1 087.6 ms |
| `process.enumeration` | cold | 50.2 ms | 51.7 ms | 52.9 ms |

The last-but-one row is issue #8, measured for the first time on a named machine: **a selector
miss is thirty-four times a hit**, and its p95 is already 1.15 s at Profile S against §36.1's
250 ms Profile M target. Issue #8's own report measured 1.40 s on a 920-process host and called it
"ten times a hit"; at Profile S with a hundred placed processes it is worse than that, which says
the cost is not in the population but in the sweep. That is the measurement #8's ADR needs, and it
exists now because this command does.

## Consequences

Easy: §33.2's four targets, §36.1's two and §23.3's cancellation distribution all have somewhere
to be measured that is not `cargo test`. ADR-0459's debt is writable: the runner already records
`cancel_ms` per benchmark, on the named environment, under §37.4's rule.

Hard: the reference environment is a shared developer machine, and it will stay one. Absolute
targets read off this baseline carry the load average of the run that produced them, and a
comparison at `Tolerance::Absolute` between runs at different loads will report regressions that
are the machine's. The honest mitigation is the one already in place — the figure names its
conditions — and the dishonest one, widening the tolerance until nothing fails, is what §32.4's
"stable absolute targets" forbids.

Also hard: `cargo xtask perf` is not in `scripts/gate.sh`. It takes minutes, builds populations,
and needs a release build; a gate that ran it would make every increment cost a benchmark run.
Comparison against the baseline is therefore an explicit `--compare`, which is what a release
qualification runs. `spec-check` still parses the baseline on every gate run (ADR-0489), so the
file cannot rot unnoticed.

Encoded by `xtask/tests/perf.rs::should_run_the_declared_benchmarks_and_write_their_records`,
`::should_name_the_reference_environment_on_every_recorded_figure` and
`::should_distinguish_a_warm_measurement_from_a_cold_one`.

## Alternatives considered

**A Rust benchmark harness — `criterion`, `divan` — instead of running the binary.** They measure
functions, and §32.3's first metric is time to *first value* of a shell command; §37.3's cold
startup is process startup. Both are properties of the binary, and neither survives being turned
into a function call. A harness would also add a dependency for something twenty lines of process
plumbing do.

**Measuring warm by running the query N times and dividing.** It reports an average that includes
the cold first run, which is the number §37.3 forbids. The marker measures the second run
directly.

**Putting `cargo xtask perf` in the gate.** It is the obvious way to stop regressions, and it
would make every increment pay for a benchmark run on a machine that is already shared with four
other agents. §32.4 permits CI to compare at a percentage tolerance; that is where this belongs
when there is a CI runner to put it on.

**Naming the environment after the machine's hostname.** It would drift the day the machine is
replaced, and every historical figure would silently change meaning. The id is a description of
the hardware and the distribution, which is what §37.2 asks to be named.
