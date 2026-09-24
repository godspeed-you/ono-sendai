# ADR-0884: A pathological budget is relaxed only on a machine loaded in the same run, and says it was not adjudicated

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §34; v0.4.1 §2.7, §32.4, §37.4
- Issues: #189
- Supersedes: ADR-0882
- Relates to: ADR-0431, ADR-0517, ADR-0883
- Decided by: agent (autonomous)

## Context

ADR-0882 judged every row of `docker/acceptance/cases/152-pathological-sockets.case` in two steps:
the median against the budget, and — whenever that missed, on any machine — the pathology's
fastest run against the ordinary host's, allowed half the budget on top, printed as
`within budget`. The independent review of v0.6.2 showed what that admits: on an idle machine,
an ordinary fastest run of 35 ms and a pathological fastest run of 60 ms — every run over the
50 ms budget — passed as "within budget". Reproduced: with 18 ms added to the pathological socket
row, the ADR-0882 case printed `first-socket-row: within budget on this machine` twice at load
18–20, with medians of 56 and 63 ms.

The review found two older defects in the same case: `stdout-contains: first-socket-row: within
budget` was also satisfied by the `baseline-first-socket-row: within budget` line, and every timed
`ono` run discarded its exit status, so a run that failed at once read as fast — replacing the
pathological cold-start statement with `exit 3` still printed `cold-start-under-load: within
budget`.

## Decision

**A row is judged in up to three steps, and the relative one exists only on a machine
demonstrably loaded in the same run; a relative pass is called what it is.**

1. Median within the budget: `within budget`.
2. Otherwise, on a machine that was **not** demonstrably loaded: the fastest run must be within
   the budget — `within budget at its fastest run`. A row whose every run is over the budget is
   `OVER BUDGET`, whatever the ordinary host costs.
3. Otherwise, only on a machine demonstrably loaded — its one-minute load average at or above its
   online processors (`cpuN` lines of `/proc/stat`, host-wide like the load average) **both**
   before the ordinary host is measured **and** before the pathological host is — the pathology
   may add at most half the budget to the ordinary host's fastest run of the same statement:
   `not adjudicated on this machine`, with the load, the processors and the figures. More than
   half the budget added is `OVER BUDGET` on any machine. The ordinary host's own row has nothing
   to be compared with; on a loaded machine its miss is `not adjudicated` as well.

Every verdict line starts with `VERDICT <row>:` and the case asserts it with a line-anchored
`stdout-matches: ^VERDICT <row>: (within budget|within budget at its fastest run|not adjudicated
on this machine)`, so no row's assertion can be satisfied by another row's line. Every timed run
must exit 0; a run that did not is printed as `RUN FAILED` and fails the case.

## Consequences

Measured in the acceptance image built from this tree (removed afterwards):

- 18 ms added to the pathological socket row, with the load gate forced to "not loaded": `OVER
  BUDGET` twice (fastest 54 and 55 ms), where ADR-0882 passed it.
- 40 ms added on the real, loaded machine (load 31 on 8): `OVER BUDGET` (adds 47 ms). 25 ms added
  at load 25: `OVER BUDGET` (adds 28 ms).
- `exit 3` in place of the cold-start statement: `RUN FAILED` for all 21 runs.
- Unmodified: green twice through `scripts/acceptance.sh` at load 15, and with 32 extra busy loops
  (load 26–35) every row `within budget`.

What remains relaxed is deliberate and visible: on a machine loaded at or above one per processor
at both readings, a pathology that adds no more than half the budget is not adjudicated, and the
line says so rather than claiming the budget was met. The GitHub runner and a quiet developer
machine are below that gate, and there every row is held to the budget by its median or its
fastest run.

## Alternatives considered

**Keep ADR-0882 and only rename the relative pass.** The idle-machine admission is the defect;
a better name does not remove it.

**Hold every row to its median on an unloaded machine.** That is issue #189 again — load 6.8 on
eight processors is not loaded by this gate, and 51 ms medians there are scheduling. The fastest
run is the product's cost without the scheduling, and it must itself be within the budget.

**Decide "loaded" from the ordinary host's own figures.** A slow ordinary host is also what a
regression in the shell looks like; the load average is a reading about the machine, not about
the product.
