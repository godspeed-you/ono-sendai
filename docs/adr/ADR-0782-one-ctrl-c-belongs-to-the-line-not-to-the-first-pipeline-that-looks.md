# ADR-0782: One Ctrl-C belongs to the line, not to the first pipeline that looks

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §32.3, §32.6, §39.4; v0.2 §18.1, §18.4, §18.5, §23.3; v0.4.1 §25.1, §25.3,
  §25.4; ADR-0008, ADR-0013, ADR-0480
- Decided by: agent (autonomous)

## Context

v0.5 §32.6 is two sentences and both are observable from outside:

> Long historical queries MUST be cancellable using normal Ono cancellation semantics. Ctrl-C
> MUST not corrupt the ledger or leave locks held.

The second held. The first did not, and the failure was total rather than slow: measured at a real
terminal on 2026-09-09, with `temporal.recording.enabled` on, a session standing at `at -2s`
running

```
find event | each { find event | each { find event | count } | count } | count | to json
```

**printed its answer and reported 0.** The interrupt was not late; it was gone. The same shape with
recording off — `get process | each { get process | count } | count | to json` — answered 130 and
printed nothing, so cancellation as such worked and something about the ledger-backed run defeated
it.

### What actually swallowed the interrupt

Ctrl-C during a native pipeline is delivered to the shell itself: there is no child for the kernel
to interrupt (spec §18.5). `ono-process`'s `SIGINT` handler therefore *notes* the signal in one
atomic flag, and `take_interrupt()` reads that flag **and clears it**. One reader, one note.

That is right for one waiter and wrong for a nest of them. `each { … }` runs a pipeline per item —
ADR-0480's block bridge — and every one of those items is an ordinary foreground run which begins
by dropping whatever note was left over from the prompt, so that a Ctrl-C typed at an idle shell
never cancels the command typed after it. On an *n*²-read query that discard happens thousands of
times a second. Whichever item started next after the signal took the note, threw it away, and the
line went on running.

Instrumenting the discard confirmed it exactly: over one interrupted run of the query above, the
note was consumed there **once**, and never seen by anything that could act on it.

Two further things made the loss certain rather than likely:

- `drive_segment`'s `select!` is `biased`, and the interrupt is its last branch. A pipeline whose
  values are already queued — which is what `find event | count` over fifty events is — never
  leaves the earlier branches pending, so the interrupt branch is not polled at all.
- While the driver runs one block item it is *outside* its runtime. Nothing it waits on is polled
  for the whole duration of the item, so a 40 ms interrupt tick is not a cancellation point.

And a third defect stands behind both, independent of blocks: `LedgerRead::events` with a large
limit is **one synchronous call**. On an 80,000-event ledger `find event --limit 100000 | count`
took about 80 s, and the shell was inside that call for all of it. Even a perfectly delivered
interrupt could not be answered until the answer nobody wanted was complete.

## Decision

### 1. The interrupt note is taken once and remembered for the whole line

`crate::eval::pipeline` holds a thread-local pair: `REACHED`, the Ctrl-C remembered for as long as
the line that received it is still running, and `RUNNING`, how many pipelines of that line this
thread is inside. `interrupt_reached()` reads the process-wide note, latches it into `REACHED`, and
answers from `REACHED` thereafter. Every level of a nest therefore sees the same interrupt, and
every level unwinds.

`ForegroundRun` is what distinguishes the levels. The **outermost** one begins the line: it drops
the stale note and clears `REACHED`, preserving the property the discard existed for. The
**nested** ones — one per `each` item — begin nothing and clear nothing. It is held in both places
that used to drop the note on their own: across `run_pipeline`, and in `run_native_segment` where
the bare `take_interrupt()` stood. Dropping the note is now something entering the line does, and
there is no second place that can do it by accident.

The memory is per thread rather than global because the foreground driver is one thread: the
evaluator owns the session, `block_on` runs its futures on the calling thread, and a synchronous
read a command makes happens there too. A background job on a runtime worker (spec §18.4) has a
memory of its own that nothing ever sets, so a foreground Ctrl-C cannot reach into it.

### 2. Cancellation is checked between items, synchronously

Two checkpoints, both cheap loads:

- at the top of `drive_segment`'s loop, which is between two block items and before the first;
- in `run_stage_list`, immediately before a native segment is assembled, which is the last point
  the shell controls before an item's own pipeline starts.

Neither waits on the runtime, because the driver is not in the runtime when it matters. This is
what makes `find event | each { … }` cancellable: many small ledger reads, one cancellation point
per read.

### 3. A long synchronous ledger scan asks whether the answer is still wanted

`ono-temporal-ledger` gains `watch_for_cancellation(check: fn() -> bool)`. The shell installs one
watcher when it begins a foreground line; `LedgerStore::events` asks it every 256 rows and refuses
with `stream.cancelled` rather than finishing a scan nobody is waiting for. The partial read is
discarded — half a query is not a smaller query — and the store is untouched: nothing is written,
no transaction is open, and the next reader and the next writer find what they found before.

