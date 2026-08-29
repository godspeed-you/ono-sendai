# ADR-0376: Bytes that cannot become objects are refused before the program runs

- Status: accepted
- Date: 2026-08-29
- Spec refs: §5, §12.2, §12.3, §16.5, §50; spec v0.3 §1.4, §1.18; ADR-0011, ADR-0013, ADR-0028, ADR-0057
- Decided by: agent (autonomous, `C6D-last`)

## Context

`docs/STATE.md`'s B-data-9 read: "an unbounded external producer never reaches a native stage".
The reproduction is `ono -c 'yes | take 1'`, which never returns — killed at eight seconds, seven
of them system time. ADR-0028 point 2 records the byte carry across a native/external join as
buffered on purpose, and calls making it stream "a later increment".

Three observations decide this, and the first two were not in the board's entry:

```text
$ ono -c 'ls /etc | count'         →  [1]
$ ono -c 'seq 3 | take 1'          →  VALUE / 310a320a330a
$ ono -c 'yes | take 1'            →  (never answers)
```

`count` answered `1` for a directory of two hundred entries, and `take 1` answered the whole
listing as one hexadecimal blob. The carry was being wrapped into a single `Value::Bytes` and
handed to stages declared over a *stream of objects*, which then dutifully counted the one value
they were given. The hang is the same defect seen through an endless producer: the wrap needs end
of file, so the question "can `take` use this at all?" was being asked after the answer had
already stopped mattering.

ADR-0028 point 1 had already written the rule that was missing:

> Where the transform does not bind and no program of that name exists either, the transform
> binds anyway and reports the type error when it runs.

The binding happened; the type error never did.

## Decision

**1. ADR-0028 point 2's buffering stands, and streaming the carry is rejected — not deferred.**
The bytes a program writes are one *document*. `from json` cannot answer half a document, and
there is no honest way to cut an arbitrary byte stream into values: cutting at newlines is the
implicit text parsing spec §50 forbids ("never parse unstable human-readable output"), and
cutting at read-buffer boundaries makes a value whose content depends on how the kernel scheduled
the producer — non-deterministic output, which spec §50 forbids from the other side. So the carry
stays whole, and `yes | from json` still runs until the producer ends, for the same reason `jq`
does. That is the semantics, not a shortcut.

**2. A stage declared over objects is refused the bytes a program wrote, and the refusal comes
before the program is spawned.** Which stages those are is already written down: a command whose
declared input admits `any`, `string`, `bytes` or `null` is defined over the §12.3 boundary and
keeps the bytes — `from`, `to`, `format`, `view`. Everything declared `stream<any>` is defined
over objects, and where no adapter turns the invocation into objects there are none to give it.
The refusal is `Ono-Sendai-E0911 adapter.required_for_structured_pipeline`, whose registry summary
already describes exactly this case: "A consumer demanded objects and no adapter can provide them
for this invocation."

The question is answerable from the contracts alone — the consumer's declared input and the
adapter registry — so it is asked where the pipeline is planned, before anything runs. That is
what makes `yes | take 1` answer: the producer is never started, so there is nothing to wait for
and nothing to end.

**3. The refusal carries the three routes out**, as the forced form of the same refusal already
did (spec v0.3 §1.18): `raw <invocation>` runs the program as typed, `<invocation> | from <format>`
decodes its output explicitly, and `get command <program>` lists what adapts it. It quotes the
invocation as the user wrote it rather than a re-expansion, so every route is a line they can run.

**4. `adapt` forcing structure is now the special case of a general rule, not the only case.**
Before this, the demand for structure was refused only when the `adapt` keyword raised it. The
demand belongs to the *consumer*: `ls | count` asks the same question `adapt ls | count` asks, and
gets the same answer.

## Consequences

- `yes | take 1`, `ls | count` and `seq 3 | take 1` all answer at once, and two of them stop
  answering something false. `find /usr -type f | take 1` is untouched: `find` binds as
  `ono.file.find`, so objects reach `take`.
- `lsblk | where type == "disk"`, `ps aux | sort memory desc`, `journalctl -n 2 | take 1` are
  untouched — an adapter gives those invocations objects, which is the whole point of the v0.3
  layer, and the refusal only fires where no adapter did.
- `echo '…' | from json`, `printf … | to json`, `curl … | ono -c 'from json | …'` are untouched:
  those stages declare bytes and get bytes.
- A user who wants a program's text in the object pipeline must say how it becomes objects. There
  is no `from text`, and this ADR does not add one: inventing a line-splitter would be the
  text-shaped coupling the object pipeline exists to remove.
- Tests: `crates/ono-cli/tests/native.rs` —
  `should_refuse_a_program_whose_bytes_cannot_become_the_objects_the_next_stage_needs`,
  `should_answer_at_once_when_an_endless_program_feeds_a_stage_defined_over_objects` (B-data-9's
  exit test, with a liveness bound), and
  `should_still_carry_a_whole_document_across_the_boundary_into_a_parser`, which holds decision 1.

## Alternatives considered

- **Stream the carry into the native segment**, reusing the machinery ADR-0059 built for
  streaming adapter decoders. Rejected: it answers a different question. That machinery streams
  *records* a decoder produced; here there is no decoder, and streaming raw bytes only helps if
  the bytes are cut into values, which decision 1 rejects on determinism grounds. Building it
  would have made `yes | take 1` return a 64 KiB slab of `y\n` whose length depended on the
  machine, and called it a stream.
- **Let the wrap stand and only bound the read.** Rejected: it keeps `ls | count` answering `1`,
  which is worse than a refusal because it is a plausible number.
- **Refuse at run time rather than at plan time.** Rejected: with `yes` upstream, run time never
  arrives. Spec §11.3's "a typo in the third stage never leaves the first two half-done" is the
  same argument — what the contracts can answer is answered before anything is spawned.
