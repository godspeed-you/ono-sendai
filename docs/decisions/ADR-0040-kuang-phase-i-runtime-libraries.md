# ADR-0040: KUANG/11 Phase I runtime libraries

- Status: accepted
- Date: 2026-08-26
- Spec refs: §31.5, §31.7, §31.8, §31.11–§31.17, §31.19, §31.22, §31.23, §31.31, §31.33,
  §31.34, §31.37, §31.49, §31.59–§31.63, §31.73, §31.74, §31.79, §37 Phase I; ADR-0022,
  ADR-0006, ADR-0015
- Decided by: agent (autonomous)

## Context

Phase I needs the production path of the extension runtime as libraries: the wire types, the
supervisor, the plugin SDK and the deterministic test host of spec §31.73, proven offline by
the conformance areas of spec §31.74. The contracts of `docs/spec/kuang/` (ADR-0022) fix the
semantics; this ADR records the structural decisions the contracts leave open, and the three
places where the implementation deliberately narrows or extends them.

## Decision

### 1. Four crates

- **`ono-kuang-protocol`** — everything both sides must agree on and nothing that runs on
  either side: manifest parsing and fail-closed validation, the message envelopes and typed
  parameter shapes, framing with pre-allocation bounds, the six-state lifecycle machine, the
  twenty-nine capability families with scope shapes and enforcement levels, the negotiated
  contract, the audit record, and the K-family error taxonomy.
- **`ono-kuang-supervisor`** — the host: policy and broker, negotiation, spawn/handshake, the
  per-instance actor, the state store, the audit trail.
- **`ono-kuang-sdk`** — the plugin author's surface, plus the example plugin binary
  (`kuang-example-plugin`) that spec §31.78's `examples/` becomes now that something executes
  it (ADR-0022 §16). Its honest mode is built on the SDK; its `--misbehave=…` modes speak the
  wire raw, because a misbehaving package would.
- **`ono-kuang-testhost`** — spec §31.73's deterministic test host: the *real* supervisor with
  a fixed clock and a test-authored policy. Determinism is added; leniency is not.

The conformance suite lives in `ono-kuang-sdk/tests/conformance.rs`, because Cargo exposes a
crate's own binaries (`CARGO_BIN_EXE_…`) to that crate's integration tests; the test host is a
dev-dependency there.

### 2. Wire encoding: JSON frames carrying the tagged `ono-value` encoding

Frames are four-byte big-endian length plus a JSON envelope; values cross in the lossless
tagged encoding of `ono_value::to_json`/`from_json`, schemas referenced by id inside
`$record`. Nothing typed is flattened: a `ByteSize` crosses as `{"$bytesize": …}`, null and
absent stay distinct, records keep schema identity and provenance.

This narrows `protocol.v1.yaml`'s `binary-representation` invariant, which asks for "the
`ono-value/1` binary encoding" on live boundaries and JSON only for the test host. What the
invariant exists to forbid — generic JSON blobs with the type system erased — is still
forbidden: the tagged encoding *is* the lossless codec, with schema ids intact. The binary
framing is a performance property (spec §31.67), not a semantic one, and lands as a `perf`
increment behind the same `Envelope` type when a measurement asks for it. The contract file is
left unedited: the invariant states the end state, this ADR records the staging.

### 3. The K-family error codes live in `ono-kuang-protocol`, pending core integration

