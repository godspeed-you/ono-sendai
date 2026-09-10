# ADR-0826: Change settings are read key by key and reported where configuration problems are

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §17.2, §28.4, §53
- Decided by: agent (autonomous)

## Context

`ChangeSettings::from_settings` failed as a whole on the first unreadable key, and the shell then
replaced *every* change setting with its default without a word — including a
`change.default_protection = require` written correctly on the next line. It also refused
`batch 2`, the way §28.4 writes a strategy with its parameter.

## Decision

- `ChangeSettings::read(lookup) -> (settings, problems)` reads each §53 key on its own: a key that
  reads keeps the operator's value, a key that does not keeps its default and becomes a problem.
  `from_settings` keeps its contract (the first problem is its error).
- `change.default_strategy`'s kind is its first word; the parameters are the planner's to read.
- `config::load` reads the change settings once every configuration layer is in — as it already
  did for v0.5's temporal settings — reports each problem through the reporter and notes it for
  `get config --problems`. The registry-time read stays silent, so nothing is reported twice.

## Consequences

Tests: `crates/ono-change-protection/tests/settings.rs` (two new cases),
`crates/ono-cli/tests/change_settings.rs`.

## Alternatives considered

Refusing to start with an unreadable key. Rejected by ADR-0010: a bad setting never stops the
shell from starting.
