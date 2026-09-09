# ADR-0642: The privacy floor is structural, and a withheld field is null

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §10.1, §10.6, §17.5, §30.1, §30.3, §30.4, §30.5; v0.2 §10.5, §20.1
- Decided by: agent (autonomous)

## Context

§10.6's second list is seven prohibitions: no arbitrary stdout or stderr bodies, no complete file
contents, no shell environment dumps, no secrets, no command-line arguments known to contain
secret values, no raw network packet payloads, no unlimited metrics samples. §30.1 says why the
default has to be conservative: "temporal retention increases privacy risk because harmless
current-state facts become a behavioral history when persisted."

Two questions the specification does not answer. What does a withheld field *read as* afterwards,
and where in the pipeline does the withholding happen.

The second is the one that decides whether the first matters. A redaction applied at render time
is a redaction the ledger's bytes do not have, and §30.3 says "values typed as `Secret` MUST be
redacted **before persistence**".

## Decision

### 1. Redaction happens where a record becomes an event, and there is no other path

`Redaction::record` runs inside `Normalizer`, which is the only way a `RecordValue` becomes a
`TemporalEvent` in this crate. An unredacted record therefore has no route into the ledger: not
through `from_provider`, not through `from_snapshot_diff`, not through `record_observation`.
`crates/ono-recorder/tests/redaction.rs` proves the outcome the way it has to be proved — by
scanning the database file's bytes for the secret.

### 2. A withheld field is `Value::Null`, never a trimmed value and never a removed field

`RecordValue::access` reads `Null` back as `FieldAccess::Unknown` (v0.2 §10.5), which is exactly
true: nobody wrote it down. An empty string would read as a value, a removed field would break the
schema, and a truncated one would be a partial file content, which §10.6 forbits as firmly as a
whole one.

The consequence a reader has to understand is that a historical process carries `command: null`
whether the argv was withheld or the kernel would not show it. That is the same three-way
distinction v0.4 already draws everywhere, and the alternative — a marker value saying "withheld"
— would be a fabricated value in a tree that has none.

### 3. Everything that is not withheld passes `ono_history`'s capture-group patterns

`ono_history::policy::Policy` is reused rather than reimplemented. Its discipline is the one that
matters: the pattern matches its context and replaces only the last capture group, so
`--password=hunter2` persists as `--password=<redacted>` and the command stays readable. A second
implementation would be a second list of secret spellings to keep in step, and the one that got
out of date would be the one that mattered.

It runs through `Value::map_text` over the whole record, so a secret is caught wherever it is: in
a field, in a list, in a map, in a nested record. `RedactedCommandSummary` in `ono-temporal-core`
handles §17.5's action summaries, where the caller states what an argument *is* and the raw text
never enters the type at all.

### 4. Argv is one switch, and turning it on does not turn the patterns off

`temporal.record.process_argv` is `false` (§30.4), and `command`, `cmdline`, `argv` and `args` are
null while it is. Turning it on changes what the ledger *contains*; the secret patterns still run
over what it now contains, because §30.3 is about secrets and §30.4 is about argv, and they are
two rules rather than one.

The withheld list is by field name, and over-withholding is the safe direction: a null costs a
reader one live query, a persisted secret costs rather more.

## Consequences

`WITHHELD_FIELDS` is a public constant, so a provider that invents a body field can be checked
against it and the contract's `must_not_persist` list has a counterpart a test can read. A field
the list does not name and should is a defect with a one-line fix.

§30.5's rule about network data needs nothing further: the endpoint metadata it permits —
protocol, local endpoint, remote endpoint, process identity — are ordinary fields, and no schema
in the tree carries a packet payload. `payload`, `packet` and `packets` are on the list anyway, so
one that arrived would be null rather than kept.

Tests: `crates/ono-recorder/tests/redaction.rs`, `crates/ono-recorder/tests/sources.rs::should_redact_before_a_record_becomes_an_event`.

## Alternatives considered

- **Redact at read time.** Rejected outright: §30.3 says "before persistence", and a ledger whose
  bytes hold the secret has already lost.
- **A `Secret` value type.** Rejected: `ono-value` has none (there is no `Value::Secret`), adding
  one is a v0.2 change, and the typed-argument shape `Redactable` already gives §17.5 what it asks
  for on the path that has typed arguments.
- **An allow-list instead of a deny-list.** Rejected as the default: a provider that adds a field
  would have it silently dropped until somebody noticed, which fails in the direction of losing
  history rather than of keeping too much. The deny-list fails in the direction of keeping a field
  that should have gone, which is a defect a review can find and a test can pin.
