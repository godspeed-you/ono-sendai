# ADR-0914: A compiled component is compiled for the architecture, not for the machine

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.2 §31.36; ADR-0863 §3, ADR-0870
- Issues: #126
- Decided by: agent (autonomous)

## Context

ADR-0870 moved the KUANG/11 compiler out of the shell: `kuang-compile` writes a `.cwasm` once,
and `ono` maps it. The engine both of them build was configured without a target, and wasmtime
then asks Cranelift to infer every optional feature of the CPU it runs on.

That made an artifact a property of one machine. CI run 36001475483 showed it: the acceptance
image, built on one runner, compiles the example component into the system store
(`/usr/lib/ono-sendai/kuang-compiled`), and the group jobs load that image on other runners. On a
runner without AVX-512 case 214 was refused with

> compilation setting "has_avx512bitalg" is enabled, but not available on the host

The refusal itself was correct, but the artifact should not have been written that way. The same thing
happens to every artifact that leaves the machine that compiled it: a container image, a system
store on a disk image, a fleet provisioned from one build host, a laptop whose CPU changed under
a restored home directory.

## Decision

The engine names the host's own target triple explicitly (`Config::target`), in the one
configuration `ono` and `kuang-compile` share (`ono-kuang-supervisor::wasm::engine`). With a target
named, wasmtime infers no CPU features, and Cranelift compiles for the architecture's baseline.
Such an artifact loads on every machine of that architecture and operating-system ABI, and it is
still refused when the architecture or the engine version differs (ADR-0870).

`target-lexicon`, already in the graph through wasmtime at the same version, becomes a direct
dependency of `ono-kuang-supervisor` so the triple is the one wasmtime itself calls the host.

## Consequences

- An image built on any x86_64 machine runs its precompiled components on every x86_64 machine.
- Compiled code gives up the optional instruction sets (SSE3 onwards, AVX, BMI, …): some float
  and SIMD operations become libcalls or longer sequences. KUANG/11 components here are
  adapters, providers and views, where that difference is not measurable against their I/O; a
  future per-machine opt-in (`kuang-compile --native`) would be a separate decision with its own
  refusal story.
- Test: `ono-kuang-supervisor` (`--features compiler`),
  `compiled::portable::should_write_an_artifact_that_loads_on_a_machine_of_the_same_architecture_without_optional_cpu_features`
  compiles through `compile` and maps the artifact in an engine whose host detector reports no
  optional feature. It failed before this change with the error CI saw, for `has_sse3`.

## Alternatives considered

**Compile the example component when the container starts, not when the image is built.** It
would fix the image and nothing else: every other way an artifact travels would still fail, and a
login shell's container would pay a compile at every start.

**Name an explicit feature level (x86-64-v2).** Faster code, and a refusal on the machines that
lack it. That is a portability promise nobody has asked this project to narrow.
