# ADR-0748: A budget stated as an increase is measured as a subtraction

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §32.1, §32.2; v0.4.1 §33.2, §65.10; ADR-0489
- Decided by: agent (autonomous)

## Context

`perf::TARGETS` was written for v0.4.1 §33.2, whose four rows are absolute: *"basic cached
look/near first result < 50 ms p95"*. `verdicts` reads the recorded p95 off the baseline and
compares it with the budget.

v0.5 §32.1 is not that shape:

> With persistent recording disabled and no historical query executed, v0.5 MUST add less than
> **5 ms p95** to Ono interactive startup on the release reference environment.

Read as an absolute budget, 5 ms is a figure the shell has already spent: `shell.cold_start` stands
at 4.851 ms p95 in the checked-in baseline. A row asserting `startup <= 5 ms` would go green while
saying nothing about the thing §32.1 protects, and it would go red the day the shell got 0.2 ms
slower for a reason having nothing to do with v0.5.

§32.1's second sentence says what the addition would be if there were one: *"Temporal storage
initialization MUST be lazy when recording is disabled."* A store that is opened, migrated or
integrity-checked at startup costs more the larger it is. So the measurement that answers §32.1 is
not one figure — it is two, and the difference between them.

## Decision

**`Target` gains `relative_to: Option<&'static str>`. A target that names one is measured as the
increase over the record it names.**

```rust
Target {
    spec: "v0.5 s32.1: added to interactive startup with recording disabled",
    benchmark: "temporal.startup_disabled",
    profile: "T",
    temperature: Temperature::Cold,
    budget_ms: 5.0,
    relative_to: Some("shell.cold_start"),
}
```

`temporal.startup_disabled` starts the shell with §49's million-event fixture ledger hard-linked to
the canonical path `$HOME/.local/share/ono/temporal/ledger.sqlite3` and recording disabled.
`shell.cold_start` starts the same binary with no ledger anywhere. Both run `echo ready`. The
difference is what the store's *presence* costs, which is precisely what §32.1's laziness clause
protects and precisely what will start costing something the day somebody opens the file eagerly.

Two rules keep the subtraction honest:

- **Half a subtraction is `Unmeasured`, not a smaller difference.** If either record is missing,
  the verdict is `Unmeasured`, which §65.10 makes a failure rather than a pass.
- **A negative difference is zero.** A shell that got *faster* with a ledger present added nothing;
  reporting −1.2 ms against a 5 ms budget would read as a comfortable margin rather than as noise.

## Consequences

§32.1 is measurable today, before any temporal command exists, and it keeps measuring the same
property afterwards. The row bites the moment somebody opens the store at startup, and it is
indifferent to the shell getting faster or slower for unrelated reasons — which an absolute 5 ms
budget would not have been.

`relative_to` is looked up by benchmark id, preferring the Profile S record. There is one
`shell.cold_start` record in the baseline; if a second is ever recorded at another profile, this
lookup gets a profile of its own rather than a tie-break.

Today the two figures should differ by approximately nothing, because the `ono` binary does not yet
link the temporal crates at all. That is worth stating plainly rather than presenting as a pass:
the row is a **regression guard**, and what it currently proves is that a large file sitting at the
canonical path costs the shell nothing. It becomes a proof of §32.1 proper when the session opens
a ledger, and it will not need editing then.

## Alternatives considered

- **An absolute 5 ms startup budget.** Rejected: green while saying nothing, red for unrelated
  reasons. The specification says "add", and a budget that ignores the verb is a different budget.
- **Compare against the pre-v0.5 baseline record across commits.** Rejected: `--write-baseline`
  overwrites that record, so the comparison would destroy its own reference on first use.
- **A `Delta` variant of `TargetVerdict`.** Rejected: `Held`, `Missed` and `Unmeasured` already say
  everything a reader needs, and the figure they carry is the difference. A fourth word would make
  every consumer ask which kind of number it was holding.
