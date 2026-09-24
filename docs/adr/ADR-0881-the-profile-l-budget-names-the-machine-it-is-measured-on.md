# ADR-0881: The Profile L budget names the machine it is measured on

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §2.7 (tests report execution truth), §32.4 ("release qualification MUST run
  on a named reference environment"), §33.2, §33.3, §38.1, §38.2, §38.4; AGENTS.md §11
- Issues: #166
- Relates to: ADR-0431, ADR-0489, ADR-0494, ADR-0496, ADR-0513, ADR-0514, ADR-0517
- Decided by: agent (autonomous)

## Context

`crates/ono-cli/tests/spatial_first_output.rs::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`
places Profile L's hundred thousand listening sockets and requires
`enter network; map --live --json | take 1 | to json` to say something within §33.3's 30 s.
ADR-0431 and ADR-0517 keep that budget unscaled on purpose: the duration is the observation, and a
budget that stretched with the load would measure nothing.

Issue #166 recorded what that costs: the test failed 4 of 6 workspace runs at a load average of
22–26 on eight processors and 0 of 12 at 9–13. Reproduced for this ADR on the same machine, at
`fe1508dc`: alone, beside the other agents' builds (load 17–28), it passes in 6.5–36 s; with 24
extra busy loops (load 25–37) it failed 1 of 2, and with 48 (load 47–66) 2 of 2.

The exit test asks for one of two things: the live map answers inside 30 s at load 25, or the case
names the machine its budget is measured on.

**Is the live map slow for a product reason?** Profiled on a debug build at load 39 against the
same hundred thousand sockets (`perf` is unavailable to an unprivileged user here, so by sampling
stacks with `gdb`): the run takes 9.5–11 s of wall time and 5.7 s of CPU. `get socket | count`
alone is 3.9 s, spent in `ono_provider_netlink::provider::collect` reading the table; the plain
`map --json` at `network` is 9.4 s and the live one 8.8 s, and the difference is spent projecting
each socket into the spatial index — `Projection::project_as` and `ProviderBridge::absorb`, a
`BTreeMap` insert per object. That is linear work over a hundred thousand objects in an
unoptimised build, with no quadratic step, no retry and no wait in it. Making it faster means not
reading the whole table before orienting — §34.4's incremental neighbourhood — which is v0.4.1's
performance work, not this milestone's verification work, and it would be a `perf` increment with
a benchmark of its own. So the product half of the exit test is not taken here, and the second
half is.

## Decision

**The budget is measured on a machine whose one-minute load average stays within 1.5 times its
processor count while the map runs. The test reads the machine before and after the run, judges
by the busier reading, and names the machine in whatever it reports.**

- **The run answers within 30 s:** the test passes, on any machine.
- **The run is silent for 30 s on a machine inside the envelope:** the test fails, and the
  message names the load and the processor count, so the failure is a statement about the shell
  on a machine that could have answered.
- **The run is silent for 30 s on a machine outside it:** the test announces
  `SKIP(fixture_not_applicable)` — "Profile L's live map produced nothing within 30s on a machine at
  a load average of 50.59 on 8 processors, above the 1.5x its processors that the budget is
  measured on (ADR-0881)". That is not a verdict about the shell, and §2.7 forbids reporting it
  as one.

The budget itself is untouched: 30 s, unscaled, as ADR-0431 decided. What changed is which
outcomes the test is entitled to call a defect.

**1.5**, because that is where the evidence stops. Twelve of twelve runs answered at 9–13 on eight
processors, up to 1.6 per processor; four of six did not at 22–26, from 2.75. The constant is
`REFERENCE_LOAD_PER_PROCESSOR` beside the test, with that sentence.

**Judged by the busier of two readings,** because the one-minute average lags: a load that
arrives during the run is seen at its end, and one that leaves during it at its start. A run
judged by the quieter reading would fail on a machine that was busy while it ran.

**The skip is declared.** The test gains a `fixture_not_applicable` row in `declared:`, beside
its `missing_privilege` one, and the condition joins its existing `canonical_ci.permitted_skips`
entry. The canonical CI runner is expected to be inside the envelope, and there CI runs the
budget assertion exactly as before; a runner that is not says so in the skip marker, with its
load and processor count. Whether a shared runner is busy is a property of the runner,
so `skip-check` neither requires nor forbids the skip (ADR-0517).

## Consequences

Easy: a busy machine produces a skip that names how busy it was, instead of a red result that
names the shell. With 48 extra busy loops the test skipped 3 of 3 at load 50–66; with the envelope
temporarily raised to 100 per processor, the same machine failed it with the load in the message —
the assertion is live wherever the machine is inside the envelope.

Hard: on a machine above 1.5 per processor, §33.3 at Profile L goes unchecked for that run. The
development machine often is — six agents building beside each other held it at 17–30 on eight
processors while this was written. That is the cost the issue names, now visible in the log
rather than hidden in a red result, and it is bounded: a run that answers still passes. Case
`197` measures §33.3 in the acceptance container, on a machine the case owns, but at Profile M;
Profile L has no acceptance counterpart, so on a busy developer machine this test is the only
place that would have noticed, and it now says that it could not.

Also hard: load is a proxy for "free CPU", and not a perfect one. A machine whose load is I/O wait
reads as busy while it has cores to spare, and a load average reads the whole machine rather than
this test's share of it. Both errors point the same way — towards a skip on a busy machine — and
neither can turn a machine inside the envelope into a skip.

Revisit when §34.4's incremental neighbourhood lands: a live map that no longer reads the whole
socket table first would answer at Profile L on a far busier machine, and the envelope could
widen or go.

Encoded by the test above and by its rows in `docs/contracts/hardening/expected_test_skips.yaml`.

## Alternatives considered

**Scale the 30 s with the load, as `Shell`'s watchdog does.** ADR-0517 names this budget as the
one that must not scale: here the duration *is* the observation, and a budget that grows with the
load stops measuring §33.3 at exactly the moment the machine is busy enough to matter.

**Retry a silent run.** A retry on the same busy machine answers the same way, and a retry that
passes is a flake nobody investigates (ADR-0517).

**Measure the machine's own speed with a calibration run and scale the budget by it.** A second
measurement to correct the first, on a machine whose load changes within the run; the two would
disagree for the same reason the test did. The load average is what `ono_testkit` already reads
for its watchdogs, and a reader can check it in one command.

**Make the live map faster.** Real, and out of scope for a verification milestone; see Context.
