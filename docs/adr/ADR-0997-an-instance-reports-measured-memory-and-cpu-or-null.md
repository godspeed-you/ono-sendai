# ADR-0997: An instance reports the memory and CPU time the host measured, or null

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §10.4, §10.5, §10.6, §31.8, §31.33, §35.3; v0.4.1 §17.2; ADR-0283, ADR-0569,
  ADR-0996; issue #222
- Decided by: agent (autonomous)

## Context

Spec §31.8's `get plugin` table shows a `MEMORY` column, and §31.33 asks the host for an instance's
`memory/current/limit` and its `cpu time`. `inspect plugin` already reported `memory_current` and
`cpu_time` from the supervisor's samples (ADR-0283), but `ono.plugin/1` — what `get plugin` answers
— set `memory` to null for every package, loaded or not, and had no CPU figure at all. A running
instance looked exactly like one nothing had loaded.

What the host can measure depends on the tier:

- **`native-confined`** — the supervisor samples the process from `/proc` on every frame and on a
  timer: `VmData`, the figure `RLIMIT_DATA` bounds, and `utime + stime` from `/proc/<pid>/stat`,
  counted by the kernel in clock ticks.
- **`wasm`** — wasmtime passes every growth of a component's memory through the host's limiter,
  so the gauge is exact. A component runs on the shell's own runtime threads, and wasmtime keeps
  no per-instance CPU account: the epochs the supervisor uses to preempt a spinning component
  count deadlines, not time used, and a thread's CPU clock mixes the component with everything
  else on that thread.

## Decision

**`get plugin` reports what the host measured, typed, and null where there is no measurement.**
The schema changes once with issue #199: `ono.plugin/2` (ADR-0996) carries both.

- `memory` (`bytesize`) is the instance's current allocation as the supervisor last sampled it —
  the kernel's `VmData` for a process, the runtime's gauge for a component. Null for a package with
  no instance and for an instance not yet sampled.
- `cpu_time` (`duration`, new) is the CPU time the instance has used — `utime + stime` for a
  process, at the kernel's tick resolution, so an instance that has done next to nothing reads
  `0ns`, which is a measurement. Null for a package with no instance, for an instance not yet
  sampled, and for a `wasm` component, whose CPU time nothing measures.
- Both are the figures `inspect plugin` reports as `memory_current` and `cpu_time`, read from the
  same samples, so the two commands cannot disagree. The reason a figure is null is documented on
  the field in both contracts (`plugin.v2.yaml`, `plugin-inspection.v1.yaml`).
- Unknown is null, never zero (spec §35.3); a figure is never derived from a guess such as the
  ceiling or the time the instance has existed.

`cpu_time` is an optional field added to the version ADR-0996 introduces, so the record changes
version once. The default view keeps spec §31.8's columns (`id, version, state, trust, jobs,
memory`).

## Consequences

Easy: the `MEMORY` column of §31.8 shows something for a running package; a script can sort packages
by what they use. `memory` and `cpu_time` are typed values, so `where memory > 100MiB` and unit
arithmetic work on them (§10.6).

Hard:

- A component's CPU time stays null until the runtime can account it per instance — measuring it
  would need the supervisor to time every call into the component on its own thread (a
  supervisor change, not a reporting one).
- The figures are samples: between two frames of an idle instance they are as old as the sampling
  interval.

Encoded by `crates/ono-cli/tests/plugin_record.rs`:
`::should_report_the_memory_and_cpu_time_of_an_instance_that_has_run`,
`::should_report_no_memory_or_cpu_time_for_a_package_nothing_has_loaded`,
`::should_report_a_components_memory_and_no_cpu_time_when_it_is_inspected` (an expected skip
without the `wasm32-wasip2` target); the generated
`crates/ono-cli/tests/provider_conformance.rs::should_shape_ono_plugin_2_the_way_ono_shell_declares_it`;
acceptance case `156-a-plugin-instance-reports-its-memory-and-cpu-time`.

## Alternatives considered

**Report the component's thread CPU time.** The thread runs the shell's other work too; the number
would be a fabrication with a unit attached.

**Report zero where nothing is measured.** Spec §35.3 forbids it, and a zero reads as "uses nothing".

**Peak memory instead of current.** §31.33 asks for `current`, and `inspect plugin` already shows
the limit beside it; the supervisor's peak stays an internal figure for classifying a death.
