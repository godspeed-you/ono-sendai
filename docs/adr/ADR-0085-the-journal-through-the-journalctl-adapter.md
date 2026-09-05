# ADR-0085: `get journal` and `tail journal` read the journal through the `journalctl` adapter, and a provider's failure fails the run

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §8.1, §10.5, §11.1, §11.3, §16.5, §23.3, §35.3, §50; spec v0.3 §1.8, §1.37;
  ADR-0006, ADR-0012, ADR-0028, ADR-0059, ADR-0068, ADR-0076
- Decided by: agent (autonomous)

## Context

Spec §8.1 lists `journal` among the targets and §7.1 lists it among what `tail` follows, but no
section defines a command over it; `docs/contracts/commands/service.yaml` declared `ono.journal.get`
as `planned` and `ono.journal.tail` with `phase: planned`, both over an `ono.log-record/1` that
no schema file declared. Meanwhile the v0.3 adapter pack for systemd (ADR-0059) already turns
`journalctl -o json` into `ono.journal-event/1` records, so a user who typed `journalctl` got
typed events and a user who typed `get journal` got `E0101`.

Three decisions were open: how the shell reads the journal, what `get journal` emits, and what
happens to a pipeline when the journal cannot be read at all.

## Decision

### 1. The journal is read through `journalctl --output=json`, with the adapter pack's decoder

The journal has no D-Bus surface. Reading the binary files needs `libsystemd`'s `sd-journal`
(a C dependency, plus the file-format knowledge to do without it); `journalctl --output=json` is
a machine format systemd documents and keeps stable, and spec §50 admits an adapter fallback
where it is documented. `JournalProvider` (in `ono-provider-systemd`) spawns
`journalctl --output=json --no-pager --quiet [options]` in its own process group and runs the
bundled `org.ono.compat.systemd.journalctl` adapter's decoder over its stdout, line by line, so
`get journal` and an adapted `journalctl` cannot disagree about what a record is. The records
are signed by the provider (`systemd-journal`), with the adapter kept in their provenance as the
mechanism (spec v0.3 §1.8) and the invocation as their source.

`journalctl` is never parsed as prose. A non-zero exit — "No journal files were found", a user
the journal is not readable to — is `E0401 provider.unavailable` carrying what it said, never an
empty stream (spec §10.5). No `journalctl` on `PATH` is the provider's `Availability::Unavailable`.

### 2. `get journal` emits `ono.journal-event/1`; `--since`, `--boot` are pushed down

The output is the existing schema (spec v0.3 §1.37), not a new one: the journal *is* the
JournalEvent. The declared options travel to `journalctl` as its own arguments —
`--since=@<epoch seconds>`, `--boot=<n>` — and a value of the wrong type is `E0201`, never
passed through as text for `journalctl` to guess at. Remaining selectors (a context frame's
ambient one, ADR-0076) are applied in the provider. `take` is not pushed down as `--lines`,
because `-n` counts from the end and `take` from the start.

`ono.journal.get` moves from `planned` to `experimental`, phase C; its overlap with
`ono.log.get` stays recorded in the contract's `note` (ADR-0086 gives `log` its own shape).

### 3. `tail journal` is `get journal` following; `take` bounds it

`ono.journal.tail` binds to the same producer with `follow: true`, which reaches the provider
as the query option `follow` and becomes `--follow`; `--lines` becomes `-n`. The stream is
`Unbounded` (spec §11.1). A followed journal that is quiet has no next value on which the
producer could learn that `take` is satisfied, so `StreamSink::closed()` is added to
`ono-pipeline`: it resolves when the consumer dropped the stream or the pipeline was cancelled,
and the provider selects on it against its read, then stops the child. `tail journal | take 1`
returns the moment one record arrived.

### 4. A failure of the provider kind fails the run

The native runner reported partial failures on stderr and derived exit status 0 as long as any
value arrived — and `to json` over an empty stream is a value (`[]`). So
`get journal | take 1 | to json` on a box without a journal printed `E0401` and exited 0. Spec
§16.5's partial failure is *per object*; a provider that could not answer lost the answer, not an
object. Rule: **a stream failure whose kind is `provider` (E04xx) makes the pipeline's exit
status 1**, after everything that did arrive is written. Failures of other kinds keep the
per-object rule of ADR-0028/ADR-0029.

### 5. `journalctl`'s two non-string encodings of a string field decode

`journalctl -o json` writes a value that is not UTF-8 as an array of byte values and a field an
entry carries more than once as an array of strings. The adapter decoder refused both as
`E0908 adapter.schema_violation`, which turned `get journal | where priority <= 3` into a run
that failed on the first binary message. A string field now decodes the byte array lossily
(U+FFFD for what is not UTF-8) and joins repeated values with newlines; the adapter's `limits`
say so, and fixture `systemd/journalctl/binary-and-multi-valued-message` encodes it. That is a
`fix` commit of its own.

## Consequences

- `crates/ono-cli/tests/services_logs_missing.rs`: `get journal` (4 tests) and `tail journal`
  (2 tests) un-ignored. `crates/ono-pipeline/tests/cancellation.rs` encodes `closed()`;
  `crates/ono-adapter/tests/conformance.rs` the decoder fix. Acceptance case 038 exercises the
  `E0401` path in the container, which has no journal.
- `should_only_emit_recent_events_when_since_is_a_relative_timestamp` stays ignored: it writes
  `--since (now() - 1h)`, and `now()` is the language family's (`language_missing.rs`); the
  option itself is delivered and takes any timestamp value.
- Every journal query costs one `journalctl` process. Reading the files directly would be a
  `perf` increment with its own ADR, behind the same provider contract.
- `ono.journal-event/1`'s `unit` is `_SYSTEMD_UNIT`; `get journal` inside `enter service X`
  is refused by ADR-0076 because the schema carries no `service` field — `get log --service`
  is the spelling for that (ADR-0086).

## Alternatives considered

- **`sd-journal` through `libsystemd`.** Rejected for now: a C dependency and a build-time
  requirement on every machine that builds the shell, for a reading the documented JSON output
  already gives. Kept open as a `perf` increment.
- **Route `get journal` through the shell's adapter execution path (`native.rs`).** Rejected:
  that path is the evaluator's and knows argv, not queries; a provider is what `get` composes
  with (`enter`, `watch`, links, KUANG/11 bridges) and what the command table binds.
- **A new `ono.log-record/1` for the journal too.** Rejected: the journal has a schema, and two
  schemas for one record would make `journalctl | …` and `get journal | …` differ for no reason.
- **Exit 1 on any stream failure.** Rejected: it would reverse ADR-0029, which deliberately keeps
  a process that vanished mid-enumeration from failing a successful query.
