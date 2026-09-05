# ADR-0221: A count of what could not be read is not zero

- Status: accepted
- Date: 2026-08-29
- Spec refs: §6.1, §11.1, §16.5, §35.3, §43
- Decided by: agent (autonomous, `close-data`)

## Context

```text
$ ono -c 'get process 999999 | count'
ono: Ono-Sendai-E0301 io.not_found /proc/999999/stat: No such file or directory
VALUE
0
$ echo $?
0
```

Three answers that contradict each other. `get process 999999` alone exits 1 — the CLI's
partial-failure rule (ADR-0085) says that a run which reported a failure and produced no values
did not answer. Putting `count` after it produced a value, so the rule no longer applied, and
the status said the run had succeeded.

The value `count` produced was fabricated. Nothing was counted; the stream carried one failure
and no objects, and `0` is an assertion about the system that nobody made. Spec §35.3 is explicit:
unknown data is null, never fabricated or zero. `measure` did the same, emitting a summary row
of nulls with `count: 0`; `reduce --initial 0` answered its seed.

Separately, when nothing survived, the failure was printed twice: once by the loop that reports
every failure, and once more by the caller that reports the error the run failed with.

## Decision

**A selector that names an object which is not there is a refusal, and the refusal is the
answer** — whatever stage follows it.

**An aggregate over a stream that produced no values and reported a failure emits nothing.**
`ValueStream` records whether a failure passed through it; `count`, `measure` and `reduce` ask
after reading to the end, and stay silent rather than answer for a stream they could not read.
The failure then reaches the CLI as the only thing the pipeline produced, and the existing rule
— reported failures, no values, exit 1 — decides the status, with no new status vocabulary.

**A partial failure is unaffected.** `get process | count` on a churning host counts every
process it could read and reports the ones it could not: values arrived, so the aggregate
answers and spec §16.5 holds.

**A failure that is the answer is reported once.** When nothing survived, the first failure
travels as the error the run failed with and is printed by the caller that prints it; the others
are reported here.

## Consequences

- `get process 999999 | count`, `| measure pid` and `| reduce … --initial 0` all exit 1 with one
  reported failure and nothing on stdout. `get file /nope | count` likewise.
- `catch` sees the structured error, because the run fails with it rather than with a fabricated
  zero.
- A `--initial` seed is still what an *empty* fold answers (ADR-0032); it is not what a *failed*
  one answers.

## Alternatives considered

- **Widen the CLI's `unanswered` rule to every failure kind.** Rejected: it would make one
  unreadable `/proc` entry fail a `get process | count` over five hundred that were read, which
  is exactly the collapse spec §16.5 forbids.
- **Let the provider refuse the query outright instead of streaming a failure.** A better error
  for `get process 999999` — the process was never listed, so the help "it exited in between" is
  wrong — but it is the provider's text to fix, and it would not have stopped `count` from
  fabricating a zero for any other empty-and-failed stream.
- **Exit 0 and print nothing at all.** Rejected: the object was named and is not there; a shell
  that says nothing about it cannot be scripted against.
