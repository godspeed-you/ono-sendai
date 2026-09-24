# ADR-0882: A pathological budget is read against the same machine in the same run

- Status: superseded by ADR-0884
- Date: 2026-09-24
- Spec refs: v0.2 §34 (budgets, and "Performance tests SHOULD include pathological environments");
  v0.4.1 §2.7 (tests report execution truth), §32.4, §37.4
- Issues: #189
- Relates to: ADR-0431, ADR-0489, ADR-0517, ADR-0881
- Decided by: agent (autonomous)

## Context

`docker/acceptance/cases/152-pathological-sockets.case` measures §34's budgets on a host with
five thousand listening sockets: the first socket row, the first connection row, a cold start and
the whole socket table, each as the median of whole `ono` runs, plus the ordinary host's first
socket row for comparison. In a full 125-case run at load 6.8 it read 48 ms on the ordinary host,
then 51 ms and 56 ms on the pathological one, and reported both as OVER BUDGET. Run alone it
passed, and it passed on the GitHub runner. Issue #189's exit test: the case states what it does
when the machine cannot meet the budget, rather than reading a number two milliseconds from the
edge and calling it a defect.

Measured for this ADR in the acceptance image built from `fe1508dc`, on the eight-processor
development machine:

- In the container, `get socket | take 1` costs 35–38 ms of the 50 ms budget on the **ordinary**
  host at the machine's background load, against 5 ms for `ono -c true`. The row's margin is
  15 ms before any load arrives, and it is the product's, not the case's.
- With 32 extra busy loops (load 28–43) the old case went red in two runs of three — on the
  connection row (52 ms), and once on the ordinary host as well; with 56 (load 51–76) in three of
  three, on up to all three socket rows. The same statement measured twice in one run on the
  ordinary host read 62 ms and 129 ms: a median of twenty whole processes on a machine that busy
  is a reading of the machine.

A relative rule over medians — "the pathology may add half the budget to the ordinary host's
median" — was tried first and failed in the harness at the machine's background load: the
connection row's median read 87 ms against references of 51 and 35 ms taken seconds before and
after it. The medians move with the load *between* measurements, so any rule over medians is a
rule about when the load arrived.

## Decision

**Every row is judged in two steps, and says which one it passed.**

1. **Its median against the budget, exactly as before.** On a machine with headroom nothing about
   the case has changed.
2. **Only where the median misses: what the row costs at the fastest of its runs, against what
   the same machine costs in the same run without the thing the row is about.**
   - A pathological row compares its fastest run with the fastest run of the same statement on
     the ordinary host, measured both before the fixture opens its sockets and after it has
     closed them. The pathology may add at most **half the budget**.
   - The ordinary host's own row has no ordinary host to compare with; its fastest run must itself
     be within the budget.

The fastest of twenty runs is the figure a busy machine disturbs least: load adds waiting, and
waiting can only make a run slower. So the second step reads the product's own cost, and a
pathology that genuinely costs more than half the budget fails however quiet or busy the machine
is. What the second step gives up is the tail — a pathology that made most runs slow and left
one fast would pass it — and it gives that up only on a machine whose median already missed the
budget, where the tail is the machine's.

Each verdict line says which step it passed, with the figures:

```text
first-socket-row: within budget on this machine: the median is the machine's, and at its fastest
run the pathology adds 2ms to ordinary-first-socket-row's 33ms, within half the 50ms budget
```

A miss in the second step still prints `OVER BUDGET`, and the case's assertions are unchanged:
every row must say `within budget`, and nothing may say `OVER BUDGET`.

**Half the budget**, because it keeps the pathology's allowance independent of the machine and
still catches a pathology that costs a meaningful share of what §34 allows: an extra 40 ms added
to the pathological socket row was red in the second step at load 21, 47 and 79 alike.

## Consequences

Easy: at load 22–23, where the old case's socket-row median already read over 50 ms, the new case
passed on the second step with the pathology adding 2–11 ms. With 32 extra busy loops (load 26–37)
and with 56 (load 55–69) it passed every row, each saying which step it passed, while the old case
was red on one to three rows in the same conditions. Through `scripts/acceptance.sh` the case
passed twice of two at the machine's background load (19–25).

Hard: the second step does not measure the tail, as above. The case measures the product's own
cost on a busy machine and the budget itself on a quiet one; §32.4's reference-environment
figures are where the tail is judged.

Also: the ordinary host's first socket row costs 35–38 ms of its 50 ms in the container because
`get socket | take 1` answers only after reading the whole table — `get socket | count` costs the
same. That is a first-row cost of the provider, not of this case, and is recorded for filing.

## Alternatives considered

**Judge the pathological median against the ordinary median plus a tolerance.** Tried; red in the
harness at background load for the reason in Context.

**Raise the budget, or measure more runs.** A budget is §34's number and is not the case's to
move; more runs of a median on a busy machine converge on the machine.

**Skip on a busy machine.** The acceptance harness has no conditional skip — every expected skip
is expected in every run (ADR-0514) — and a case that skipped on a busy runner would stop
measuring §34 exactly where it is cheapest to be wrong.

**Alternate the fixture on and off and pair the measurements.** It narrows the window the load can
arrive in and does not close it; the fastest-run comparison does not depend on when the load came.
