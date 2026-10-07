# ADR-0931: The registry index reaches the frozen baselines where they are

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §52.1, §52.3, §57 (H0); v0.5 §49; ADR-0547, ADR-0548, ADR-0625, ADR-0785,
  ADR-0864
- Decided by: agent (autonomous)

## Context

v0.4.1 §52.3: "`scripts/gate.sh` MUST validate every machine-readable contract for schema
correctness and cross-reference integrity." `docs/contracts/hardening/registries.yaml` is the index
that makes *every* checkable (ADR-0547): a row per contract, naming the gate function that holds it,
and a sweep that fails on a file in the directory without a row. ADR-0625 let rows reach
`docs/contracts/temporal/`, `change/` and `recovery/` by relative path — but the sweep still looked
only at `hardening/`, so a file added beside the indexed temporal registries without a row went
unnoticed.

`docs/baselines/v0.4.1.json` and `v0.5.0.json` are validated by `xtask::baseline::check` in
`spec-check`, and `docs/baselines/binary-size.yaml` by `xtask::binary_size::check_record`, yet no
row names any of them. ADR-0548's *Consequences* recorded this as the one deliberate exception
(issue #171).

Two exits: move the snapshots into the indexed tree, or let the index reach them. The paths are
read by `xtask/src/baseline.rs` (`PATH`, `DIRECTORY`), `binary_size.rs` (`RECORD`), `affected.rs`
(change selection), `scripts/binary-size.sh`, `package.sh`, `build-core.sh`, `ci.yml`, the tests of
`xtask/tests/{perf,metrics,provenance,harness}.rs`, `limits.yaml`, release notes v0.5.0 and v0.6.2
and eleven ADRs. ADR-0548's own reasoning still holds: a snapshot of a tranche is a record, not
part of the public contract surface `docs/contracts/` is.

## Decision

**The snapshots stay in `docs/baselines/`; the index reaches them, and the index's sweep covers
every directory it answers for.**

1. `registries.yaml` gains three rows — `../../baselines/v0.4.1.json`, `../../baselines/v0.5.0.json`
   and `../../baselines/binary-size.yaml` — each with its validator, consumer, spec sections and
   doc, under a heading that says why they are indexed from here.
2. `check_registry_inventory` sweeps, for unindexed `*.yaml`/`*.json` files: the index's own
   directory, every directory a row reaches (lexically normalised, so `../temporal/x.yaml` and the
   listing of `docs/contracts/temporal/` compare as one path), and `docs/baselines/`
   (`xtask::baseline::DIRECTORY`), named explicitly so that deleting every baseline row cannot
   shrink the sweep with it. A file without a row is reported by its repository path.
3. The `file` field's documentation says it is a path relative to the index's directory.

This replaces the exception paragraph of ADR-0548's *Consequences*; the rest of ADR-0548 stands.

## Consequences

- `spec-check` fails on an unindexed machine-readable file in `hardening/`, `temporal/`, `change/`,
  `recovery/` or `docs/baselines/`. The next tranche's `cargo xtask baseline --write` (ADR-0785)
  therefore writes a snapshot the gate refuses until its row is added — the same rule every
  registry already follows: a contract arrives with its validator or does not arrive.
- No path moves: release notes, ADR links, scripts, CI and `affected.rs` keep working.
- `docs/contracts/` other than the swept directories (`commands/`, `schemas/`, `providers/`, …) is
  not indexed here; each has its own completeness rules in `spec-check`, outside §52's hardening
  index.

Encoded by `xtask/tests/contracts.rs`:
`should_report_a_baseline_snapshot_the_registry_index_does_not_name`,
`should_report_an_unindexed_contract_in_a_directory_the_registry_index_reaches` (both red before),
and `should_validate_every_machine_readable_hardening_contract_in_the_gate` on the real tree.

## Alternatives considered

- **Move the snapshots to `docs/contracts/hardening/baselines/`.** Satisfies the index with no
  rule change, at the cost of rewriting every path above, breaking links in published release
  notes, and calling a historical record a contract.
- **Keep the exception and document it better.** It is the state the issue exists to end.
