# ADR-0569: A component runs inside the runtime Ono embeds and speaks the same protocol

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.10, §31.11, §31.15, §31.34, §31.61, §31.73, §31.74; v0.4.1 §16.1, §16.4, §17.2, §17.3, §19.1, Appendix D; ADR-0022, ADR-0040, ADR-0283, ADR-0442, ADR-0448
- Decided by: agent (autonomous)

## Context

Issue #3 (b): `RuntimeKind::WasmComponent` existed in the manifest and was never spawned;
`ExecutionTier::Wasm` was a name the model could express, `available: false`, with every
control `not_provided`. Spec §31.10 makes T2 — "WASM/component runtime with capabilities" — the
default tier for third-party code "if the required host APIs can be expressed safely", and
leaves the technology an implementation choice. The host API is JSON frames over two streams,
which any runtime with standard streams can carry.

## Decision

**1. The technology is the WebAssembly component model, through `wasmtime`,** pinned to the
release the toolchain supports. The dependency was measured before it was taken: 144 crates,
a little over a minute of release build on the reference machine, a few hundred kilobytes of
binary. The engine is one per process and shared; its configuration is the tier's.

**2. A component speaks the protocol a process speaks, over WASI stdio.** The supervisor hands
the guest a WASI context whose standard input and output are pipes the host holds, and nothing
else: no preopened directory, no permitted address, no environment. The actor that drives a
native process drives a component unchanged — a writer for its input, frames from its output,
a way to end it and learn how it ended — behind one `Runtime` with two variants. The example
package, built for `wasm32-wasip2` from the same source, passes the same conformance cases.

**3. Confinement is the runtime's, and the table says which rows it holds.** A component has,
by construction, what a process has to install: no descriptors, no filesystem, no network, no
environment, its lifetime bound to the instance, and a memory ceiling the runtime's limiter
enforces on every growth of linear memory. Those rows are `mandatory`. What only a process could
have — session separation, `no_new_privs`, the rlimits on files, cores and children, the
scheduling priority, a working directory, the kernel allowlists — is `not_provided`, written
down rather than inferred (§16.4, Appendix D). The tier is `available: true` and the manifest
kind `wasm-component` loads.

**4. There is no CPU ceiling, and the boundary says so.** The engine's epoch interruption
preempts a component at every epoch so a spinning guest cannot hold the runtime's thread, and
the store yields rather than traps — a plugin waiting on its input is not a plugin to stop.
`rlimit_cpu` is `not_provided`, and the tier's boundary sentence says a spinning component is
preempted, not stopped.

**5. Memory is exact, not sampled.** The limiter sees every growth, so the gauge the shell reads
for `inspect plugin` is the guest's linear memory as it is, and a component that reaches its
ceiling ends with `runtime.memory_limit` like a process at `RLIMIT_DATA`.

## Consequences

- The conformance suite builds the example package as a component into its own target
  directory and runs it under the test host: the tier and its controls as reported, typed
  values over the protocol streams, and the capability broker holding a component exactly as it
  holds a process. A host without the `wasm32-wasip2` target skips those three cases and says
  so; the pinned toolchain lists the target, so the canonical CI has it.
- `ono-value` reads a path's bytes through one seam with a Unix and a non-Unix side, because a
  component reads the same wire encoding and WASI paths are text.
- The example package's one platform-bound line — the parent process id — is behind `cfg(unix)`.
- Not in this increment: the acceptance image does not yet carry a component build of the
  example package, and `inspect plugin` for a component reports the process rows as the zeros
  they are. Both are the next increment's, with the case that proves them.

- The dependency policy of `deny.toml` allows two more licences, each a decision this ADR
  owns: `Apache-2.0 WITH LLVM-exception`, which `wasmtime` and `cranelift` carry and which only
  removes Apache-2.0's notice condition for binaries, and `Zlib` for `foldhash`, a leaf of the
  runtime, attribution-only like MIT. Neither asks for anything distributing an MIT binary does
  not already permit.
- `Cargo.lock` is now the union over the runtime's optional features too, so the supply-chain
  fixture that seeds an advisory against "the first registry crate" names the first one the
  active build actually resolves; a crate the lock lists and nothing activates is one
  `cargo deny` never sees.

## Alternatives considered

- **Running the runtime in a child process** so that "third-party plugin code MUST NOT run in
  the Ono process by default" reads literally. Rejected: the component runtime *is* the
  isolation §31.10 describes for T2; a process around it would add the native tier's controls
  on top and change nothing about what the component can reach.
- **Fuel metering as a CPU ceiling.** Rejected for this increment: fuel counts instructions,
  and a long-lived provider plugin would be stopped for doing its job; epochs keep the host
  responsive, which is the property §31.67 asks for.
- **A WIT interface for the host API instead of frames over stdio.** Rejected: it would be a
  second protocol beside the one every native package speaks, and ADR-0022's rule is one
  vocabulary.
