# ADR-0232: A CPU share names the window it is a share over

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §28.1 (`cpu` — "percent of one logical CPU unless documented otherwise"),
  §10.4 (a unit is part of the contract), §35.3 (unknown is `null`, never fabricated),
  §9.1 (`get process`); v0.4 §26.2 (the high-CPU landmark)
- Decided by: agent (autonomous)

## Context

`cpu` was `null` in every one-shot invocation. `ono-provider-linux` computes a rate by
subtracting the previous observation of the same process from the current one, and a fresh
`ono -c '…'` has no previous observation, so:

```text
$ ono -c 'get process | where cpu > 0 | count'
0
```

on a host with a compile and two browsers running. `find place --where cpu > 5` was silence, and
v0.4 §26.2's high-CPU landmark could never fire outside an interactive session. The answer was
honest — a rate needs two readings — and it meant the plainest question anyone asks a shell,
*what is busy?*, could not be asked non-interactively. §28.1's own documented example is
`get process | where cpu > 20`, a single invocation.

There is no free rate. A meaningful window is at least a couple of hundred milliseconds: with
`USER_HZ = 100`, a 100 ms window resolves to one tick, which is 10% of one CPU, and a window short
enough to be free is dominated by scheduling granularity rather than by load. Paying that interval
on every `get process` would put it on `look`, `near`, `map`, every spatial reconciliation and
every acceptance case that enumerates processes.

But a single read is not empty either. The kernel states `utime + stime` and `starttime`, and
`/proc/uptime` states now — so one read answers a share of one logical CPU **over the process's
lifetime**. It is the `%CPU` of `ps(1)`, a real measurement of the same quantity in the same unit,
over a different window.

## Decision

**`cpu` is the share of one logical CPU over the window `cpu_window` names, and `ono.process/1`
carries `cpu_window` beside it.** The provider answers over the longest window it has been paid
for:

1. an earlier observation of the same process in this provider — a second `get process` in a
   session, a `watch`, or the reading `--sample` took — gives the interval between the two;
2. otherwise `uptime - starttime`, the process's whole life;
3. `null` for both fields only when the kernel gave neither, which on Linux means no
   `/proc/uptime`.

`cpu` alone would be ambiguous — 2% over half a second and 2% since Tuesday are different facts —
so the window is not documentation, it is a field. The two are null together and non-null
together.

**`get process --sample <duration>` buys the rate the lifetime average cannot give.** The provider
reads the CPU counters of the table it is about to enumerate, waits the interval, and answers
against that reading, so every row's window is the interval the caller chose. The invocation takes
at least that long, and it takes it only because it was asked to:

```text
$ ono -c 'get process --sample 400ms | where cpu > 1 | select name cpu cpu_window'
clickhouse-serv  33.78  444ms
claude           11.29  442ms
```

The interval is *asked for*, not inferred. Guessing it from the pipeline — "this statement
mentions `cpu`, so sample" — would make the accuracy of a number depend on how a question was
spelled, and would put the evaluator in the business of deciding what a provider measures.

## Consequences

- `get process | where cpu > 0 | count` answers 321 rather than 0 on this host, and
  `get process | where cpu > 20` — §28.1's example — is a question that can be asked. What it
  finds is what has been busy over its life, which is the right answer for a compile, a test run
  or a runaway loop, and an understatement for a long-lived server that just started working.
  `--sample` is the answer for that one, and `cpu_window` is what tells a reader which they got.
- Nothing became slower. A bare `get process` does one pass over `/proc` as before, plus **one**
  read of `/proc/uptime` per query. Reading it per *record* instead was the first implementation,
  and it cost five hundred extra opens per enumeration: enough to make
  `should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open` — a
  full-screen map refreshing live — take 50 s instead of 15 s and miss its budget. The uptime is
  read once per query, which is also the more correct answer: every process of one snapshot is
  measured over a window ending at the same instant.
- Interactive sessions and `watch process` are unchanged: they have an earlier observation, so
  rule 1 applies and the window is the interval between two frames, as it always was.
- `ono.process/1` and `ono.process-detail/1` gain a nullable `duration` field. Additive, so no
  version bump (spec §36.5); the default view is unchanged, so no table grew a column.
- **Two assertions changed with this contract**, in the same commit as the ADR:
  `should_report_every_declared_field_when_the_process_is_fully_readable` asserted
  `cpu` was `FieldAccess::Unknown` on a first read and now asserts the lifetime share the fixture
  implies (42 ticks in 500 s = 0.084%), and
  `should_report_a_rate_on_the_second_observation_after_null_on_the_first` — renamed
  `should_report_the_rate_since_the_previous_observation_once_there_is_one` — asserted `null` on
  the first observation and now asserts the lifetime share, keeping its assertion about the rate
  on the second unchanged. `should_declare_the_cpu_field_as_a_percentage` asserted the field's doc
  says "second sample" and now asserts that it names `cpu_window`.
- Encoded by `should_report_the_share_over_the_process_lifetime_when_nothing_earlier_was_observed`,
  `should_measure_the_share_over_the_interval_the_caller_asked_to_sample`,
  `should_report_the_rate_since_the_previous_observation_once_there_is_one` and acceptance case
  `120-process-cpu-share`.

## Alternatives considered

- **Sample twice on every `get process`.** Correct always, and it puts a few hundred milliseconds
  on every enumeration in the shell, including the ones nobody asked a CPU question of.
- **Infer the demand from the pipeline** — sample when a stage mentions `cpu`. It makes a number's
  accuracy a function of spelling: `get process | where cpu > 5` would sample and
  `get process | where @.cpu > 5` or a `sort` over a variable would not.
- **A separate field for the lifetime average, leaving `cpu` null.** Two fields, one of them
  always null in a script, and the documented example still answers nothing.
- **Read `/proc/<pid>/schedstat` for nanosecond accounting and sample over ~10 ms.** The
  resolution is there and the signal is not: a 10 ms window says whether the process happened to
  be on a CPU, not what its load is. `CONFIG_SCHEDSTATS` is also not guaranteed.
