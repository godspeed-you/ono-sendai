# ADR-0006: Structured error model and error-kind set

- Status: accepted
- Date: 2026-08-26
- Spec refs: §16.1, §16.4, §16.5, §43
- Decided by: agent (autonomous)

## Context

Spec §16.1 sketches an `Error` record with a `kind` drawn from a closed list. Spec §43 gives the
stable code taxonomy. The two do not line up: §43 defines `safety.confirmation_required`,
`safety.policy_denied`, `stream.unbounded_operation`, `stream.cancelled` and
`stream.backpressure_timeout`, but §16.1's kind list contains neither `safety` nor `stream`.
`stream.cancelled` could map onto the listed `cancelled` kind; the other four cannot map onto
anything without lying about them.

Every error a user or a script sees must carry both a stable code and a kind, so the mismatch
has to be resolved before the first error is constructed.

## Decision

The kind set is the ten kinds of §16.1 plus `safety` and `stream`:

```text
resolution | permission | io | parse | type | provider | external
conflict | timeout | cancelled | safety | stream
```

Every code of §43 has exactly one kind, fixed in code and covered by an exhaustive test:

| Code | Name | Kind |
|---|---|---|
| E0001 | parse.syntax | parse |
| E0002 | parse.incomplete | parse |
| E0101 | resolve.command_not_found | resolution |
| E0102 | resolve.target_not_found | resolution |
| E0103 | resolve.ambiguous | resolution |
| E0201 | type.mismatch | type |
| E0202 | type.unknown_field | type |
| E0203 | type.invalid_unit | type |
| E0301 | io.not_found | io |
| E0302 | io.permission_denied | permission |
| E0303 | io.already_exists | io |
| E0304 | io.not_directory | io |
| E0401 | provider.unavailable | provider |
| E0402 | provider.unsupported | provider |
| E0403 | provider.schema_violation | provider |
| E0501 | external.exit_nonzero | external |
| E0502 | external.signal | external |
| E0601 | remote.unreachable | provider |
| E0602 | remote.protocol_mismatch | provider |
| E0603 | remote.host_key_changed | safety |
| E0701 | safety.confirmation_required | safety |
| E0702 | safety.policy_denied | safety |
| E0801 | stream.unbounded_operation | stream |
| E0802 | stream.cancelled | cancelled |
| E0803 | stream.backpressure_timeout | timeout |

`io.permission_denied` takes the `permission` kind rather than `io` because §16.1 lists
`permission` as a kind of its own and no other code would ever carry it. `remote.host_key_changed`
takes `safety` because it is a trust decision, not a transport failure.

The rendered form of a code is `Ono-Sendai-E0001` exactly as §43 writes it; the dotted name
(`parse.syntax`) is the machine-readable selector used by `docs/spec/errors.yaml`, `where`
predicates over error values, and `try`/`catch` matching.

The taxonomy is **closed and additive**: codes are never renumbered, never removed and never
re-pointed at a different meaning (§43: "Codes should remain stable even if human messages
improve"). New codes take the next free number in their family.

`ono_core::ErrorCode` is a payload-free enum so that `ono-parser` and `ono-process` can name
codes without depending on the value model. The full `Error` record of §16.1 — with
`target: ValueRef`, `source`, `help`, `retryable` and `metadata: Record` — lives in `ono-value`
as `ErrorValue`, because spec §25 makes `Error` a variant of `Value`.

Partial failure (§16.5) is never an `Error`. A bulk operation yields one `ActionResult` per
target (§11.5), and the aggregate exit status is derived from them; collapsing them into a
single error is forbidden.

## Consequences

Easy: any layer can raise a coded error without a value-model dependency; `errors.yaml` in
phase D is generated from the enum rather than hand-maintained; `try`/`catch` and `where` can
match on a stable string.

Hard: adding a kind later is a breaking change for scripts matching on kind, so the set is
fixed now rather than grown ad hoc.

Encoded by: `crates/ono-core/src/error.rs` tests — exhaustive code/name/kind mapping, unique
numbers, stable rendering.

## Spec deviation

- Section: spec §16.1
- Text: "kind: resolution | permission | io | parse | type | provider | external | conflict | timeout | cancelled"
- Instead: the kind set additionally contains `safety` and `stream`.
- Why: spec §43 defines five codes in the `safety.*` and `stream.*` families that have no
  truthful kind in the §16.1 list. Forcing `safety.policy_denied` into `permission` or
  `stream.unbounded_operation` into `type` would make the kind field misleading, and the kind is
  what scripts branch on. Extending the list is additive; misclassifying is not repairable.
