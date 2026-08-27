# ADR-0086: `get log` is the journal seen as log records — `ono.log-record/1` adds the severity name

- Status: accepted
- Date: 2026-08-27
- Spec refs: §8.1, §11.3, §33.2, §41.4, §50; spec v0.3 §1.37; ADR-0012, ADR-0085
- Decided by: agent (autonomous)

## Context

Spec §33.2 and §41.4 both write `get log --service <ref>`, and §41.4 filters the result with
`where level >= error`; §8.1 lists `log` as a target. `docs/spec/commands/service.yaml`
declared `ono.log.get` over `ono.log-record/1`, a schema that existed nowhere and was not
deferred either — `docs/STATE.md` listed the gap. ADR-0085 delivers the journal itself as
`ono.journal-event/1`, which has `priority` (0–7) and no `level`, so the specification's own
log example could not be written over it.

## Decision

1. **`ono.log-record/1` is written** (`docs/spec/schemas/log-record.v1.yaml`): every field of
   `ono.journal-event/1`, same identity (`cursor`), plus `level` — the severity name of
   `priority` (`emerg alert crit error warning notice info debug`), null when the journal
   reported no priority. Its default view is `timestamp level unit message`. It is a *view*
   of the journal entry, not a second source: the same `journalctl --output=json` read, the
   same decoder, reshaped by the provider.

2. **`get log` is delivered by the journal provider** (`systemd-journal`, ADR-0085) over the
   target `log`. `--service <ref<ono.service/1>>` becomes `journalctl --unit=<name>`;
   `--level <name>` becomes `--priority=<name>` after the spellings a user writes are mapped
   onto journalctl's (`error`/`err`, `warn`/`warning`, `critical`/`crit`, `emergency`/`emerg`);
   a name that is none of them is `E0201` listing the eight. `--since`/`--until` travel as
   epoch seconds. `ono.log.get` moves from `phase: planned` to phase C.

3. **Both targets stay.** `journal` is the record as the journal holds it — what
   `journalctl | …` also yields, so the two never disagree; `log` is what the specification's
   workflows name, with the severity a person filters on. The contract's `note` on
   `ono.journal.get` records the overlap for the day one of them is withdrawn.

## Consequences

- `crates/ono-cli/tests/services_logs_missing.rs`: `get log` (3 tests) un-ignored;
  acceptance case 038 shows the `log` view's severity names over the recorded fixture and the
  `E0401` answer where no journal exists.
- `should_run_the_failed_service_example_when_a_level_threshold_composes` — the §41.4 example
  `get log --service X | where level >= error | take 20` — stays ignored. The command runs; the
  expression does not: ADR-0009 makes a bare identifier in expression mode a field path, so
  `error` is `E0202 unknown field` before execution, exactly as the specification's other
  examples `where state == failed` (§33.2, §41.4) and `where status == failed` (§16.5) are today.
  That is a language decision — whether an unknown bare word compared against a string field
  is the word — for the language family to take with an ADR superseding ADR-0009 in part;
  `level` is deliberately a string so that decision, whichever way it goes, makes the example
  run unchanged. `docs/STATE.md` carries it under *Next up*.
- `level` compares as text, not as severity order. A `where level >= error` that is meant as
  "error or worse" is `where priority <= 3` today; an ordered severity type would be a value
  model increment.

## Alternatives considered

- **Add `level` to `ono.journal-event/1`.** Rejected: the adapter contract of spec v0.3 §1.37
  enumerates that schema's fields, and every adapted `journalctl` record would then carry a
  field the tool did not report.
- **Make `level` an int (the priority).** Rejected: `where level >= error` would be a type
  mismatch at run time even once bare words compare as strings, and §41.4 is the contract.
- **Withdraw `ono.journal.get` now.** Rejected: the journal-as-journal is what composes with
  the adapted `journalctl` and with `tail`; withdrawing either is a decision for the stable
  contract, not for this increment.
