# ADR-1032: A reader that may not wait reads where the session stands as its last holder published it

- Status: accepted
- Date: 2026-11-15
- Spec refs: v0.4 §19.2, §21.1, §21.2, §30.3, §34, §34.1, §46; v0.5 §4.6; ADR-0982, ADR-0984
- Issues: #311
- Decided by: agent (autonomous)

## Context

The spatial session state is one process-wide asynchronous mutex (`spatial::session`). The
commands that change it hold it, often across a provider read. Two readers run outside the
runtime and may not wait for it. One is the line editor drawing a prompt, which names the current
place (v0.4 §21.1). The other is the `cd` builtin, which asks whether the place is in the storage
family and so follows the working directory (§30.3). Both used `try_lock()` and degraded when the
state was held: the prompt dropped its place segment, and `cd` said "not storage". The comment
gave the reason: "while a spatial command holds it, which is not a moment at which a prompt is
drawn".

v0.6.4 made that false. ADR-0984 reads the collection the session arrived in behind the prompt:
after **every** interactive command a task on the session's runtime locks the state to decide
whether there is anything to read, and again to register each answer. ADR-0982 registers a slow
horizon provider's answer whenever it arrives. Both hold the state on a worker thread at the
moment the main thread draws the next prompt. When the two met, the prompt read
`local://~/work` over a session standing in `local/compute`, the same text as the root place's
prompt. That is #311. It was the PTY flake that kept `scripts/release-check.sh` from completing for
v0.6.5. A diagnostic build that painted a marker on a failed `try_lock` showed the holder in every
failing transcript. Widening its critical section to 200 ms made every test that reads the place
off a prompt fail every time. A `cd` that met the same read left the place behind the working
directory: navigation state, not only its display.

## Decision

**Every holder of the spatial state publishes where the session stands when it lets the state
go, and the readers that may not wait read that publication instead of the state.**

- `spatial_session()` hands out a `SpatialSession`. It derefs to the state and, on drop, while
  the lock is still held, publishes a `Standing`: the place as the prompt names it
  (`resolve::concise_path`), whether it is in the storage family, and the linked host the
  session stands on. Publications are therefore
  ordered as the holders were, and the last one is the state as the last holder left it. The
  first one is made when the state is created, so a fresh process stands at the root (§46.1).
- `place_segment()` and `host_segment()` (the prompt) and `place_is_storage()` (`cd`) read
  `standing()`, so the host and the place the prompt names are always one moment's: a command
  that crosses a link moves the geography the moment it moves, and a prompt that read the host
  from there named the new host beside the old place. They never
  wait and never degrade. A holder that is still working has not moved the session yet, so the
  prompt drawn during a background job's command shows the place the session stands in until
  that command lets the state go. That is the truth, not a fallback.
- The mutex itself is private to the module. The only other way to it is
  `try_spatial_session()`, which hands out the same publishing guard, so no holder can change
  the state without publishing.

**The prompt's `[PAST]` marker reads the published coordinate and nothing else.** It is
published under the temporal lock by the transition that commits it (`TemporalState::commit`),
so it never needed the lock. The `try_lock` in front of it dropped the marker whenever the state
was held, and since ADR-0984 the read behind the prompt can hold it: its `not_recorded` refusal
consults the temporal state on a worker thread.

**Completion keeps its non-waiting borrow** (`try_spatial_session()`). Tab runs in the line editor
under §34's 50 ms and asks no provider. A borrow that waited would wait on whatever a job's
command is reading. A Tab that meets a registration behind the prompt offers nothing that once.
Nothing it offers is wrong, and the next Tab asks again. Its candidates need the index, which is
too large to publish on every release.

## Consequences

Easy: the prompt and `cd` are correct whatever runs behind them, and the background reads of
ADR-0982 and ADR-0984 stay exactly as they are. No production path depends on scheduling for the
place, and nothing is special-cased for tests.

`cd` now waits for the state where it is in storage, as every spatial command does, when a job's
command holds the state across a provider read. Before, it answered "not storage" and left the
place behind. Behind an ordinary prompt the holders are the background reads, whose critical
sections are in memory, so there is nothing to wait for.

Hard: every release of the state computes the place's concise path and the storage question,
and writes the publication when they changed. Both are a lookup in the index. The state is
released a few times per command, so this is not measurable against a command's cost.

Encoded by `crates/ono-cli/src/spatial/session.rs::tests::should_name_the_current_place_in_the_prompt_while_a_background_read_holds_the_session`,
`…::should_name_the_host_and_the_place_of_one_moment_in_the_prompt_while_a_holder_crosses_a_link`,
`crates/ono-cli/src/spatial/storage.rs::tests::should_know_the_place_is_in_storage_while_a_background_read_holds_the_session`
and `crates/ono-cli/src/temporal/prompt.rs::tests::should_keep_the_past_marker_in_the_prompt_while_another_task_holds_the_temporal_state`.
Each failed before this decision and holds the state itself while it reads. The PTY suites
`crates/ono-cli/tests/spatial_interactive.rs` and `…/temporal_view.rs` prove it at the terminal.

## Alternatives considered

**Wait for the lock in the prompt.** The background critical sections are short, but a job's
command may hold the state across a provider read, which may take seconds (the systemd bus allows
ten). A prompt that waited would freeze the shell behind a job, which §34 forbids. A bounded wait
would still drop the place, only more rarely.

**Run ADR-0984's decision synchronously before the prompt.** It closes the first window and not
the second: each answer is registered under the lock whenever the provider delivers it, and
ADR-0982's late answers arrive at any time.

**Publish only where the place moves.** The place is the trail's current step, and the trail is
moved from a dozen call sites through `trail_mut()`. Publishing at release covers every one of
them, and every change of a name the path is read from, without a list that a new movement
could fall off.

**Skip the publication while a panic unwinds.** A panic inside the publication would abort the
process, but nothing in it can panic: it reads the index and builds a string. Skipping it would
leave the place a holder moved to unpublished.
