# ADR-0059: Streamed adaptation

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.7, §1.20, §1.37, §1.39 (`du` MUST stream), §1.53; v0.2 §18.3, §18.5; ADR-0013, ADR-0028, ADR-0057
- Decided by: agent (autonomous)

## Context

ADR-0057 decoded a tool's whole output after the child had finished, which is right for a
JSON document and wrong for everything that is a stream: `journalctl -f` never finishes,
`du -a /` finishes after a long time, and v0.3 §1.37 wants the journal to "integrate with Ono
cancellation/backpressure semantics". The process subsystem only offered to capture stdout
into memory and hand it back with the outcome.

## Decision

1. **Decoders are incremental.** `ono_adapter::Decoding` takes bytes as they arrive
   (`feed`) and the end of the output (`finish`). A streaming kind — `jsonl` today, `lines`
   with a record separator next — yields a record per complete line from `feed`; the
   document kinds (`json`, `properties`) buffer and answer from `finish`. A fragment left at
   the end of a stream is `adapter.decode_failed` after the records before it, which were
   real. `decode()` remains the whole-output convenience over the same code.

2. **The process subsystem hands out a pipe.** `Output::Pipe` is a fourth destination for
   standard output; `Executor::start_piped` starts the pipeline in its own process group
   *without* the terminal and returns a `Foreground` the caller reads the pipe from
   (`take_pipe`), asks to stop (`terminate`, `SIGTERM` to the group) and finally waits for
   (`finish_foreground`). `run_foreground` is now `start_foreground` + `finish_foreground`.

3. **The runner streams into the next native segment.** For an adapted stage whose decoder
   streams, `run_streamed_segment` starts the child, decodes its stdout on a reader thread
   into a bounded channel, and runs the following native segment (or, with none, the renderer)
   with that channel as its seed — a `ValueStream` with the invocation's boundedness, so
   `journalctl -f` is `Unbounded` and renders as a live view at the terminal (spec §18.3) while
   `journalctl -n 100` is `Bounded` and renders as a table.

4. **The terminal stays with the shell.** A child that produces values the shell consumes is
   not what the user is interacting with: Ctrl-C reaches the shell's own pipeline exactly as
   for a native run (ADR-0028), the consumer drops the stream, and the child is told to stop.
   This is why the group is started without the terminal; the child's stdin is the plan's
   (`null`), so it never needs it.

5. **Cancellation propagates to the producer, and is not a failure.** When the consumer
   stops before the child's output ended — `take 1`, an error, Ctrl-C — the reader thread is
   still running, the child receives `SIGTERM`, and whatever status it exits with because of
   that is not reported: the pipeline did what was asked. When the reader saw end of file, the
   child's own status stands: non-zero is `external.exit_nonzero` after the records that
   arrived (spec v0.3 §1.20) — they were real too.

6. **Backpressure is the channel.** 256 events in flight; a slow consumer blocks the reader
   thread, which stops reading the pipe, which stops the child at its next write. Nothing is
   buffered without bound.

## Consequences

- ADAPT-005 is met in both halves: records flow while the child runs, and malformed input
  cannot crash the shell (the fuzz walk of `decode.rs` covers the same decoder).
- `du`, `find -printf`, `ps` and `lsof -F` stream once their `lines` decoder is taught a
  record separator in `feed`, which is the next contract step for the Tier B tools.
- Tests: `ono-cli/tests/adapters.rs` (`should_stream_decoded_records_while_the_child_still_runs`:
  `take 1` answers in well under the shim's five-second pause and the child is cancelled;
  `should_report_a_failing_streamed_child_after_its_records`;
  `should_decode_a_streamed_stage_into_typed_journal_events`), the conformance harness over
  `docs/contracts/adapters/fixtures/systemd/`, acceptance case `077`.

## Alternatives considered

- Running the child in the foreground with the terminal — rejected under point 4: the live
  view configures the terminal, which a process group that does not own it cannot do.
- Waiting for a cancelled child to die of `SIGPIPE` — rejected: a follower that writes
  nothing would keep the prompt hostage.
