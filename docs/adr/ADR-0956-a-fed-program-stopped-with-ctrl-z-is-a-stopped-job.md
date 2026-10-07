# ADR-0956: A fed program stopped with Ctrl-Z is a stopped job

- Status: accepted
- Date: 2026-10-07
- Spec refs: spec §18.1, §18.4, §18.5; v0.4.1 §28.1–§28.3; ADR-0008, ADR-0954 §2–§3
- Decided by: agent (autonomous)

## Context

ADR-0954 §2 starts the program after a streaming serializer — `… | to jsonl | less` — as the
foreground job, with a pipe for its input, and drives the native stream into that pipe while the
program runs. It said what ends the drive: the stream ending, the program leaving, Ctrl-C. It did
not say what Ctrl-Z does.

Ctrl-Z stops the foreground process group, which is the program. The shell's driver was waiting
for the next value of a source that may never produce one, or blocked in a write into a pipe the
stopped program no longer reads, and noticed nothing: `tail file app.log --follow | to jsonl |
less`, Ctrl-Z, and the shell never came back (release review C3). Before ADR-0954, `to json |
less` collected first and then ran `less` as an ordinary foreground job, whose stop the executor
files as a stopped job.

## Decision

A fed program that stops is a stopped job, as any foreground program that stops is:

- The driver watches the program's process group for a stop (`Foreground::has_stopped`, a
  `waitid(WSTOPPED | WNOHANG | WNOWAIT)` that leaves the news for the executor) beside the stream,
  the reader leaving and Ctrl-C.
- The driver never writes into the program's pipe itself. The lines are handed, one at a time, to
  a thread that writes them; the driver hands a line over only when the thread can take it, and
  reads no further value while it holds one. So a program that stops reading cannot hold the
  driver in a write, and backpressure is what it was: one line in hand, the pipe's own buffer,
  and the bounded channels behind (§28.2).
- When the program stops, the drive ends: the native stream that fed it is cancelled like any
  stream nobody reads any more (§28.3), the program's input is closed once the line in hand is
  written, and the executor files the program as a stopped job and takes the terminal back. The
  line ends with the stopped command's status, `128 + SIGTSTP` = 148 (ADR-0008), and `jobs` lists
  the program.

`fg` continues the program with what it had already been given; its input then ends. The native
stages do not survive the stop: a stream of values running in the shell's own process is not a
process group the kernel can stop and continue, and suspending it would mean holding an unbounded
source open with nobody reading it.

## Consequences

Easy: Ctrl-Z behaves at a streaming serializer's program as it does at any program — the prompt
comes back and the program is a job.

Hard: a continued program has lost the source that fed it. `less` shows what it had and reaches
the end of its input; re-running the line is how to follow the source again. The one line the
writing thread holds when the program stops is written if the program is continued, and dropped
if it is killed.

Encoded by `crates/ono-cli/tests/jsonl.rs`:
`::should_give_the_prompt_back_when_a_fed_program_is_stopped_with_ctrl_z` (PTY), and
`crates/ono-process/tests/external_command.rs`:
`::should_tell_the_caller_a_started_pipeline_stopped_and_still_file_it_as_a_stopped_job`.

## Alternatives considered

**Keep the native stages running behind the stopped program, and resume feeding on `fg`.** The
job table holds process groups; a native stream would have to become a second kind of stopped
job, kept open — a followed file, a watch — with nobody reading it and nothing bounding how long.

**Collect again when the program is a pager.** Which programs are pagers is not something the
shell can know, and a collected unbounded stream is exactly what ADR-0954 exists to avoid.

**Ignore Ctrl-Z for a fed program** (start it without job control). The terminal's stop is the
user's; a program that cannot be stopped while every other one can is a surprise of its own.