`ono_core::ErrorCode` is a closed enum and belongs to the parent's file scope.
`KuangErrorCode` therefore mirrors its shape (`code()`/`name()`/`kind()`, rendered
`Ono-Sendai-K11NNN` per ADR-0022's deviation) inside the protocol crate, and `WireError`
carries codes as strings so core codes (`io.not_found`, `resolve.command_not_found`) and
K-codes travel in one shape. Folding the K-family into `ono_core::ErrorCode` — so `catch` and
`where` match K-codes exactly like E-codes — is the parent's integration step and is required
before Phase I is user-visible.

### 4. `provider.query` added to `protocol.v1.yaml`

`contributions.v1.yaml` registers contributed targets into the provider registry (spec §31.23,
§31.64), but `protocol.v1.yaml`'s `plugin_calls` had no host→plugin call that carries a query
to the providing package — commands and analyses had calls, providers did not. The provider
conformance case (spec §31.74 "output schema conformance" for a contributed target) forced the
gap closed: `provider.query {target, options?, output, invocation}` → `{status, error?}`,
answered as a value stream validated against the target's declared schema. This is the one
edit to `docs/spec/kuang/` in this increment.

### 5. Supervisory semantics the contracts imply but do not spell out

- **Negotiation precedes spawn.** A denied required capability fails the load before the
  artifact starts (`load.capability_denied`); manifest-before-code extends to
  capability-before-code.
- **The hello is identity cross-check, not authority.** The host validates the manifest from
  the package, then checks the instance's hello against it: format, id, version, API range.
  A mismatch is `package.invalid` and the instance is killed.
- **Credit is explicit and initial.** `command.invoke`/`provider.query` carry the opening
  credit (the negotiated queue depth); further credit arrives only as the host's consumer
  takes values. The SDK blocks emission at zero credit; the supervisor treats an emission
  beyond credit as `runtime.protocol_violation`.
- **Output validation is per-value.** A contributed command's declared output type
  (`stream<int>`, `stream<pkg.item/1>`) is checked on every emitted value; a violation closes
  that stream with `runtime.schema_violation` and leaves the instance running (spec §31.34's
  schema-violation class), unlike a framing violation, which quarantines (ADR-0041).
- **Provenance is restamped by the host.** Every `$record` a plugin emits gets
  `provenance.provider = plugin:<package.id>` before decoding; whatever the plugin wrote there
  is overwritten (spec §31.80).
- **`ask` resolves to deny.** The libraries are non-interactive; a prompt nobody can answer is
  a denial that pretends otherwise (capabilities.v1.yaml → `grant.decisions`).
- **Load-time scope coverage is capability-granularity.** At load, a request is covered when a
  grant for the family exists; the scope subset check happens per call, against the concrete
  value the operation uses. Textual subset comparison of two glob languages at load time would
  claim a precision the broker does not need — every call is checked anyway (spec §31.19:
  "evaluated per call").

## Consequences

Easy: the conformance suite of spec §31.74 runs offline in the workspace test run — manifest
validation, denial paths (required, optional, call-level, scope), cancellation, backpressure
both ways, quota exhaustion, protocol-violation quarantine, output schema conformance,
provider queries, leases, audit determinism. The parent's wiring surface is three types:
`Supervisor::load(LoadConfig) -> LoadedPlugin`, `LoadedPlugin::{commands, targets}` for
registry entry, `LoadedPlugin::{invoke, query}` for execution.

Hard, and deliberately deferred to later Phase I increments: wasm-component isolation (T2 —
`native-process` is the only tier the supervisor spawns today), the objects/streams/views/
models host domains, install/verify/signing (spec §31.36), state persistence to disk
(the store is in-memory per instance), migrations, hot reload, and the binary value encoding
of §2 above.

## Alternatives considered

- **One crate** — rejected: the SDK must be usable by a plugin author without pulling the
  supervisor in, and the protocol types must not depend on tokio.
- **Depending on `ono-protocol` for framing** — rejected: the remote link's frame carries
  stream multiplexing in its header; the KUANG/11 boundary multiplexes at the message level.
  The patterns (length-declared frames, pre-allocation checks) are reused conceptually, as the
  task directs.
- **Extending `ono_core::ErrorCode` from here** — rejected: outside this increment's file
  scope, and a cross-crate change the parent should land atomically with `catch` support.
- **Push-based delivery with host-side buffering** — rejected: ADR-0022 §8 already decided
  pull-based flow in both directions, and the conformance suite proves the structural bound.
