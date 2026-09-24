# ADR-0883: A silent Profile L map on a busy machine is asked again before it is excused

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §2.7, §33.3, §38.1, §38.2, §38.4; AGENTS.md §11
- Issues: #166
- Supersedes: ADR-0881
- Decided by: agent (autonomous)

## Context

ADR-0881 let `spatial_first_output.rs::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`
skip instead of fail when Profile L's live map stayed silent for §33.3's 30 s on a machine loaded
above 1.5 times its processors. The independent review of v0.6.2 found three ways that rule turns
a hang into a skip:

1. **The load was the busier of a reading before and after the run.** The reading after includes
   the load the silent map and the hundred-thousand-socket fixture produced themselves, so the
   subject's own work could push its machine out of the envelope.
2. **Nothing distinguished a hang from a busy machine.** Once outside the envelope, silence was
   excused whatever caused it — and 1.5 × 8 = 12 sits inside the gate's ordinary load of 9–13, so
   ordinary gate runs could turn a genuine hang into a skip.
3. **The processors were counted with `available_parallelism`,** which follows this process's
   affinity and cgroup quota, against `/proc/loadavg`, which averages the whole host. In a
   container limited to two processors on a busy host every run would be outside the envelope.

## Decision

**The envelope decides whether a silent map may be asked again, and only a second answer excuses
it.**

- The machine is read **before** the map runs: the one-minute load against the host's **online
  processors** as `/proc/stat` lists them (`cpuN` lines) — the same population `/proc/loadavg`
  averages over. `available_parallelism` is only the fallback where `/proc/stat` is unreadable.
- An answer within 30 s passes on any machine.
- Silence on a machine inside 1.5 × its online processors fails, as before.
- Silence on a busier machine is **run again once with three times the watchdog** (90 s). If it
  then answers, the map was slow on a machine the budget is not measured on, and the test
  announces `SKIP(fixture_not_applicable)` naming the load and saying it answered late. If it is
  still silent, the test fails: a map that says nothing for two minutes is hung however busy the
  machine is.

The 30 s budget is unchanged and unscaled (ADR-0431, ADR-0517). The second run is not a retry of
the assertion — it cannot turn a failure into a pass, only an unanswerable silence into either a
skip that names the machine or a failure.

The decision is a pure function of the first outcome, the machine and the second outcome, and is
tested deterministically in the same file with readings the test chooses (idle and busy machines,
answered and silent runs), as is the `/proc/stat` count.

## Consequences

A hang now fails on every machine; only a map that demonstrably answers when given time is
excused, and only above the envelope measured before the run. The cost is up to 90 s more on a
busy machine where the first run was silent. Measured: with 48 extra busy loops (load 46–106) the
test skipped 3 of 3 after the second run answered (93–139 s in total); at the machine's background
load it passed 5 of 5.

## Alternatives considered

**Keep the busier-of-two reading.** It measures the subject's own load, which is the defect.

**Pressure stall information (`/proc/pressure/cpu`).** A better signal where present, but not on
every kernel the suite runs on, and the second run already decides the case the envelope cannot.

**Scale the budget instead.** ADR-0517 names this budget as the one that must not scale.
