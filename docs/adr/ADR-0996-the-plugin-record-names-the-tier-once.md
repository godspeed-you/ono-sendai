# ADR-0996: The plugin record names the tier once — `ono.plugin/2`

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §10.4, §27.3, §31.8, §31.10, §31.36; v0.4.1 §17.2, §17.3; ADR-0022, ADR-0107,
  ADR-0448; issue #199
- Decided by: agent (autonomous)

## Context

ADR-0448 added `execution_tier` beside `isolation` on `ono.plugin/1` and named the cost: two
adjacent fields answering related questions, a reader who skims stopping at
`isolation: trusted-native`. It left removing one of them to "whichever increment revisits
`ono.plugin/1`'s version", because removing a field is a breaking change (spec §10.4).

The two fields do not answer two questions on this record. `isolation` is the manifest's
`runtime.kind` translated into spec §31.10's vocabulary — a function of the manifest, as
`execution_tier` is for a package with no instance. Both answer "what does this package run in";
only `execution_tier` answers it in the words v0.4.1 §17.2 fixes, and `trusted-native` and
`isolated-component` are the words §17.3 restricts. ADR-0448's reason for keeping both — a
declared tier that disagrees with the actual one — is a fact about the manifest, and the manifest
is already on `inspect plugin` (`manifest.runtime.kind`), beside `isolation_statement` and the
control rows.

The repository has no precedent for bumping a schema: every contract is at version 1. Spec §10.4
makes removing a field a breaking change and §27.3 identifies a schema by `id/version`; the build
registers every `docs/contracts/schemas/<name>.v<N>.yaml`, and `spec-check` requires every
`ref<…>` and command `output` to name a schema that exists.

## Decision

**`get plugin` answers `ono.plugin/2`: `ono.plugin/1` without `isolation`.** `execution_tier` is
the one field on the record that names what an instance runs in.

- The contract moves from `docs/contracts/schemas/plugin.v1.yaml` to `plugin.v2.yaml`
  (`id: ono.plugin/2`). Version 1 is removed rather than kept beside it: nothing in the shell
  produces it any more, and a registered schema without a producer is metadata without an
  implementation (spec §36.5). The identity `[id, version]` is unchanged, so a reference to a
  package version means the same object under both versions.
- Every contract that names the record follows it: the `ono.plugin.*` commands' `input` and
  `output` (`docs/contracts/commands/kuang.yaml`), `ono.shell`'s advertised schemas
  (`providers/linux-procfs.yaml`), and the `ref<ono.plugin/2>` fields of the KUANG/11 schemas
  (`assistant`, `capability-grant`, `permission`, `plugin-audit-event`, `plugin-inspection`,
  `plugin-package`, `plugin-runtime`). Those records carry `{id, version}` exactly as before, so
  this is not a break of theirs and none of them changes version.
- The help page, completion metadata, the generated conformance suites and the reference pages
  are derived from the contracts and follow them. A package version as the object of an action
  (`install`, `remove`, …) is an `ono.plugin/2` object.
- A record built from a schema name takes the newest version the build registers under that name
  (`kuang_host::schema`), so the version a record is built to is decided once, by the contract.
- `ono.plugin-runtime/1` keeps both fields: issue #199 is about the package record, and the runtime
  record of a live instance is where declared and actual tier sit side by side on purpose
  (ADR-0448).

## Consequences

Easy: one field, one answer; the table and its JSON no longer carry a tier-shaped word §17.3
restricts. A later bump has a precedent: move the file, change the `id`, follow the references.

Hard:

- **Breaking change for anyone reading `get plugin` as data**: a script that selects `isolation`
  now gets a missing field. The manifest's own word is `inspect plugin <id> | select manifest`
  (`runtime.kind`); the tier is `execution_tier`. This belongs in the migration notes of the
  release that ships it.
- The `ono.plugin` schema id changes from `/1` to `/2` wherever it is printed (help, `get command`,
  reference pages).

Encoded by `crates/ono-cli/tests/plugin_record.rs`:
`::should_name_the_tier_an_instance_runs_in_with_one_field_when_plugins_are_listed` (the record is
exactly the shape `plugin.v2.yaml` declares, without `isolation`),
`::should_say_get_plugin_answers_the_second_version_of_the_plugin_record`; the generated
`crates/ono-cli/tests/provider_conformance.rs::should_shape_ono_plugin_2_the_way_ono_shell_declares_it`;
`crates/ono-cli/tests/plugin_commands.rs` (the `get plugin` record test now holds
`execution_tier`); acceptance case `140-plugin-signatures` selects `execution_tier`.

## Alternatives considered

**Remove `execution_tier` and keep `isolation`.** `isolation` is the field whose values §17.3
restricts and §17.2 replaces; keeping it would undo ADR-0448.

**Keep both and only document them better.** That is where ADR-0448 stopped, and the issue exists
because the documentation does not make one field answer one question.

**Keep `plugin.v1.yaml` beside `plugin.v2.yaml`.** The shell would advertise a schema nothing
produces, and every `ref<ono.plugin/…>` would have two candidates for the same identity.

**Bump every schema that references the record.** Their fields carry `{id, version}`, unchanged;
renumbering seven records for a reference to an identity that did not change would be a break no
reader can observe — the reasoning ADR-0553 applied to `ono.filesystem/1`.
