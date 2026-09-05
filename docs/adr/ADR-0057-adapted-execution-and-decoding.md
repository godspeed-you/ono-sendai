# ADR-0057: Adapted execution, decoding and adapter provenance

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.4, §1.7, §1.8, §1.10, §1.11, §1.18, §1.20, §1.47, §1.57, §1.60; v0.2 §12.3, §18, §25.2; ADR-0013, ADR-0016, ADR-0028, ADR-0052, ADR-0055, ADR-0056
- Decided by: agent (autonomous)

## Context

With the demand planned (ADR-0052), the contract written (ADR-0055) and the negotiation
decided (ADR-0056), an adapted stage has to actually run: something must spawn the plan,
read what the tool wrote, turn it into records of the canonical schema, say where every
value came from, and keep every Unix guarantee the stage had before it was adapted.

## Decision

1. **The executor is the native runner, and the process subsystem spawns.** In
   `ono-cli`'s `run_from`, an external segment whose *last* stage negotiates to a plan is run
   as the same `ono_process::Pipeline` it would have been, with that last command replaced
   by the plan — the pinned executable, the plan's argv, the plan's environment on top of the
   session's, stdin per the plan (`null`) or the carried bytes — and its stdout captured
   (spec v0.3 §1.7: "adapters describe semantics; they SHOULD NOT reinvent process
   execution"). Job control, signals, the terminal and stderr are exactly as for any child.

2. **When a stage is adapted.** The demand is computed as ADR-0052 says, from the stage's
   position: `Structured` when the next segment is native and its first contract does not
   accept bytes; `Interactive` when the stage is last, stdout is a terminal, the stage has no
   stdout redirection and the pipeline is not backgrounded; otherwise bytes, and the stage
   runs as it always has. `native::claims` therefore also claims an all-external pipeline
   whose last stage is adapted at the terminal — that is what makes `lsblk` typed at the
   prompt render as a table, and `lsblk &`, `lsblk > f` and `lsblk | grep` stay classic.

3. **Exit status keeps Unix semantics (§1.20).** A child that exits non-zero fails the stage
   with `external.exit_nonzero` (or `external.signal`) before any decoding; a successful
   decode never turns a failed child into success. stderr is never captured.

4. **Decoding is whole-output for documents and streaming for line protocols.** A `json`
   decoder reads the captured bytes as one document, takes the record list at `records`
   (or the document itself), and builds one record per entry. `lines` and JSON-lines decoders
   (the ones `ps`, `find`, `journalctl` will use) decode per record as bytes arrive. Either
   way a decoder is total: malformed, truncated, non-UTF-8 or hostile input yields
   `adapter.decode_failed` — never a panic — with the adapter, the executable, the user's
   invocation and the first 4 KiB of the raw bytes in the error's metadata under the keys of
   ADR-0053.

5. **Coercion is driven by the schema, exactness by the contract.** Each `fields` entry
   takes the decoded value at `from`, applies `map`, `split`, `contains` and `unit`, and
   coerces into the schema field's declared type: booleans also from `0`/`1`/`true`/`false`/
   `yes`/`no`; integers also from numeric strings; byte sizes from numbers in the declared
   unit or from `12 MiB` strings; timestamps from RFC 3339 or epoch numbers; paths, IPs,
   ports and enums from strings; lists element-wise. A value that cannot be coerced is
   `adapter.schema_violation` naming the field — "no silent best-effort field shifting"
   (§1.10). A decoded field the map omits is `null`; a decoded field the map does not name
   lands in the record's extension map under `<pack>.<adapter>` (§1.11); the schema's own
   `validate` runs on every record last.

6. **Provenance grows an adapter block (extends ADR-0016).** `ono_value::Provenance` gains
   `AdapterTrace { adapter, adapter_version, executable, executable_version,
   user_invocation, actual_invocation, decoder, stability, exactness, limits }`, set by the
   decoder on every adapted record; the provider is `adapter:<full id>`, the source the
   actual invocation, the link the session's, `observed` the moment the child finished.
   `inspect` renders all of it (v0.3 §1.8's ten questions), with `exactness` listing only the
   fields that are not exact. Per-field provenance stays out of the value, as ADR-0016 chose;
   per-field *exactness* is a small map on the record's provenance, which is what §1.8
   asks for.

7. **Fallback after the fact.** Under an `Interactive` demand a decode failure with a `raw`
   fallback does not re-run the tool — a second run is a second side effect. The captured
   bytes are written to stdout as they are, the §1.57 diagnostic goes to stderr, and the
   stage's status is the child's. Under a `Structured` demand the error stands (§1.18).

8. **Conformance is one function.** `ono_adapter::conformance::check_pack` decodes every
   fixture of every adapter through the same decoder the shell uses and compares the result
   with the fixture's sidecar field by field in canonical text form (or the expected
   `adapter.*` selector); `spec-check` and the crate's tests both call it (ADAPT-010).

## Consequences

- Adapted values are indistinguishable from native ones to every transform, renderer,
  `@` reuse and `to json` (§1.12), and distinguishable to `inspect` (§1.8).
- The first tool to run end-to-end is util-linux's; every later Tier A/B tool is a contract,
  fixtures and an acceptance case.
- Tests: `crates/ono-adapter/tests/conformance.rs` (every fixture), `decode.rs` (coercion
  and the error payloads), `crates/ono-cli/tests/adapters.rs` (end-to-end with the real
  tools and with shadowing scripts for the failure paths), acceptance case `074`.

## Alternatives considered

- Decoding inside `ono-process` — rejected: the process subsystem moves bytes and manages
  jobs; values are the pipeline's business.
- Re-running the tool raw after a decode failure — rejected under point 7.
- Per-field provenance records — rejected again, as in ADR-0016; exactness is the per-field
  fact §1.8 actually needs.