§39.4 forbids `ono-temporal-core` from exposing SQL, and it equally forbids the ledger from knowing
what §18.5 cancellation *means*. So the store does not decide anything: the shell says how to ask,
once, and the scan asks. Where nobody has installed a watcher — a test, a migration, a recorder
flush — the scan reads to the end exactly as before.

The gate on `RUNNING > 0` is what keeps this honest: only a thread actually running a foreground
pipeline may answer yes, so a recorder flush or a background job reading the same store is never
cancelled by somebody else's Ctrl-C.

### 4. What is cancelled, and what is merely refused

Stated precisely, because §32.6 deserves a precise claim:

| Work | What Ctrl-C does |
|---|---|
| `find event \| each { … }`, and any stage that drives a body per value | The item in flight finishes; no further item is started; the line unwinds with 128 + SIGINT. |
| One `LedgerRead::events` scan | The scan stops at the next 256-row boundary and refuses. The rows already decoded are dropped. |
| Work a command does around its read — building records, evaluating a predicate | Not interrupted. It completes, and the interrupt is answered at the next checkpoint after it. |

Nothing here is *abandoned in the background*: there is no thread left grinding on after the prompt
returns, because the work is on the driver thread and the driver thread has returned. What is left
behind is at most the remainder of one item and one batch of rows.

## Consequences

Easy now:

- Ctrl-C during a historical query returns the prompt with 128 + SIGINT, and the answer is never
  printed. `crates/ono-cli/tests/temporal_cancellation.rs::should_return_to_the_prompt_with_the_interrupt_status_when_a_long_historical_query_is_cancelled`
  is the red-to-green test; it went from failing with `alive-0` and a printed `[55]` to passing,
  and the run it interrupts now ends in about a second instead of running to completion.
- A single scan over a large ledger is interruptible, which no amount of work in the shell could
  have achieved: `crates/ono-temporal-ledger/tests/cancellation.rs` states that at the crate
  boundary, including that the store still reads, still writes and still holds every event.
- Cancellation with recording off keeps working — it now goes through the same latch — and
  `docker/acceptance/cases/275-temporal-cancellation.case`, whose query is a filesystem walk with
  no block in it, is unaffected.
- An interrupt outranks whatever the run had already gathered. `find file / | … | count` collects
  an `io.permission_denied` for every directory this user may not read, and when the Ctrl-C landed
  the first of those became the answer — status 1, a refusal about a directory nobody had asked
  about, with the interrupt nowhere in it. `interrupted_flow` now consults the latch as well as the
  error, so a line the shell was told to stop ends with 128 + SIGINT whatever else it was carrying.
  `docker/acceptance/cases/275-temporal-cancellation.case`'s `t12c5` and `t12c11` are what state
  it, on a real terminal with a real `^C`.
- The last window in which a `SIGINT` could still be discarded is closed. Between the checkpoint in
  `run_stage_list` and the assembly of a native segment, the shell resolves stages, expands globs
  and binds arguments; `crates/ono-cli/src/eval/native/foreground.rs` used to drop the note there
  with a bare `take_interrupt()`, which cost **697 ms of 19,104 ms — 3.65% — on the *n*²-read query
  above**, or roughly one Ctrl-C in twenty-seven on the worst-case shape. That discard is now the
  acquisition of a `ForegroundRun`, so it clears the note at the outermost level of the line and at
  no other, and every level of an `each` nest sees the same interrupt.

Hard, and knowingly left:

- The 256-row batch is a number, not a derived quantity. It is large enough that an ordinary query
  pays the question once and small enough that a scan over a hundred thousand events answers within
  a few hundred rows. Revisit it if §32.3's budgets move.
- `watch_for_cancellation` is process-wide and first-writer-wins, which is right for a shell and
  wrong for a library with two independent consumers. A second consumer is a reason to revisit it,
  not a reason to generalise it now (AGENTS.md §4).

## Alternatives considered

**Make the interrupt reach the driver by polling harder.** A watchdog task on the runtime, or a
tighter tick in `interrupted()`, latching the note from a second thread. Rejected on arithmetic: the
loss window is ~230 µs per item, so a 1 ms watchdog closes about a quarter of it and a watchdog fast
enough to close all of it spins a core for the length of the query. It also cannot help while the
driver is inside a synchronous read, which is the case that matters.

**Trip the pipeline's `CancelToken` instead of latching the note.** The token is the right mechanism
for stopping producers (ADR-0013) and the wrong one for *noticing*: it is tripped by whoever saw the
interrupt, and the whole defect is that nobody saw it. The token still does its job after the fact,
where `run_native_segment` already cancels it.

**Run the block item somewhere the driver can abandon.** The session is not `Send` and only the
thread that owns it may run statements (ADR-0480), so there is nowhere to put it.

**Give `LedgerRead::events` a cancellation parameter.** It would put the shell's §18.5 vocabulary
into `ono-temporal-core`'s read contract, which §39.4 exists to keep clean, and it would oblige
every implementation — including §10.7's in-memory session ledger, which cannot be slow — to carry
it. An installed watcher costs the fast path one predicate and the contract nothing.

**Return the rows read so far when a scan is cancelled.** Rejected: a truncated history is
indistinguishable from a short one, and §43.2 prefers an explicit gap to pretended continuity. A
cancelled query has no answer, which is what 128 + SIGINT says.
