# ADR-0787: A death at the memory ceiling is read from the kernel's own high-water mark

- Status: accepted
- Date: 2026-09-09
- Spec refs: §31.15, §31.34; `docs/contracts/kuang/errors.v1.yaml` (K11201, K11203)
- Decided by: agent (autonomous)

## Context

Spec §31.34 lists the failure classes a plugin death can belong to, and puts two of them side by
side:

> ```text
> trap/crash
> …
> resource limit
> ```

The error contract gives each its own code and its own promise to the reader.
`Ono-Sendai-K11203` (`runtime.memory_limit`) is *"The plugin instance exceeded its memory ceiling
and was terminated"*; `Ono-Sendai-K11201` (`runtime.trap`) is *"The plugin instance trapped or
crashed"* and names nothing.

Linux gives the host no way to tell them apart from the death itself. `RLIMIT_CPU` raises
`SIGXCPU` and `RLIMIT_FSIZE` raises `SIGXFSZ`, so those limits name themselves; memory does not.
`RLIMIT_DATA` makes the allocation that would cross the ceiling *fail*, and what the package does
next is the package's own business — a Rust artifact calls `abort()`, so the host sees `SIGABRT`,
which is also what an assertion, a panic and a deliberate crash look like. The class therefore has
to be inferred from how much memory the instance held when it died, and the quality of that
inference is entirely the quality of that one figure.

Until this decision the figure was the host's own periodic sample of `/proc/<pid>/status`
`VmData`, taken every 100 ms (and on every inbound frame), with "at the ceiling" read as *within a
sixteenth of it* (ADR-0283). That made the classification a fact about the machine's load rather
than about the run. Two tests recorded it:

- `crates/ono-cli/tests/plugins.rs::should_end_the_instance_and_not_the_shell_when_a_package_exceeds_its_memory_ceiling`
- `crates/ono-kuang-sdk/tests/failure_classes.rs::should_distinguish_a_launch_failure_from_a_quarantine_a_resource_kill_and_a_crash`

Run alone, both passed. Run inside a full `cargo test -p ono-cli --test plugins` — 23 tests over
8 threads — the first failed in two runs out of three, reporting

```text
Ono-Sendai-K11201 runtime.trap
the plugin instance was killed by signal 6; it had allocated 62611456 bytes of its
67108864 byte ceiling when last observed
```

62 611 456 of 67 108 864 is 93.30 % — four hundredths of a percent under the sixteenth the rule
asked for. The fixture allocates a mebibyte every 20 ms, so a 100 ms sampling interval is worth
5 MiB of climb, and the last observation lands anywhere in the 5 MiB below the ceiling while the
margin only reaches 4 MiB up from it. The margin and the sampler were arguing about a number
neither of them measured.

Widening the margin until that particular run fits is the one thing this must not be: the number
would then be a property of one fixture's pace on one machine, and the next fixture, or the next
loaded machine, restarts the argument.

## Decision

**The host classifies a death at the ceiling from the kernel's own accounting for the ended
process, not from its last sample of the living one.**

Concretely, in `crates/ono-kuang-supervisor/src/supervisor.rs`:

1. When a native instance ends, and *before* it is reaped, the host asks the kernel for its
   accumulated `rusage` with `waitid(P_PID, pid, WEXITED | WNOWAIT, &rusage)` — the raw Linux
   syscall, because `wait4` rejects `WNOWAIT` and no libc wrapper passes `rusage` out of
   `waitid`. `WNOWAIT` leaves the zombie in place, so the ordinary `Child::wait` still reaps it
   and still reports the exit status. `ru_maxrss` is the kernel's high-water mark for that
   process, in kibibytes.
2. The figure the classification uses is the **larger** of that high-water mark and the host's
   own sampled `VmData` peak. Each is the smaller truth on its own: the sample bounds the memory
   `RLIMIT_DATA` bounds but only as recently as the sampler last looked, and `ru_maxrss` is exact
   and load-independent but counts only pages the instance actually touched.
