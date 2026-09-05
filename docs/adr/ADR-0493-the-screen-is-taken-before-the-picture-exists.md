# ADR-0493: The screen is taken before the picture exists

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4 §23.3, §23.4, §34, §49.8, §52.1; v0.4.1 §33.1, §34.2, §35.2, §61.1, §65.9;
  ADR-0263, ADR-0424, ADR-0431, ADR-0491, ADR-0492; issues #20, #85
- Decided by: agent (autonomous)

## Context

Issue #20 — B-spat-6 — says a full-screen map of COMPUTE is unresponsive while one projection is
in flight, and it has been open since a 500-process host showed it. ADR-0263 fixed the
*neighbouring* defect (the index grew by one population per observation, so the view got
monotonically slower for as long as it stayed open) and said in as many words that it did not make
the first projection concurrent with the repaint.

The cause is one line of ordering. `run_map_view` projected the place, and only then took the
terminal:

```rust
let mut record = crate::spatial::map::projection(ctx, session, &center, &request, now).await?;
…
let _raw = RawMode::enter()?;
let _screen = AlternateScreen::enter()?;
```

So for the whole of the opening projection the terminal was still cooked, no key was read, and
nothing at all was drawn. On the reference environment, projecting COMPUTE at Profile M costs
1.84 s in a debug build. §34 asks the opposite:

> the shell MUST remain interactive and progressively update rather than block unnecessarily

and v0.4.1 §33.1 makes time to first *useful* result a first-class target, with §65.9 naming a
progress-free interactive computation as a failure mode.

ADR-0424 had already solved the same problem for every *later* projection: `while_answering`
polls the keyboard every 16 ms while the work runs, queues what was typed and lets `Close` leave
at once. The opening projection was the one that never went through it, because there was no
screen to answer into.

## Decision

**The guards and the first frame come before the first projection, and the projection then runs
where a key can reach it.**

`run_map_view` now enters raw mode and the alternate screen, paints an opening frame, and awaits
the opening projection inside `while_answering`. Nothing else about the loop changes: the same
guards in the same order, the same key queue, the same `Awaited::Left` path out.

### 1. The opening frame says what it does not know

§35.2 permits a first frame that does not hold every edge and requires it to be *"truthful about
omitted/pending detail"*. Nothing is projected yet, so the frame carries the place path and one
line saying the place is being projected and that no detail is drawn. That is §65.9's middle
answer — results, **progress**, or a bounded refusal — and it is the only one of the three that is
true at that moment. An empty map would be a lie about the place; a blank screen is the defect.

### 2. The margin is measured, not assumed

`FIRST_FRAME` in the test suite is 700 ms, against a projection that costs 1.84 s at Profile M in
a debug build. That is a discriminator rather than a liveness bound, and the two tests fail
without the change and pass with it — verified by reverting the source file and re-running:

| Test | Without the change | With it |
| --- | --- | --- |
| `should_answer_focus_movement_while_a_projection_is_still_running` | fails: the alternate screen never appears inside 700 ms | passes |
| `should_close_the_full_screen_map_promptly_while_a_projection_is_in_flight` | fails for the same reason | passes |

Both hold the host at Profile M for their duration, because §32.1 forbids proving anything about a
spatial operation against whatever the machine happened to be running.

### 3. One existing test had to move, and why that is a `fix` rather than a `refactor`

`should_restore_the_shell_screen_when_the_full_screen_map_closes` read the transcript immediately
after the alternate screen opened and asserted the domains were in it. That was true when the
screen opened *after* the projection and is not true now, so it waits for the frame with the exits
in it instead. The behaviour changed — which is why this is a `fix` with its own tests and not a
refactor (AGENTS.md §4, §11) — and the assertion is unchanged: the same domains, on the same
screen, one frame later.

## Consequences

Easy: every projection in the view now runs through `while_answering`, so `Esc` works at every
moment the view exists, including the first. The opening frame also gives the live map somewhere
to say what ADR-0492 made it say in a pipe.

Hard: `map` on a fast place now paints twice — the opening frame and then the picture. The loop
already refuses to write a frame identical to the one on screen (§25.2, §39.4), so nothing
flickers that was not going to be drawn anyway, but a terminal recording of `map` on a small host
now has one extra frame in it.

Also hard, and not fixed here: the 1.84 s is still 1.84 s. Making the projection itself cheap is
§34.2's cost classes and §34.4's bounded neighbourhood — issues #86 and #87 — and ADR-0491's
`#[ignore]`d target test owns the figure. What this increment removes is the *blankness*, which is
what issue #20 is about and what §65.9 forbids.

Encoded by
`crates/ono-cli/tests/spatial_interactive.rs::should_answer_focus_movement_while_a_projection_is_still_running`,
`::should_close_the_full_screen_map_promptly_while_a_projection_is_in_flight`, and acceptance case
`196`'s §33.1 section, which drives a real controlling terminal in the container.

## Alternatives considered

**Project on a background task and let the loop poll it.** It is what "concurrent with the
repaint" sounds like, and it needs the session — `&mut SpatialSessionState` — inside the task,
which would mean sharing the index across a task boundary for one frame's benefit.
`while_answering` already gives the loop its 16 ms slices without any of that, and it is the
mechanism ADR-0424 chose for exactly this problem.

**Draw an empty map as the first frame.** It would need no new frame builder and it would show a
place with no exits, which §2.17 forbids: a projection that cannot say what it left out must not
present a bound as the whole picture.

**Assert the fix with a wall-clock threshold on the close alone.** A close is fast either way once
the screen exists, so the assertion would pass without the change as soon as the projection had
finished. The discriminator is the *first frame*, which is why both tests bound that and not the
close.

**Leave issue #20 to the increment that makes the projection cheap.** A projection that took 200 ms
instead of 1.8 s would still be a blank terminal for 200 ms, and §65.9 is about the blankness
rather than about the duration. The two are separate defects and they are fixed separately.
