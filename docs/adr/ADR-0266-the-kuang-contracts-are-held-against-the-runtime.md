# ADR-0266: The KUANG/11 contracts are held against the runtime

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.7, §31.8, §31.16, §31.79, §36.5
- Decided by: agent (autonomous, `close-spat`)

## Context

Every registry under `docs/contracts/` is held against the code that serves it —
`check_error_registry`, `check_commands`, `check_spatial_registry`,
`check_provider_claims` — except the seven `docs/contracts/kuang/*.v1.yaml`, which reached
`spec-check` only through the generic sweep. The sweep proves a file is non-empty valid YAML and
nothing else, so the KUANG/11 contracts could say anything at all about a runtime that did
something else, and §36.5's drift rule would not notice.

## Decision

`xtask::contracts::check_kuang_contracts` compares four of the seven with `crates/ono-kuang-*`,
in both directions — a contract entry the runtime does not carry is drift, and a runtime item the
contract does not declare is drift too:

1. **`capabilities.v1.yaml` ↔ `Capability`** — every family's id, `risk`, `elevation`, scope-key
   names and each key's `enforcement`.
2. **`errors.v1.yaml` ↔ `KuangErrorCode`** — every condition's dotted name, rendered code and
   §43 kind.
3. **`lifecycle.v1.yaml` ↔ `PluginState`** — every state, and its `code_has_run` answer, which is
   the sentence §31.8 exists for ("KUANG/11 MUST distinguish package presence from code
   execution").
4. **`manifest.v1.yaml` ↔ the package parser** — the fields of every closed section.

**The manifest check asks the parser rather than mirroring it.** A section probed with a key
nothing declares answers with the list of keys it does accept, because every closed section is a
`deny_unknown_fields` struct. That list is the comparison, so no second copy of the manifest shape
exists to go stale — which is exactly the failure mode a drift check is meant to prevent, and one
a hand-written table in `xtask` would have reintroduced.

**Two sections had to be closed for that to be true.** `remote` and `assistant` were
`Option<Json>`: the contract declared them `closed: true` and the parser accepted any key at all,
so a typo in either passed §31.7's "unknown field in a closed section fails closed". They are now
structs whose fields are individually opaque — a local-only supervisor still preserves the
declaration without interpreting it — and whose key set is closed.

**`contributions.v1.yaml`, `protocol.v1.yaml` and `assistants.v1.yaml` are not checked yet.** Each
describes a surface this build implements in part, so a comparison would have to encode which part,
and a check that passes by not looking is worse than none. They stay open work, said so here rather
than quietly counted as covered.

## Consequences

- `PluginState::ALL` and the two closed manifest sections are new public surface; the first is
  what makes the lifecycle comparison possible from outside the crate.
- Adding a capability family, a K-code or a package state now requires the contract and the code
  to move together, which is the point.
- `xtask` gains a dependency on `ono-kuang-protocol`, as it already has on `ono-core`,
  `ono-command` and `ono-spatial-core` for the same reason.
- Encoded by `xtask/tests/contracts.rs::should_match_the_kuang_contracts_against_the_runtime_that_serves_them`,
  `::should_report_a_kuang_manifest_field_the_runtime_does_not_implement` and
  `::should_report_a_kuang_capability_the_runtime_does_not_know`.

## Alternatives considered

- **A hand-written table of manifest fields in `xtask`** — a third copy of the shape, drifting
  from both the contract and the parser.
- **Generating the contract from the code** — inverts the authority order: `docs/contracts/` is the
  public contract and the code is measured against it, not the other way round (AGENTS.md §5).
