# ADR-0641: The recorder drives every timer, and nothing below it starts a thread

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §10.4, §31.8, §31.9, §32.1, §32.5, §39.2, §39.3; ADR-0634, ADR-0635
- Decided by: agent (autonomous)

## Context

Three parts of v0.5 are described as background work, and none of them says whose background it
is. §31.8: "retention cleanup MUST run in bounded background work." §31.9: "checkpoints MUST not
block the interactive prompt." §32.5: "the recorder SHOULD batch commits within the default 2s
flush interval."

The layer below has deliberately taken no position. `Ledger::sweep(now)` takes the instant as a
parameter and returns `complete: false` when it should be called again (ADR-0634); nothing in
`ono-temporal-ledger` starts a thread, and `ono-temporal-core`, `-reconstruct` and `-query` may
not read a clock at all (§39.2). So the work exists, the mechanism exists, and somebody has to
turn the handle.

Making it the recorder rather than a thread inside the ledger is what keeps §32.1's promise. A
crate that spawned a maintenance thread on construction would spawn it whether or not recording
was enabled, and §32.1 budgets the disabled path at under 5 ms with "temporal storage
initialization MUST be lazy when recording is disabled".

## Decision

### 1. `Recorder::maintenance(now)` is the one turn of the handle

One call flushes if `temporal.flush.interval` has elapsed, drives retention, and reports whether a
checkpoint is due. The caller — a `tokio` interval in the shell, or the `ono-recorder.service`
main loop — decides how often to call it. Nothing in this crate spawns a timer either: a recorder
that is constructed and never driven costs one in-memory ledger and no thread.

### 2. Retention is driven to a bound, and says so when the bound is not reached

`Recorder::sweep(now)` calls `Ledger::sweep` up to `RecorderSettings::max_sweep_passes` times,
default 8, stopping early when the bounds hold. The aggregate `Swept` carries `complete` from the
last pass, so a caller that is behind is told rather than blocked: eight passes of 2048 events is
16 384 events per drive, and a ledger further behind than that gets another drive on the next
turn.

Driving to completion in a loop was the alternative and it is exactly what §31.8's word "bounded"
rules out. A 512 MiB ledger compacted in one call is a prompt held for the duration.

### 3. A checkpoint's projection runs off the prompt path; only its write touches the store

`Recorder::begin_checkpoint` runs `project_checkpoint` — the bounded projection of §42.2, which
walks every object the capture holds — on a thread of its own, and returns a `PendingCheckpoint`
the caller may join later or never. `Recorder::status` reads a counter and the ledger's retention
summary, and waits for neither.

`Recorder` is therefore an `Arc` facade: cloning one clones a handle, so the projection thread and
the prompt hold the same ledger, the same state and the same in-flight counter. `checkpoints_in_flight`
is in the status because a checkpoint that is still running is a thing an operator can see.

### 4. Every instant is a parameter, including the recorder's own

`start`, `stop`, `status`, `sweep`, `maintenance` and `checkpoint` all take `now`. The recorder is
allowed to read the clock — it is the component §39.2 exempts — and it still does not, because a
test that cannot choose the instant measures the machine it runs on. The caller reads the clock
once per turn and passes it down, which also means every event, coverage interval and gap written
in one turn agrees about when the turn was.

## Consequences

The shell owns the cadence, which is where the cancellation, the interrupt handling and the
foreground/background policy already are. `ono-recorder` has no runtime of its own and no
`tokio::spawn` outside the bounded ingestion channel.

`docs/contracts/temporal/recorder.yaml` gives the flush interval as 2s and the checkpoint interval
as 5m; both are settings the recorder holds and the caller reads back from the status, so the
driving loop needs no second source for them.

Tests: `crates/ono-recorder/tests/retention_load.rs` (the bounded drive, the incomplete answer,
and the §32.4 measurement), `crates/ono-recorder/tests/checkpoints.rs` (the status answers while a
projection runs).

## Alternatives considered

- **A maintenance thread inside `Recorder::new`.** Rejected: it would run for a recorder nobody
  started, against §32.1, and it would make every test of the recorder a test of a scheduler.
- **A `tokio` task started at `start` and cancelled at `stop`.** Rejected more narrowly: it works,
  and it puts the cadence in the crate that has no opinion about the shell's runtime, its
  cancellation token or its foreground policy. The handle is one function call; the policy is the
  shell's.
- **Sweeping to completion inside `stop`.** Rejected: `stop recorder` must "flush the ledger and
  stop cleanly" (§10.8), and a stop that also compacted 512 MiB would be neither quick nor clean.
