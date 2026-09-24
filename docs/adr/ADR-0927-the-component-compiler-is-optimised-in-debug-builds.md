# ADR-0927: The component compiler is optimised in debug builds

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38.1; ADR-0517, ADR-0870, ADR-0905
- Issues: #126
- Decided by: agent (autonomous)

## Context

Since #126 the tests that install or load a component run `kuang-compile`, and in the gate and in
CI that is a debug build. Its Cranelift is then unoptimised: compiling the example component
(12.8 MB, a debug `wasm32-wasip2` build) took 37.8 CPU-seconds, 6.3 s of wall clock on an
eight-core workstation. CI run 36048615701 showed what that means on a four-core runner shared
with the rest of the suite: `plugins.rs::should_compile_a_component_package_when_it_is_installed_so_that_it_loads`
missed its 30 s watchdog — not a hang and not a race, a computation that needs the whole machine.

## Decision

The compiler's own crates — `cranelift-codegen`, `cranelift-assembler-x64`, `cranelift-frontend`,
`cranelift-entity`, `cranelift-bforest`, `regalloc2`, `wasmtime-internal-cranelift` and
`wasmparser` — are built at `opt-level = 2` in the `dev` profile. Nothing of this workspace is,
so debugging Ono is unchanged, and the release profile is untouched.

## Consequences

- The same compile takes 3.6 CPU-seconds and 1.1 s of wall clock (measured at load 7, with a
  container build beside it): a tenth. Every test that compiles a component gains the same.
- A cold debug build compiles those crates with optimisation once: `cargo build -p ono-kuang-sdk
  --bin kuang-compile` took 68 s after the change, and the build cache keeps the result.
- The watchdog stays at 30 s. Raising it would have hidden the cost; this removes it.

## Alternatives considered

**A longer watchdog for the install tests.** The test would pass and every run would keep paying
38 CPU-seconds per compile.

**Build the fixture component in release mode.** Smaller input, but the fixture then differs from
what a developer builds, and the other tests that compile components would still pay for an
unoptimised compiler.
