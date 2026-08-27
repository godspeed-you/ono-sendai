# ADR-0065: Adapter packages under the KUANG/11 broker

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.22, §1.26, §1.27, §1.44–§1.47, §1.56, §2.3; v0.2 §31.4, §31.7, §31.16–§31.18, §31.73; ADR-0022, ADR-0040, ADR-0051, ADR-0055, ADR-0056
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.26 makes KUANG/11 the distribution mechanism for adapters and §1.22 makes an
adapter "executable knowledge and therefore a security boundary": a package that adapts `ss`
must request `process.exec` scoped to `ss` and must not turn that into general execution.
§1.45 wants simple adapters without a runtime component. The v0.2 package model (ADR-0022)
had the `adapter` role and a `process.exec` capability scoped by `programs`, but no way to
ship a pack, no scope for named executables, and no rule for what a denied grant means.

## Decision

1. **A pack is a contribution.** `contributions.adapters` lists files in the
   `ono-adapter-pack/1` format (ADR-0055) inside the package. A package with the `adapter`
   role and such contributions needs no `runtime`; it is declarative.
2. **`process.exec` gains two scope keys**: `executables` (an id-list of names or absolute
   paths) and `argv_policy` (`declared-invocations-only`). Every executable a contributed
   adapter names must be in the manifest's `executables` scope, or the package does not load:
   `adapter.capability_denied` (E0909). The pack's own `capabilities.process.exec` must agree
   with it too (ADR-0055's rule). A pack may only claim the `community` or `experimental`
   tier; first-party is what ships with the shell.
3. **Loading is validation plus policy.** `load plugin <id>` reads and validates the packs
   (`ono_kuang_supervisor::validate_package`: the contract, the fixtures under the package
   directory, the id and publisher, the tier, the executables scope) and registers them with
   the session's adapter registry. Under the default-deny policy the packs are registered
   **disabled**: their adapters answer `Negotiation::Disabled`, which is `adapter.disabled`
   (E0902) under a structured demand and the raw program at the terminal or before a byte
   consumer. `load plugin <id> --grant process.exec` — the explicit user action spec §31.18
   asks for, on the command line where it is visible — grants the capability with exactly
   the scope the manifest requested, and enables the packs.
4. **Trust tiers (§1.56).** A `community` pack is enabled by the grant alone. An
   `experimental` pack stays disabled until `--allow-experimental` is added; the reason is
   shown at load and in every refusal.
5. **The test host validates packages.** `ono_kuang_testhost::check_adapter_package(dir)`
   runs the same validation and reports the adapters, the problems, and what the default
   policy and an explicit grant would do — the check a package author runs before
   publishing (spec §31.73). The SDK ships an example declarative package
   (`crates/ono-kuang-sdk/examples/adapter-package/dev.example.users`: `getent passwd` as
   `ono.user/1`) that the test host, the shell's tests and the container all exercise.
6. **Re-loading replaces.** A registry holds one pack per id; loading a package again
   replaces its packs, so a grant given on the second load takes effect and no conflict is
   manufactured from a reload.

## Consequences

- A third-party adapter is YAML plus fixtures in a package directory, with no code and no
  process — and it cannot run anything the user was not shown at load time.
- `get plugin` still lists a declarative package as `installed`; a loaded state for
  declarative packages joins the plugin records of ADR-0051's follow-up.
- Tests: `ono-kuang-protocol/tests/manifest_validation.rs`, `ono-adapter/tests/negotiation.rs`
  (the disabled state), `ono-kuang-testhost/tests/adapter_package.rs`,
  `ono-cli/tests/plugins.rs` (denied, granted, undeclared executable, experimental),
  acceptance case `083`.
