# ADR-0448: The execution tier is a name, and it sits beside the manifest's tier

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §15.1, §16.5, §17.1, §17.2, §17.3, §19.2, §54.2, Appendix D; base spec
  §31.10, §31.16, §31.36; AGENTS.md §7 (contract before implementation); ADR-0107, ADR-0442,
  ADR-0445, ADR-0447
- Decided by: agent (autonomous)

## Context

v0.4.1 §17.2 asks the execution-tier model to make stronger isolation possible later without
changing plugin protocol semantics, names three tiers — `native-confined`, `native-isolated`,
`wasm` — and gives one negative rule:

> The v0.4.1 code SHOULD avoid boolean names such as `sandboxed: true` that cannot represent these
> distinctions.

This repository had no `sandboxed: bool`. What it had was a field called `isolation` on
`ono.plugin/1` and `ono.plugin-runtime/1`, carrying spec §31.10's manifest vocabulary —
`core-built-in`, `trusted-native`, `isolated-component`, `remote-service` — which is a *different
question* wearing the answer's clothes. `trusted-native` says what kind of artifact the package
declared; it says nothing about what was installed around it, and two of its four values
(`trusted`, `isolated`) are precisely the words §19.1 fixes and §17.3 restricts.

So the boolean §17.2 warns about was present in substance: one field asked to answer both "what is
this thing" and "what is it running inside", and could answer neither honestly.

## Decision

**`execution_tier` is a new field beside `isolation`, and the two answer different questions.**

- `isolation` keeps spec §31.10's manifest vocabulary and its meaning: what kind of thing the
  artifact is, as the package declared it. Its documentation now says so, and says that it is not
  a statement about what is installed.
- `execution_tier` is the named tier a loaded instance actually runs in —
  `native-confined | native-isolated | wasm | remote-service | declarative | core-built-in` — and
  it is the name that reaches audit, diagnostics and documentation.

Keeping both rather than replacing one is the point, not a compromise. A `wasm-component` package
on a build with no component runtime declares one tier and runs in neither; spec §31.36 already
insists that "what can this code do even if I do not trust it" is a different question from "do I
trust it", and this is the same insistence one level down.

Three properties follow:

**A name that resolves to a table.** `ExecutionTier::NativeConfined` is not a synonym for
"sandboxed": it is a key into the central control table of ADR-0442, and `inspect plugin` shows
the rows — `{control, required, attempted, result, platform_detail}` — beside the name. That is
what a boolean could not do and what §17.3 asks a tier name to carry.

**A name that states what it is not.** `ExecutionTier::boundary()` is §15.2's statement, and
`inspect plugin` renders `execution_boundary` from it rather than from prose typed beside it
(§19.2). The record and the documentation are the same string.

**A name this build refuses to offer.** `native-isolated` and `wasm` are declared, with
`available: false` and no control rows. §17.2 asks that the model be able to express them; §17.1
does not require implementing them and §17.3 forbids describing isolation that does not exist.
A name the code refuses to select is the first without being the second.

**Appendix D's `not_provided` rows stay out of the spawn report.** `filesystem_isolation` and
`network_isolation` are properties of the tier, and they live in the tier's table where a reader
goes to ask what the tier is. A per-spawn report listing them would invite exactly the inference
Appendix D closes by forbidding: "The UI/documentation MUST never infer the last four rows from
the first rows."

The contracts changed first (AGENTS.md §7 step 1): `docs/contracts/schemas/plugin.v1.yaml` and
`docs/contracts/schemas/plugin-runtime.v1.yaml` gained the fields, the runtime record's default view
shows `execution_tier` where it showed `isolation`, and `cargo xtask docs` regenerated the
reference page.

## Consequences

Easy: `inspect plugin <id> | select runtime` answers what tier the instance is in, what that tier
is not, and which controls are in force — without `RUST_LOG=debug`, which is §54.2. A future
`native-isolated` tier adds rows to one table and a variant to one enum; no caller has to learn
that a boolean grew a third value.

Hard: two adjacent fields with related names is a genuine cost, and a reader who skims will read
`isolation: trusted-native` and stop. The doc strings on both fields exist to answer that, and the
default view now shows `execution_tier`. Removing `isolation` would be the cleaner shape and is a
schema break; it belongs to whichever increment revisits `ono.plugin/1`'s version, not to a P2.

Also: `Sandbox` carries the tier, so the type is no longer "the native process sandbox" but "the
confinement of an instance in a named tier". The name `Sandbox` is now the least accurate
identifier in the crate. Renaming it is a pure refactor and deliberately not part of this
increment (AGENTS.md §4).

Encoded by: `crates/ono-kuang-supervisor/tests/confinement.rs::should_report_a_named_execution_tier_rather_than_a_sandboxed_boolean`,
`crates/ono-cli/tests/plugins.rs::should_show_the_execution_tier_and_its_controls_when_a_plugin_is_inspected`,
`xtask/tests/contracts.rs::should_match_the_confinement_control_table_against_the_runtime_that_serves_it`,
case `189-kuang-confinement-fail-closed`.

## Alternatives considered

**Replace `isolation` with `execution_tier`.** Cleaner, and a break in a stable schema for a P2
naming change. It also loses a fact: what the manifest declared is a separate, checkable claim,
and an instance whose declared tier and actual tier disagree is exactly the case a future
`wasm-component` build has to be able to report.

**Reuse `isolation`'s values for the new field.** `trusted-native` and `isolated-component` are
the two words §19.1 fixes and §17.3 restricts, used of a tier that is neither trusted nor
isolated in those senses. §17.2 gives three better names and this takes them.

**A `sandboxed: bool` with a `sandbox_kind: string` beside it.** The shape §17.2 names and rejects
in the same sentence. Two fields where one name suffices, and the boolean would be the one every
caller read.

**Declare only `native-confined`, and add the other tiers when they exist.** §17.2 asks the model
to make them possible without changing protocol semantics, and a model that cannot name them
cannot demonstrate that. `available: false` is what keeps naming them from becoming offering them.
