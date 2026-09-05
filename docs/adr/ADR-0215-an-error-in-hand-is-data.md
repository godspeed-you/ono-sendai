# ADR-0215: An error a schema declares is data, not a failed read

- Status: accepted
- Date: 2026-08-29
- Spec refs: §10.5, §11.4, §11.5, §16.1, §25
- Decided by: agent (autonomous, `close-data`)

## Context

Spec §10.5 requires three field outcomes to stay apart: absent, unknown, and "could not be
read". `RecordValue::access` implemented the third by looking at the *stored value*: any field
holding a `Value::Error` was reported `FieldAccess::Failed`, and `Value::follow` turned that into
a raised error.

That rule cannot tell two different things apart:

- `ono.process/1`'s `memory` is a `bytesize`. An error stored there means the provider tried to
  read it and could not — a failed access, exactly §10.5's third case.
- `ono.action-result/1`'s `error` is declared `type: ono.error/1` (spec §11.5, field-for-field).
  An error stored there is the field's *value*: it is what a failed mutation reports, and §16.5
  requires one such result per target rather than one collapsed error.

Collapsing them made the second unusable, in three visible ways:

- `stop process 999999 | select error.name` answered a record whose `name` field held the whole
  `ono.error/1`, because `Select::project` caught the raised error and stored it as the field.
  `select error.code` answered the same thing. The path never descended.
- `try { get file /nope } catch e { echo $e.name }` re-raised the caught error instead of
  printing `io.not_found`, so the `catch e` binding of spec §16 could not be read at all.
- `where error.name == "io.not_found"` could not be written over a stream of `ActionResult`s.

## Decision

**A field whose *declared* type is `error` holds an error as its value.** `RecordValue::access`
reports it `FieldAccess::Known(Value::Error(..))`. Every other declared type, and every
undeclared extension field, keeps the old rule: a stored error there is `FieldAccess::Failed`,
and reading it raises, through `?.` as well (§10.5 is not weakened — see ADR-0014).

**A `Value::Error` reached as a value has the fields `ono.error/1` declares**, and a path step
reads them: `code`, `name`, `kind`, `message`, `target`, `source`, `help`, `retryable`, `span`,
`metadata`. A step naming anything else is `type.unknown_field`, or null under `?.`, like any
other record. `source` yields the nested error, so `e.source.name` walks the chain of §16.1.

The three §10.5 outcomes are therefore decided by the schema, not by the bytes in the slot —
which is what a schema is for.

## Consequences

- `select error.name`, `select error.code`, `where error.kind == "io"` and `catch e { … $e.name }`
  all work, over any schema that declares an `ono.error/1` field: `ActionResult`, `Plugin`,
  `AssistantTurn`, `PluginAuditEvent`, `PluginInspection` and `Error` itself.
- A provider that stores an error in a field of some other type still gets §10.5's failed access.
  It cannot opt out by accident; it opts in by declaring the field `ono.error/1`.
- `Value::follow` over a `Value::Error` no longer raises. Nothing depended on that: the raise a
  failed field produces comes from `access`, one level up, and is unchanged.

## Alternatives considered

- **Descend into any error value.** Rejected: a `memory` field that could not be read would
  answer `memory.message` instead of refusing, and §10.5's third case would have no
  representation left.
- **Keep the raise and special-case `select`.** Rejected: it fixes the one command a user
  noticed and leaves `where`, `each` and the `catch` binding broken. The defect is in the field
  model, not in the projection.
- **A distinct `FieldAccess::Error` variant.** Rejected as generality with no second use: the
  value *is* known, and `Known(Value::Error(..))` says so with the vocabulary that already exists.
