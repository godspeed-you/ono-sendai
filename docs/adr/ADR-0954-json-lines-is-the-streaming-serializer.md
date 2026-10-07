# ADR-0954: JSON Lines is the streaming serializer

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §12.2, §12.3, §18.3, §33.5, §50; v0.4.1 §2.5, §22.3, §28.1–§28.4, §65.8;
  ADR-0008, ADR-0013, ADR-0220 (a reader that leaves), ADR-0223, ADR-0455
- Decided by: agent (autonomous)

## Context

Spec §18.3 requires an unbounded stream to be given a representation before it reaches a pipe or
a file, and every representation `to` had was one document for the whole stream: `to json` is one
array, which exists only once the stream has ended. So `map --live --json | take 100 | to json`
printed nothing if it was cut off before the hundredth value, `watch process | to json` — the very
command the shell's own §18.3 refusal recommended — never answered at all, and a live view could
not be scripted (issue #214). The contract of `to json` is one array, and it stays one array.

## Decision

### 1. `to jsonl`

`to jsonl` writes JSON Lines: each value becomes one compact JSON document on a line of its own.
The encoding is `to json`'s — `ono_value::to_json_data`, the canonical data form of §33.5 — so a
line is exactly the element `to json` would write for that value inside its array; `--human` works
as it does for `to json`, and `--pretty` is refused (`type.mismatch`) because a pretty document is
not one line.

The stage holds nothing between values and keeps its input's boundedness: a stream declared
unbounded is a legal input — there is no `stream.unbounded_operation` refusal, because nothing
waits for an end — and its output is unbounded too.

### 2. The lines are written as they arrive

A native segment whose last serializer is `to jsonl`, followed only by stages that pass values on
as they come (`take 2`, `where …` — streaming, non-materializing contracts), is not collected for
the end. The driver writes each line and flushes it the moment it reaches the end of the
pipeline:

- to the shell's standard output, or to the file the stage's redirection names;
- into the standard input of the program after it, when that program is the last stage
  (`… | to jsonl | jq .`): the program is started first, as the foreground job, with a pipe for
  its input (`ono_process::Input::Pipe`), and its status is the pipeline's (ADR-0008).

A capture — `let x = (… | to jsonl)` — still collects, because its value is one text. A pipeline
whose bytes reach a program in the middle of it, with more stages after the program, keeps the
collected byte boundary it had (ADR-0013).

### 3. Backpressure, the reader leaving, and Ctrl-C

- **Backpressure.** A write blocks while the reader is slow, the driver does not drain while it is
  blocked, and the bounded channels behind it fill and stop the producers (§28.1, §28.2). Nothing
  is buffered for a slow reader.
- **The reader leaving.** A write that finds the reader gone ends the drain; so does the reader
  going while nothing is being written, because the write end of a pipe reports an error the
  moment no reader is left and a watcher thread waits for exactly that. A shell whose own output's
  reader left ends as a program killed by `SIGPIPE` does, silently, with status 141 (ADR-0220) — it
  never panics. A program the shell was feeding that read what it wanted and left is `yes | head
  -1`: the pipeline ends with the program's status.
- **Ctrl-C** ends a streaming line like any other native line (130). Whichever way the drain ends,
  the pipeline's cancellation scope is tripped, so a producer that was waiting — a followed file
  that never grows — stops rather than lingering (§28.3), and a fed program is always waited for.

### 4. The contract enumerates the formats

`to`'s `format` selector declares its closed set — `values: [json, jsonl, yaml, csv, text, bytes]`
— so completion offers them from metadata (`to j<TAB>` → `json`, `jsonl`), and `to` accepts exactly
that set: the implementation reads the list from the contract rather than keeping one of its own.
The §18.3 refusal now recommends `to jsonl`.

### 5. `from jsonl` is left for later

Reading JSON Lines back is the obvious symmetry, and it is not obviously right yet: `from` reads a
document out of text chunks that are not lines (a program's output arrives as one block of bytes,
a followed file as one value per line without its newline), so which boundaries delimit a document
has to be decided for both shapes, and decided streaming. It is a follow-up, not part of this
change.

## Consequences

Easy: a live view is scriptable — `watch process | to jsonl`, `tail file app.log --follow | where
… | to jsonl | jq .` — and a cut-off run has printed everything up to where it was cut.

Hard: two serializer shapes exist — documents for the whole stream, and documents per value —
and only one of them streams. The plan says which (`streaming yes` on `to` is the contract's).
`to json`, `to yaml`, `to csv` and `to text` still collect over an unbounded stream rather than
refusing it; that is recorded as a finding outside this change.

Encoded by `crates/ono-cli/tests/jsonl.rs`:
`::should_write_the_first_line_before_the_second_value_exists`,
`::should_encode_each_value_as_to_json_encodes_it_in_its_array`,
`::should_end_when_take_bounds_the_stream_before_or_after_the_serializer`,
`::should_accept_an_unbounded_stream_that_to_json_could_not_finish`,
`::should_name_the_streaming_serializer_when_a_live_stream_has_no_representation`,
`::should_stop_without_a_panic_when_the_reader_goes_away`,
`::should_stop_writing_into_a_program_that_has_read_enough`,
`::should_keep_memory_flat_while_an_unbounded_stream_is_serialized`,
`::should_stop_a_streaming_serializer_on_ctrl_c_and_keep_the_prompt` (PTY);
`crates/ono-command/tests/completion.rs::should_offer_the_formats_to_writes_including_the_streaming_one`;
`crates/ono-process/tests/external_command.rs::should_hand_the_caller_a_pipe_it_writes_the_childs_stdin_into_while_it_runs`;
acceptance case `363`.

## Alternatives considered

**`to json` forwarding one document per value on an unbounded stream.** The issue offers it. It
makes the shape of `to json`'s output depend on a declaration the reader cannot see — an array for
a bounded stream, lines for an unbounded one — and a script would have to know which it got. A
format name is the one place that says what the bytes are.

**Collect into the program and refuse an unbounded stream there.** Safe and useless: the case a
streaming serializer exists for is exactly the one it would refuse.

**Detect the reader leaving only on the next write.** What a classic shell does, and why `tail -f
log | head -1` waits for the log to grow before it ends. The watcher costs one sleeping thread per
streaming write to a pipe and ends the run when the reader does.
