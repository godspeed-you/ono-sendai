# ADR-0629: A duration setting is typed as a duration, and the registry holds the defaults

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §10.4, §33, §36.4; v0.4.1 §52.2, §55.1, §55.2, Appendix A; ADR-0010, ADR-0094,
  ADR-0456
- Decided by: agent (autonomous)

## Context

v0.5 §33 declares twelve settings and requires them to be *"typed, inspectable and included in
machine-readable configuration metadata"*. Five of them are durations:

```text
temporal.retention.max_age = 24h
temporal.checkpoint.interval = 5m
temporal.flush.interval = 2s
temporal.timeline.default_window = 30m
temporal.remote.clock_uncertainty_warn = 100ms
```

`ono_cli::settings::SettingType` had four types — `int`, `bool`, `string`, `bytesize`. The
existing precedent for a duration is v0.4.1's `limits.completion_soft_ms`, an `int` whose unit
lives in the key name, which Appendix A justifies for hardening limits: *"Limits MUST be expressed
internally in integer base units and rendered in human-readable units separately."*

§36.4 then requires the gate to fail when *"default configuration differs from registry"*, which
needs the registry and the catalogue to state the same figure in comparable form.

## Decision

### 1. `SettingType::Duration` exists, and the five duration settings use it

`ono_value::Duration` is already the value model's span type, with `parse` accepting `24h`, `5m`,
`1h30m` and `100ms`, and the arithmetic on `Timestamp` already defined. A setting typed as a
duration reads back from `get config` as a duration, `set config temporal.flush.interval 2s`
parses in the declared type the way every other typed parameter does (ADR-0070), and the key names
stay the ones §33 spells rather than growing a `_ms` suffix the specification does not have.

The `_ms` convention stays where Appendix A put it: `limits.*`, whose values are range-checked
security boundaries expressed in integer base units.

### 2. The temporal settings declare no range

v0.4.1 §55.2 makes range checking mandatory for `limits.*`, and its binding sentence is about
security: a limit that failed to parse must not silently become unlimited. The temporal settings
are not that family. `range: None` is the state every key predating §55 is in, and adding invented
bounds would be a second contract nobody derived from a specification.

### 3. The registry writes defaults the way a user types them

`docs/contracts/temporal/temporal.yaml`'s `settings:` block carries `key`, `type` and `default`,
with the default written as `24h`, `512MiB`, `100000`, `false`. The gate parses each in its
declared type and compares the resulting `Value` against `SettingSpec::default_value()`, so `24h`
here and 86_400_000_000_000 nanoseconds in the catalogue are one figure rather than two that
happen to agree — v0.4.1 §52.2's rule, applied to the temporal family.

The comparison runs in both directions on key, type and default: a key the shell declares and the
registry omits fails the gate, and so does the reverse.

## Consequences

- `get config`, completion and `explain` report `duration` for five settings, and a value of the
  wrong type is `type.mismatch` with the earlier layer's value left in force, as ADR-0094 already
  fixed for every other type.
- Three functions in `settings.rs` gained a match arm (`name`, `accepts`, `read_word`), plus
  `with_article` and `magnitude_of`. The compiler found all of them; the type is closed.
- A sixth setting type is now a cheap addition, which was not the intent and is a real
  consequence: `Percent` and `Timestamp` are both in the value model, and neither should be added
  until a specification asks for one.
- §33's "included in machine-readable configuration metadata" is satisfied by the registry, and
  §36.4's drift rule is satisfied by the comparison. Neither is satisfied by the catalogue alone,
  because a catalogue is the shell agreeing with itself.

## Alternatives considered

**Declare the five as `int` milliseconds, named `temporal.flush.interval_ms`.** Rejected: it
renames five settings the specification spells, and §33's list is the user-facing contract.

**Declare them as `int` milliseconds under §33's own names.** Rejected: it puts the unit nowhere a
reader or a parser can see it, so `set config temporal.retention.max_age 24` would silently mean 24
milliseconds.

**Declare them as `string` and parse at each read site.** Rejected: it moves the type check from
the configuration layer to every consumer, which is the shape ADR-0094 replaced.
