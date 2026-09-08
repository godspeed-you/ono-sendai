# ADR-0611: Temporal evidence is its own schema, because the name it asked for is taken

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.4, §7, §35; v0.2 §31.24, §31.25, §31.26; ADR-0022, ADR-0012
- Decided by: agent (autonomous)

## Context

v0.5 §35 lists `ono.evidence/1` among the schemas v0.5 adds, and §3.4 gives it fields:
`evidence_id`, `source`, `observed_at`, `source_time`, `scope`, `subject`, `claim`, `strength`,
`raw_ref`, `provenance`.

`ono.evidence/1` already exists. ADR-0022 wrote it for spec §31.24's `Finding.evidence` and
§31.26's `Relation.evidence`: one inspectable observation supporting a finding, an annotation or
an assistant claim, with a required `kind` from `[object, field, samples, source, command,
unavailable]` and a required `reference` that must resolve. Findings, relations and assistant
citations carry it today, `output.finding` rejects a contribution whose reference does not
resolve, and the schema is in a released binary.

The two are different records that happen to share a word. §31.24 evidence cites an observation
inside one analysis. §7 evidence is a durable, addressable claim by a named source about a scope
at a time, with an `EvidenceId` that causal links and reconstructions reference for as long as
retention keeps them.

## Decision

v0.5's evidence record is registered as **`ono.temporal-evidence/1`**, with the field set of
§3.4 and the strengths of §7.2. `ono.evidence/1` keeps its meaning and its consumers.

Every other schema v0.5 §35 names is added under the name §35 gives it, because none of them is
taken: `ono.temporal-event/1`, `ono.temporal-context/1`, `ono.temporal-coverage/1`,
`ono.temporal-gap/1`, `ono.temporal-change/1`, `ono.causal-link/1`,
`ono.causal-explanation/1`, `ono.action-event/1`, `ono.recorder-status/1`,
`ono.temporal-source/1`.

The two evidence records may cite one another and never merge: a `Finding` that rests on
retained history carries an `ono.evidence/1` row whose `reference` is an `EvidenceRef`, and the
temporal record is what that reference resolves to.

## Consequences

- `inspect --provenance` walks temporal evidence through `ono.temporal-evidence/1`; nothing about
  finding evidence changes.
- A reader of v0.5 §35 who greps for `ono.evidence/1` finds ADR-0022's schema and this ADR.
- The `temporal-` prefix already groups the family in `docs/contracts/schemas/`, so the name is
  the one a reader would guess from its neighbours.

## Spec deviation

- Section: v0.5 §35
- Text: "ono.evidence/1"
- Instead: `ono.temporal-evidence/1`, with the fields v0.5 §3.4 fixes.
- Why: `ono.evidence/1` is the implemented schema of v0.2 §31.24 and carries a different record.
  Re-pointing a shipped schema id at an incompatible field set is a schema break without a
  version bump, which `spec-check` refuses and which v0.5 §0.2 forbids by requiring v0.5 to
  extend the v0.4 foundations rather than duplicate or replace them.

## Alternatives considered

- **Widening `ono.evidence/1` to hold both.** Every field of one record is meaningless in the
  other, so the union would be ten nullable fields and a `kind` word deciding which half is
  real — the shape ADR-0012 §13 rejects as a registry that parses while discarding the contract.
- **Bumping to `ono.evidence/2` with the temporal fields.** A v2 says the v1 record evolved into
  this one. It did not; the two are unrelated records.
