# ADR-0028: Native stages in a Unix pipeline

- Status: accepted
- Date: 2026-08-26
- Spec refs: §5, §11.3, §12.3, §16.5, §33.5, §50; ADR-0009, ADR-0011, ADR-0013
- Decided by: agent (autonomous)

## Context

The value system, the Linux providers and the command registry were all built and tested as
libraries, but nothing connected them to the evaluator: `get process | where cpu > 20 | to json`
reported `command not found: get`. Wiring them up forced four questions the spec leaves open,
because a pipeline can now hold two kinds of stage and the shell has to run both.

## Decision

### 1. A structured transform binds only where structure reaches it

ADR-0011 puts native commands ahead of `PATH`. Taken literally that breaks ordinary Unix work on
the first line someone types, because `sort`, `find`, `join`, `diff` and `tail` are all declared
verbs *and* programs everybody has. `printf 'b\nc\na\n' | sort` would bind the transform, which
is defined over a stream of records, to a stream of bytes.

So step 4 of ADR-0011 gains a condition:

> A native command whose declared input is a stream of objects binds only where a stream of
> objects reaches it — at a stage whose predecessor is itself native and produces objects.
> Elsewhere the name resolves onward to `PATH`.

A command that declares `any`, `string`, `bytes` or `null` on its input is unaffected: `to`,
`from` and `format` are defined over the boundary of spec §12.3 and bind on either side of it,
which is exactly what makes `echo … | from json | where …` work.

Where the transform does not bind and **no program of that name exists either**, the transform
binds anyway and reports the type error when it runs. "`count` needs objects; pipe through
`from …`" is a better answer than "command not found: count", and the user typed a real command.

`ono:sort` still forces the transform, as ADR-0011 says a forced namespace must.

### 2. A pipeline is a sequence of runs, and only a boundary passes through the shell

The stage list is split into maximal runs of adjacent stages on the same side of §12.3's
boundary. A run of external stages is handed to `ono-process` whole, so ADR-0013's real `pipe(2)`
between children is untouched and `yes | head -1` is still a genuine `SIGPIPE`. Bytes are carried
by the shell **only** where a native run adjoins an external one: into the first child as
`Input::Bytes`, out of the last child as captured stdout.

That carry is buffered, and buffering is a real cost — `find / | from text | take 1` reads the
whole listing before answering. It buys the boundary being explicit and correct now; making it
stream is a later increment, and it is on the board rather than in a comment.

### 3. Objects aimed at a process are a type error, not a rendering

Confirming ADR-0013 in the implementation: a native run that does not end in `to` or `format`,
followed by a child process, is `type.mismatch` naming the fix. Spec §12.3 says the conversion is
explicit precisely so that "hidden formatting" never becomes API behaviour, and a table the
receiving program would have to parse back is the text-shaped coupling the object pipeline exists
to remove.

### 4. Partial failure is reported, not fatal

Spec §16.5 forbids collapsing "97 succeeded, 3 failed" into one answer. The Linux process
provider is built to that rule: a process that exits between being listed and being read lands on
the stream's error channel, with its identity, while the stream keeps running.

The evaluator had to decide what that means for the run as a whole, and the first implementation
got it wrong — it raised the first failure and discarded every value, so `get process | count`
failed on any busy machine. The rule is:

> Failures on the error channel are reported on stderr, one structured error each, and the values
> that arrived are still delivered. The run fails only when **nothing** arrived: a query with no
> values and at least one failure produced no answer, and that is a failure.

A query that legitimately matches nothing — `get process | where pid == 999999` — has no failures
and succeeds with an empty result, which is a different thing and reads differently.

### 5. A native stage cannot be backgrounded yet

`&` puts a pipeline in a process group the shell can signal and resume. A native run has no
process group, so backgrounding one is not a small change, and pretending otherwise would give
the user a job that `fg` cannot bring back. A backgrounded pipeline therefore keeps the external
path today. `docs/STATE.md` carries it.

## Consequences

- `sort`, `find`, `tail`, `diff` and `join` keep their Unix meaning at a byte boundary and their
  Ono meaning after a native stage, with no whitespace heuristic and no configuration deciding
  which — the types decide, and `explain` reports the decision.
- The buffered boundary and the missing background path are both bounded, both known, and both on
  the board. Neither is hidden behind a `TODO`.
- Tests: `crates/ono-cli/tests/native.rs` asserts each rule at the CLI boundary — a native
  pipeline serialised, a filter over provider objects, an external program feeding `from json`,
  the §12.3 error, and redirected output being byte-identical to piped output (spec §50).

## Alternatives considered

- **Native always wins, as ADR-0011 reads literally.** Rejected: it breaks `printf | sort` and
  `cat *.txt | sort -k2 -n`, both of which the Phase A acceptance case runs. Spec §1 asks the
  shell to replace bash for ordinary work; losing `sort` on day one is not that.
- **External always wins where a program exists.** Rejected: it would make `get process | sort
  name` spawn `/usr/bin/sort` on a stream of records, which is the same type error from the other
  side, and would make the object pipeline unusable for the verbs it is built on.
- **Decide by whitespace or by an option.** Rejected for the reason ADR-0009 rejected it for
  arguments: the shape of a value must never decide the structure of a command.
- **Render objects automatically when a process follows.** Rejected by spec §12.3 outright.
- **Fail the run on any partial failure.** Rejected: it makes `get process` fail on a busy
  machine, which teaches users to ignore the status.
