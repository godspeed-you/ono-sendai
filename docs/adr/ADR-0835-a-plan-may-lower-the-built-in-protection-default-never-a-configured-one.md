# ADR-0835: A plan may lower the built-in protection default, never a configured one

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §17.1, §17.2, §17.3, §53; ADR-0810
- Decided by: agent (autonomous)

## Context

ADR-0810 made the stricter of plan and configuration win in both directions. With nothing
configured, that meant `plan --protection off` still ran under the built-in `prefer`: the plan
planned a PREPARE action and `apply` created an asset (acceptance case 296 r4b/r4c). §17.3 says a
plan MAY override configuration, and §53's closing line forbids configuration weakening what a plan
requires — both are about what an operator wrote. The built-in default is nobody's requirement.

## Decision

An explicit `--protection` may lower the **built-in** default. It never lowers a mode the operator
configured (a configuration file or the environment) or one a profile supplies (ADR-0836): there
the stricter mode stays in force, and the plan reports that its request was raised. `ChangeSettings`
records whether `change.default_protection` was actually written; the shell passes default-layer
values to it as unset. ADR-0810's rule is otherwise unchanged.

Two command options that promised behaviour nothing implemented are settled at the same time:
`protect --refresh` is removed — `protect` always creates a fresh recovery point, so there is
nothing to refresh — and `verify --timeout` now caps every check of the run, a contract's own
shorter timeout still winning (§23.5).

## Consequences

`plan --protection off` with nothing configured plans no preparation and creates nothing; under a
configured `prefer` it stays `prefer`. Tests: `crates/ono-cli/tests/change_protection_override.rs`,
`crates/ono-change-protection/tests/settings.rs`.

## Alternatives considered

Keeping ADR-0810's symmetric rule. Rejected: it makes `--protection off` impossible to honour on a
host where the operator never configured anything, which is the case §17.3's override exists for.
