# ADR-0461: The limits a user reads are the limits the shell enforces

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §12.4, §14.2, §22.4, §52.2, §53.3, §54.3, §55.1, Appendix A; v0.2 §9.1, §30;
  ADR-0010, ADR-0093, ADR-0094, ADR-0456
- Decided by: agent (autonomous)

## Context

§54.3 asks for a diagnostic surface and immediately says what matters more than the surface:

> A diagnostic surface such as `inspect limits`, `get limit`, or existing equivalent SHOULD expose
> effective non-secret runtime limits. The exact target MUST fit the command registry; this is
> secondary to having the values accessible to tests and `explain`.

So the requirement has two halves and they are ranked. The values have to be reachable — §12.4
wants "the centralized `Limits`" printed by a diagnostic command or a test fixture, §22.4 wants
the effective budget in `explain` — and the command that prints them is the smaller half.

## Decision

### 1. `inspect limits`, answered by the evaluator

`ono.limits.inspect` is declared in `docs/spec/commands/meta.yaml` against a new
`ono.target.limits` and a new `ono.limit/1` schema, and it is answered where `get config` is
answered: the evaluator's meta seam (ADR-0093), claimed by the head word and its target.

Not `impls/inspect.rs`, which asks a provider for an object. There is no provider for "what this
shell will enforce", and registering one would be the wrong authority in the wrong place for the
same reason ADR-0010 keeps the configuration layers in the session. `impls/mod.rs` therefore
excludes `limits` from the generic producer beside `command`, `config` and `context`, and
`xtask/src/bindings.rs` records the claim, so the gate checks it in both directions.

`inspect` rather than `get` because §9.1 makes `inspect` the verb for "tell me about this thing in
detail", and a limit's range and enforcing layer are exactly that.

### 2. Every row is derived from the catalogue, so there is nothing to disagree with

`crate::limits::rows` walks `settings::CATALOGUE` for the `limits.` prefix and reports, per key:
the effective value in its own type, the same figure in base units for a byte ceiling, the
declared type, the layer that supplied it, the permitted range, and the description. All of it
from the same declaration `Settings::assign` validates against and the pipeline and the history
read — which is §52.2's requirement applied to the diagnostic rather than only to the
configuration.

The prefix rather than a list: a key added under `limits.` is a limit, and a diagnostic that had
to be told about each one separately would be the second copy the section forbids.

`should_answer_the_same_figures_inspect_limits_shows_from_the_contract_registry` closes the loop
in both directions against `docs/spec/hardening/limits.yaml` — a key the shell enforces and the
registry does not declare fails, and so does the reverse.

### 3. Non-secret, and provably so

§53.3 permits limits, fingerprints and capability ids in diagnostics and forbids secrets. The
guarantee here is structural rather than a filter: the rows come from the settings catalogue,
which holds no credentials, and every row's key starts with `limits.`. The test asserts both —
every key is a limit, and the output carries no `sha256:`, no PEM header, no bearer token — so a
future key that started carrying something else would fail rather than leak.

## Consequences

Easy: a user asks what the shell will refuse, and gets objects. A test asks the same question and
gets the same objects, so §54.3's "accessible to tests" is the same code path rather than a second
one. `explain` (ADR-0460) reads the same catalogue for its budget line.

Hard: `ono.limits.inspect` is a stable command of a delivered phase, which makes it a
compatibility promise (§4.1). The four `limits.remote_*` rows it prints are declared and validated
and not yet enforced (ADR-0456), so a user reading them sees a ceiling that nothing applies. The
registry's `enforced_by: pending` says so and the command does not — a `status` column would be
the honest addition, and it belongs with the increment that makes them enforced rather than with
this one.

Encoded by `crates/ono-cli/tests/resource_limits.rs::should_answer_the_effective_non_secret_limits_when_inspect_limits_runs`
and `::should_answer_the_same_figures_inspect_limits_shows_from_the_contract_registry`.

## Alternatives considered

**`get limit`, §54.3's other suggestion.** `get` produces objects of a target from a provider, and
these come from the session. `get config` is the precedent for the exception, and adding a second
one would make the rule "`get` reads a provider, except twice".

**Extending `get config` with a `--limits` flag.** The values are already there —
`get config limits.` answers them today, because they are settings. What that cannot show is the
permitted range and the base-unit figure, which are what makes a limit readable, and adding them
to `ono.config-setting/1` would change a stable schema for the benefit of thirteen of its rows.

**A `Limits` struct printed by a debug command.** §54.2 is explicit that important explanations
must not need `RUST_LOG=debug`, and a struct is a second copy of every number (§52.2).
