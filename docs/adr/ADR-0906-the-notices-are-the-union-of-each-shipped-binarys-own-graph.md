# ADR-0906: The notices are the union of each shipped binary's own graph

- Status: accepted
- Date: 2026-09-24
- Spec refs: ADR-0608, ADR-0870, ADR-0905
- Decided by: agent (autonomous)

## Context

`THIRD-PARTY-LICENSES` is generated from the dependency graph of `ono-cli`. That graph was read
from `cargo metadata`, whose resolution covers the whole workspace with features unified across
members. As a result it listed crates only another member's features pull in, among them the
wasmtime compiler crates that ADR-0870 removed from `ono`. ADR-0870 recorded this as its second
open point. Since ADR-0905, `kuang-compile` also ships, and it does contain the compiler, so its
licences really are distributed now.

## Decision

**The notices cover exactly the union of the graphs of the shipped packages, `ono-cli` (`ono`)
and `ono-kuang-sdk` (`kuang-compile`). Each graph is resolved the way its own release build
resolves it: by `cargo tree -p <package> --edges normal,build --target all`.**

- `xtask::notices::SHIPPED` names both packages. `dependencies_of(root, members)` runs
  `cargo tree` once per member and takes the union of the results. It then looks each crate up
  in `cargo metadata` for its licence expression and manifest directory, and drops workspace
  members. Dev-dependencies are still excluded and build dependencies still included. The graph
  is still the union over all target platforms.
- A package's binaries share its normal dependencies, so the SDK's graph is the graph of
  `kuang-compile`.
- The file says in its preamble that it covers both binaries, each resolved separately.

## Consequences

- The regenerated file no longer lists crates that neither build links (for example `addr2line`,
  `cpp_demangle`, `defmt*`, `embedded-io`, `rustc-demangle`, `wasmtime-internal-jit-debug`). It
  still lists Cranelift, because `kuang-compile` links it. It gains no crate the old file lacked.
- `xtask/tests/notices.rs::should_owe_exactly_the_crates_each_shipped_binary_links_resolved_on_its_own`
  builds a scratch workspace with two members that share an engine whose compiler sits behind a
  feature. It checks that one member's graph excludes the other member's feature, that the union
  includes it once with its licence text, that a dev-dependency is excluded, and that a build
  dependency is included. This replaces the unit test of the old graph walk, which checked
  dev/build handling only.
- Generating the file now runs `cargo tree` twice. It still needs no network (`--locked`).

## Alternatives considered

- **Keep the workspace graph.** It overstates what is shipped, and after ADR-0905 it can no
  longer be treated as a harmless superset: it names crates that are not in either binary.
- **Resolve features ourselves from `cargo metadata`.** That would re-implement cargo's feature
  resolver. `cargo tree -p` is cargo's own answer to the question.
