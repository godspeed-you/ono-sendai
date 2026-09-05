# ADR-0225: `explain` prints the spelling a frame narrows to

- Status: accepted
- Date: 2026-08-29
- Spec refs: §14.3, §14.5, §17.1, §42.1; ADR-0023, ADR-0076
- Decided by: agent (autonomous, `close-data`)

## Context

ADR-0023 made a promise on the shell's behalf: "`enter service nginx` then `get process` is
exactly `get process --service nginx`, and `explain` prints the second form when asked about the
first." ADR-0076 §1 then moved narrowing onto the argument seam, and noted in its own
consequences that "`explain` can print the explicit spelling by reading the narrowed arguments" —
and nothing did. `enter process 1; explain get process` planned `1. get process`, with no sign
that a frame was in force at all.

That is the one thing spec §14.5 asks a context to be: inspectable. A user who cannot see what a
frame contributes cannot check it, and the plan is where they would look.

## Decision

**A plan is made in the context it would run in.** `PlanContext` carries the frames in force, and
a native stage's plan reports the explicit spelling the frame narrows it to, in a `narrowed` row
beside `command`:

```text
1. get process
   command      ono.process.get
   narrowed     get process 1
```

The plan asks `narrow` — the same function the command table runs — so the plan and the run can
never disagree. The row is absent when nothing was filled in, so a plan outside a frame says
nothing about frames, and `get process 5` inside `enter process 1` shows no narrowing because
what was typed wins (ADR-0076 §2).

**The spelling is one a user could have typed.** A declared selector is written positionally
(`get process 1`), a declared option as `--name value` (`get process --user root`), and an
ambient selector — a field the command declares no parameter for (ADR-0076 §3) — as the filter it
performs (`get process | where service == systemd-journald.service`). A value is quoted only
when it needs to be.

The stage heading still shows what was typed. The plan reports; it does not rewrite the line.

## Consequences

- `explain` answers spec §14.5's question — "what is this context doing to my command?" — for
  every native stage, and `type` and `ono.meta.explain` carry the same field, since all three
  plan through `plan_with`.
- `PlanContext` gained a `context` field, so every construction site names it. The completion
  path and the pre-flight check pass `&[]`: neither is planning a run in a frame.
- A frame that cannot narrow a command still fails at the seam with `resolve.target_not_found`
  (spec §14.3); the plan reports the narrowing that would happen, not a refusal that has not.

## Alternatives considered

- **Rewrite the stage heading to the narrowed spelling.** Rejected: the heading is what the user
  typed, and a plan that quotes something else back at them is harder to read, not easier.
- **Print the frames once at the top of the plan.** Rejected: `get context` already lists the
  frames. The question `explain` answers is what *this stage* becomes.
