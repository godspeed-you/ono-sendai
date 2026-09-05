# ADR-0096: A bare word compared with an enum field is that field's value

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.3, §10.3, §11.3, §16.5, §33.2, §41.4, §50; ADR-0009, ADR-0014, ADR-0086
- Decided by: agent (autonomous)

## Context

ADR-0009 makes a bare identifier in expression mode a field path, so `select pid name cpu`
needs no quoting. The specification also writes `where state == failed` (§33.2, §41.4),
`where status == failed` (§16.5) and `where level >= error` (§41.4), and every one of them was
`E0202 unknown field` before execution: `failed` was looked up as a field. Spec §50 requires
documented examples to run, and `docs/contracts/commands/service.yaml` carries
`get service | where state == failed` as one.

## Decision

In a comparison (`== != < <= > >=`) whose one side is a bare path naming a field declared
`type: enum`, a bare path on the other side that is **not** a field of the schema and **is** one
of that enum's declared `values` is that value — a string — not a field lookup.

- The pre-flight check (spec §11.3) applies the rule against the advertised schema, so
  `where state == failed` passes and `where state == broken` is still `E0202` naming `broken`.
- The evaluator applies the same rule against the record's own schema, so the two never
  disagree and a KUANG/11 provider that extends an enum extends both.
- A field always wins (spec §10.3): a word that names a field is that field, whatever the
  other side is. A string field's comparand stays a field lookup — `where name == foo` still
  reports `foo` — because a string has no declared vocabulary to check the word against.
- `ono.log-record/1`'s `level` becomes `type: enum` with the eight severity names, so §41.4's
  `where level >= error` runs; it compares as text, not as severity order (ADR-0086).

## Consequences

- `crates/ono-command/tests/expressions.rs` (two tests), `crates/ono-cli/tests/native.rs`
  (`where state != zombie` over processes; `sleping` still `E0202`),
  `services_logs_missing.rs::should_run_the_failed_service_example…` un-ignored (15/15).
- ADR-0009 stands: the word is still lexed as a path; only its meaning in one comparison
  against a declared vocabulary changes. Completion after `state ==` can now offer the values.
- A schema author who wants a bare-word vocabulary declares the field `enum`.

## Alternatives considered

- **Any unknown bare word compared with a string field is a string.** Rejected: it would turn
  every typo on the right-hand side into a silent non-match, the failure mode the pre-flight
  check exists to prevent.
- **Require quotes, and fix the specification's examples.** Rejected: the specification is
  immutable (AGENTS.md §5.1) and its spelling is what users will type.
