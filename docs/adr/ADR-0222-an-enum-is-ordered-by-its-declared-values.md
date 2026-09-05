# ADR-0222: An enum is ordered by its declared values

- Status: accepted
- Date: 2026-08-29
- Spec refs: §6.3, §10.2, §41.4; ADR-0096
- Decided by: agent (autonomous, `close-data`)

## Context

Spec §41.4 and the `get log` contract both write

```text
get log --service <ref> | where level >= error | take 20
```

`level` is a string, so `>=` compared it as text. `"warning" > "error"` and `"crit" < "error"`
alphabetically, so the line kept precisely the records it was written to drop and dropped the
ones it was written to keep. The documented example ran and answered wrongly.

`level` is not a string, though — `ono.log-record/1` declares it `enum` with the eight syslog
severities, and ADR-0096 already taught the expression layer to read a bare word beside an enum
field as one of that field's values. What was missing is that an enum's values are a *sequence*,
not a set.

## Decision

**An enum's `values` are written from least to greatest, and that order is the enum's order.**
An ordering comparison — `<`, `<=`, `>`, `>=` — where one side is a path to a field the schema
declares as an enum, and both sides name declared variants, compares their positions.
`sort <field>` over such a field sorts by the same positions. Equality is unaffected: two names
are equal or they are not, whatever their positions.

Every other comparison is unchanged. A `string` field compares as text, whatever it holds; a
value that is not one of the declared variants falls back to the ordinary comparison, so nothing
mixes ranks with names.

**`ono.log-record/1.level` is therefore declared `[debug, info, notice, warning, error, crit,
alert, emerg]`** — the reverse of the syslog `priority` number, which the field's doc now says.
The provider is untouched: it maps a priority to a name through its own table, indexed by
priority, and never reads the schema's order.

The other enums in `docs/contracts/` already read as ascending orders where they have one —
`finding.severity: [info, low, medium, high, critical]`, `assistant.autonomy: [L0 … L4]`,
`assistant-action.risk: [read, observe, mutate, destructive]`,
`config-setting.layer: [default, system, user, environment, invocation]` — so the rule fits what
is declared rather than requiring it to change.

## Consequences

- `where level >= error` keeps `error`, `crit`, `alert` and `emerg`, as spec §41.4 means it, and
  `sort level` orders a log from mildest to most severe.
- An enum's declared order is now part of its contract. Reordering `values` changes behaviour and
  is a schema change like any other.
- A stream with no schema — `echo '[{"level":"warning"}]' | ono -c 'from json | where level >=
  "error"'` — still compares as text, and correctly so: a plain JSON document declares no enum,
  and there is nothing that says those strings are severities. Give the stream a schema and the
  ordering follows.

## Alternatives considered

- **A `severity` type.** Rejected: it would name one enum specially, and the next ordered enum —
  a finding's severity, an autonomy level — would need another.
- **Keep `values` in priority order and read `>=` as "at least as severe".** Rejected: it makes
  `>=` mean the opposite of `>` on the same field's ordinal, which no reader can be expected to
  hold in mind.
- **An explicit `order:` key beside `values`.** Rejected as a second way to say what `values`
  already says, for no case that exists.
