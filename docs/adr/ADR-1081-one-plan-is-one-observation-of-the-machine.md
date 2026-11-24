# ADR-1081: One plan is one observation of the machine

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.6 §2.1, §4.3, §7.2, §9.3, §9.6, §33.2 (v0.4); v0.2 §34; ADR-0880, ADR-1080
- Issues: #140
- Decided by: agent (autonomous)

## Context

Planning a restart cost about 0.75 s per target (issue #140): `get service | take 50 | plan
restart service` took 38.5 s, linear in the target count. Each target is frozen and then *placed*
(`change::world::placed`): its v0.4 neighbourhood is read with a revalidating interest so the
impact walk (§9.3) has something to walk. That expansion built a fresh set of kernel relationship
providers per target, and the providers' shared snapshots (`ono_graph`'s one-observation cache,
ADR of the trace work) lived only as long as that set. So every target

- enumerated the whole process table (`ServiceProcesses`) and read every process's
  `/proc/<pid>/cgroup` to find the unit's members, and
- enumerated every unit with its properties (`ServiceDependencies`) to resolve its dependencies.

### Measured first

Debug build, the fixture service manager of ADR-1080 (`tests/support/service_manager.rs`; every
unit but the first `Wants` the first), `ono -c 'get service | plan restart service | to json'` in a
scratch XDG home, three runs per size, on the run's host (i7-6600U, 2 cores/4 threads, load
average 5–11 from the parallel workstream's builds). Requests are counted at the fixture's D-Bus
boundary per plan; syscalls by `strace -f -c -e trace=openat,readlink,sendmsg` of one plan.

| units | wall-clock before (s) | after (s) | `ListUnits` per plan before → after | unit reads per plan before → after | `openat` before → after | `readlink` before → after | `sendmsg` before → after |
|---:|---|---|---|---|---|---|---|
| 1  | 1.37 / 0.56 / 0.48 | 0.54 / 0.26 / 0.26 | 1 → 1   | 5 → 5       | 2,568 → 2,553   | 1,355 → 1,352  | 29 → 29     |
| 5  | 1.92 / 2.08 / 1.14 | 0.45 / 0.42 / 0.46 | 5 → 2   | 45 → 30     | 11,533 → 2,564  | 6,731 → 1,358  | 129 → 96    |
| 10 | 5.42 / 4.29 / 5.53 | 0.70 / 0.53 / 0.63 | 10 → 2  | 140 → 60    | 22,493 → 2,569  | 13,292 → 1,361 | 344 → 176   |
| 25 | 12.1 / 13.9 / 14.2 | 1.31 / 1.14 / 1.11 | 25 → 2  | 725 → 150   | 56,703 → 2,644  | 33,788 → 1,406 | 1,589 → 416 |
| 50 | 29.2 / 41.9 / 38.2 | 1.61 / 1.95 / 3.33 | 50 → 2  | 2,700 → 300 | 114,876 → 2,707 | 68,528 → 1,436 | 5,664 → 816 |

On the host's own systemd (≈800 units, ≈600 processes; read-only, scratch XDG home),
`get service | where unit_file != null | take N | plan restart service | to json` under strace:
N = 5: 11.5 s → 4.7 s; N = 50: 166.6 s → 9.2 s, `openat` 118,294 → 2,633, `readlink` 70,682 →
1,397, `sendmsg` 38,117 → 1,592.

What remains per target is its own unit — frozen once and revalidated once (§4.3, §33.2), two
named reads — and is linear by design.

## Decision

**A plan is one observation of the machine.** Every target of one plan is placed with one shared
set of relationship providers, so the process table, the control groups and the unit list that
the neighbourhoods are read from are read once per plan rather than once per target.

- `spatial::relations::SharedRelations` is that set (the kernel relationship providers plus
  `ProcessUsers`, exactly what one expansion built before); `relations::observe_within` expands a
  place with a given set, and `relations::observe` is unchanged for every other caller (a fresh
  set per call, as before).
- `change::plan::freeze_all` makes one set per plan and `change::world::freeze`/`placed` use it;
  `rebase` (§7.5) makes one per rebase, because it refreezes every target of one plan.
- `ono_graph`'s shared snapshots also memoise each process's control groups for their lifetime,
  so `ServiceProcesses` reads `/proc/<pid>/cgroup` once per observation instead of once per unit
  expanded.

The single read is a snapshot *of the plan*: nothing outlives the plan's construction, and nothing
about `apply` changes. §7.2's preconditions are still checked against the machine as it is when
`apply` runs, by the same code (`world::revalidate`, the executor's precondition check). The
tests that hold that — `change_revalidate.rs` (6, among them
`should_stop_the_apply_when_the_planned_process_has_gone`,
`should_refuse_to_write_through_a_symlink_swapped_in_after_the_seal`),
`change_revalidate_seal.rs` (2), `change_prepare_failure.rs` and `change_inspect.rs` — pass
unchanged.

## Consequences

- The plan is the same plan: for 1, 5 and 10 units the sealed plan (ids, timestamps and digest
  normalised) is byte-identical before and after — same targets, identities, actions, effects,
  preconditions, risk and ordering. For 25 and 50 units the old plan was not deterministic: three
  runs of the same plan reported 46/38/38 and 38/18/28 impact boundaries, where each target, alone,
  reports two (`its cgroup`, `its listeners`). Every target's expansion re-read and re-absorbed
  the whole unit list, which erased what earlier targets had recorded about their exits; the new
  plan reports 50 and 100 on every run, the sum of its targets. Everything else is identical. On
  the host the same erosion had hidden 40 of 46 dependents of a fifty-unit restart.
- `crates/ono-cli/tests/change_planning.rs::should_read_the_unit_list_once_per_plan_however_many_targets_it_freezes`
  is the benchmark that stays in the suite: it counts requests at the fixture's boundary and holds
  the unit list to one enumeration per plan and a target to a constant number of unit reads
  (before: 49 `ListUnits` for fifty targets and 2,650 unit reads; after: 1 and 300 — the test
  fails against the old code).
- Suite cost on this host: `change_gates.rs` 56 s → 20 s for its twelve tests (it had been 94–99 s
  per test against the host's units), `change_views.rs` 8.7 s → 5.5 s (40 s for its bulk test
  against the host).
- `crates/ono-cli/src/spatial/relations.rs` (owned by the spatial work) gained `SharedRelations`
  and `observe_within`; `observe`'s behaviour is unchanged.

## Alternatives considered

- Reading every unit once with `get service` and freezing from those records — freezing is §4.3's
  resolution of each selector against the provider *now*, and handing it the pipeline's input would
  freeze what the pipeline saw rather than what the provider answers.
- Skipping the neighbourhood read for bulk plans — the impact graph and §9.6's boundaries would
  then depend on the target count.
- A cache with a time-to-live in the spatial session — it would outlive the plan and leak an old
  observation into the next one; the shared set lives exactly as long as the plan's construction.
