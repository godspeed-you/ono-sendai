# ADR-0218: A union output is checked against every alternative

- Status: accepted
- Date: 2026-08-29
- Spec refs: §11.3, §16.1, §30, §43
- Decided by: agent (autonomous, `close-data`)

## Context

`get config --problems` returns the diagnostics ADR-0010 keeps when a configuration layer fails
to load. They are `ono.error/1` values — that is what a load failure *is* — but the contract
declared `output: stream<ono.config-setting/1>`, so the pre-flight field check of spec §11.3
measured the next stage against `ConfigSetting`:

```text
get config --problems | select code
Ono-Sendai-E0202 type.unknown_field unknown field `code` on ConfigSetting
```

The stage produced exactly what the user asked for and the check refused to let it run.

Declaring the union alone was not enough: `IoType::element_schema` answers `None` for a type with
two schema references, and `check_stage` then treated the element type as unknown and checked
nothing at all. That would have traded one wrong answer for a worse one — `get config | select
typo` would stop being caught.

## Decision

**`ono.config.get` declares what it can produce:**
`output: stream<ono.config-setting/1> | stream<ono.error/1>`.

**The pre-flight check carries a *set* of element schemas, not one**, and a field is unknown only
when **no** alternative declares it. The reported error is the first alternative's, so the
message still names a schema and its nearest field. An alternative that nothing advertises makes
the whole element type unknown, rather than a subset that would reject a field the missing schema
declares.

Completion (`schema_after`) is unchanged in shape: it offers a schema only where the stage
carries exactly one, because a completion list merged from two schemas would offer fields that
half the rows do not have.

## Consequences

- `get config --problems | select code name`, `… | where kind == "type"` and `… | to json` all
  work over the diagnostics as the errors they are.
- `get config | select nosuchfield` is still refused before the stage runs, naming
  `ConfigSetting`.
- A union output is now a usable declaration rather than one that silently disables the check.
  `ono.config.get` is the only command that declares one today; the rule is the same for the next.

## Alternatives considered

- **An option-scoped output override in the contract** (`--problems` declares its own output
  type). Rejected as machinery built for one option: the union already says everything true about
  the stage, and the check can read it.
- **Give `--problems` its own schema, `ono.config-problem/1`.** Rejected: it would wrap an
  `ono.error/1` in a record whose only interesting field is the error, and `select code` — the
  thing a user actually writes — would need `select error.code`.
- **Accept the union and let the check lapse.** Rejected: it withdraws spec §11.3's guarantee
  from `get config` to fix one option.