3. The threshold is unchanged: at the ceiling means **within a sixteenth of it**, the rule
   ADR-0283 stated and its unit tests fix. It is not retuned, because the defect was never the
   threshold — it was that the figure compared against it had a 100 ms hole in it. The sixteenth
   now covers what it was always meant to cover: a ceiling that bounds *allocated* pages measured
   against a figure that counts *touched* ones.
4. A `runtime.memory_limit` error carries the ceilings its contract promises —
   `declared_memory_max` (the package's manifest), `effective_memory_max` (that capped by host
   policy, the ceiling actually in force) — beside the existing `resource_class: memory` and the
   `observed_memory_peak` the decision was taken on.

Everything else stays `runtime.trap`. The rule converts no trap that the host cannot show a
figure for: an instance that never came near its ceiling, and an instance the kernel says nothing
about, are crashes.

**Why this margin.** It is not chosen for a test; it is inherited unchanged, and the change is to
the evidence rather than to the rule. What makes the classification deterministic now is that the
evidence is: for the fixture that reaches its 64 MiB ceiling, `ru_maxrss` is 66 276 KiB — 101.1 %
of the ceiling — under load and unloaded alike, and with the fixture's pacing on or off. The
sixteenth is doing the job it was written for, which is to absorb the gap between a ceiling on
allocation and a measurement of residency, not to guess at a sampler's blind spot.

**What the rule still refuses to claim.** An instance that reserves memory it never touches, and
then dies, leaves a small `ru_maxrss`; if the sampler also missed it, the host reports
`runtime.trap`, because it has no evidence of a ceiling. Under-claiming is the right direction:
spec §35.3 forbids fabricating an unknown, and a host that guessed would report every `abort()`
as an out-of-memory.

## Consequences

- The two flaky tests above are load-independent, and so is any future one: the figure the
  classification rests on no longer depends on when a sampler looked.
- `crates/ono-kuang-sdk/src/bin/kuang-example-plugin.rs` no longer needs to pace itself for the
  classification's sake. The `--pace-ms` option stays — it is how a suite asks for a climb the
  host's sampler *cannot* see, which is what the new test uses — but the comment claiming the
  pace is what makes the class provable is corrected.
- One `unsafe` block enters `crates/ono-kuang-supervisor/src/sandbox.rs`, beside the `sysconf`
  call already there, with a `// SAFETY:` note per AGENTS.md §16. It crosses no crate boundary.
- The reader of a `waitid` that blocks: it blocks exactly as long as the `Child::wait` that
  follows it would have, and runs on a blocking thread so it does not hold a runtime worker.
- Non-Linux targets get a `None` from `resident_peak_at_exit` and fall back to the sampled peak,
  which is what they had before.
- Encoded by `crates/ono-kuang-sdk/tests/memory_ceiling.rs`:
  `should_name_the_ceiling_when_an_instance_traps_with_its_memory_at_the_ceiling` and
  `should_still_name_a_trap_when_an_instance_dies_nowhere_near_its_ceiling`.

## Alternatives considered

- **Widen the margin.** Rejected: the width would be a property of one fixture's allocation pace
  and one machine's load, tuned until a test passed, and it would make `runtime.memory_limit`
  progressively mean "died with a lot of memory".
- **Extrapolate the unobserved window from the observed growth rate.** Deterministic given the
  observations, and it would have caught the 62.6 MiB case; rejected because it still fails the
  case that matters most — an allocator that reaches the ceiling inside a single sampling
  interval was never observed growing at all — and because it would classify a plugin that
  crashes at 40 MiB while growing fast as one that hit a 64 MiB ceiling.
- **Sample more often.** Same rule, shorter blind spot, still a blind spot; and it spends a
  `/proc` read per instance per interval to buy nothing but odds.
- **A cgroup v2 `memory.max` with its `memory.events` counters,** which would make the refusal
  itself observable rather than inferred. This remains the exact answer and remains unavailable:
  it needs a delegated cgroup the shell does not have as an unprivileged user, as ADR-0283
  already recorded.
- **`wait4` for the `rusage`, reaping the child ourselves.** Rejected: tokio's `Child` would
  still try to reap a pid it no longer owns, and a recycled pid makes that a hazard. `WNOWAIT`
  reads the accounting and leaves the ownership where it was.
