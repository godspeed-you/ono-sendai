# ADR-0109: `install plugin` in the evaluator, `remove plugin` on the mutation road

- Status: accepted
- Date: 2026-08-27
- Spec refs: §11.5, §17.4, §31.9, §31.35, §31.81; ADR-0051, ADR-0068, ADR-0107, ADR-0108
- Decided by: agent (autonomous)

## Context

Spec §31.9 makes installation a plan shown before any mutation, confirmed explicitly, and never
a capability grant; §31.81 makes removal explicit about what it retains. Both answer
`ono.action-result/1`. The mutation road of ADR-0068 gives `remove plugin` everything it needs
— a provider that advertises `plugin.remove` and answers `remove` in `act`, per-target rows,
piped objects, `--dry-run`. It cannot give `install plugin` what *it* needs: the refusal a
script gets without `--confirm` must reach stderr as `safety.confirmation_required`, and a
provider's refusal inside `act` is a failed row on stdout, not an error the run reports.

## Decision

### 1. `install plugin <reference> [--confirm]` runs in the evaluator

Like `load plugin` and `verify plugin` (ADR-0108 §2), the evaluator claims it and seeds the
pipeline with the result. In order: the reference is resolved (`path:<directory>` is the
scheme this build supports; an installed id verifies what is there); the package is verified
and a blocking check refuses before any plan is shown (ADR-0015 rule 4); an already installed
identical version refuses with `io.already_exists` — `ono.plugin/1`'s identity is
`[id, version]` and a version is never silently replaced (spec §31.35); then the install plan
of lifecycle.v1 is built. Interactively it is printed and `proceed? [y/N]` is asked; in a script
the absence of `--confirm` is `safety.confirmation_required` carrying the plan in its metadata
(spec §17.4). Only then is the directory copied under the first directory of the plugin path,
by id (ADR-0051), and the management state written with the source reference and the content
hash `verify plugin` re-checks. A different installed version of the same id is replaced —
one directory per id — and its state is kept. The result is one `ono.action-result/1` row.

### 2. `remove plugin` is `act` on `ono.shell`

`ono.shell` advertises `plugin.remove`, so `ono.plugin.remove` binds to `ProviderMutation`
(ADR-0068 §3): the selector resolves through the plugin table to `[id, version]`, and `act`
unloads a running instance first (`ShutdownReason::Unload`), removes the package directory,
and removes the management state unless `--keep-state`. `--dry-run` answers `skipped` naming
the directory. Only a directory under the plugin path is ever removed; a `path:` source is a
source, not an installation.

## Consequences

- `crates/ono-cli/tests/plugins_missing.rs`:
  `should_install_a_package_from_a_path_reference_when_confirmed`,
  `should_refuse_to_install_without_confirmation_in_a_script`,
  `should_refuse_to_install_a_version_that_is_already_installed`,
  `should_remove_the_package_directory_when_removed`,
  `should_unload_a_loaded_package_before_removing_it`.
- `get plugin | where … | remove plugin` composes, as every mutation does.
- `--keep-grants` is accepted and inert until grants persist beyond the session (ADR-0111).

## Alternatives considered

- **`install plugin` through `ProviderMutation`.** The `reference` selector would have to
  resolve to an object that carries the source path, and the confirmation refusal would be a
  row, not an error. Rejected.
- **A stand-alone `install` verb implementation in `ono-command`.** It would need the plugin
  home and the state directory, which are the session's. Rejected.
