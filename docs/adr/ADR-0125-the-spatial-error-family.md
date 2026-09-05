# ADR-0125: The spatial error family is E10xx in the one error registry

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §40 (error model), §41 (machine-readable spatial registry); v0.2 §16, §43
  (error taxonomy); ADR-0006, ADR-0053 (the adapter family E09xx)
- Decided by: agent (autonomous); confirmed by the user 2026-08-28

## Context

v0.4 §40 requires fourteen structured errors and names them by their dotted names —
`spatial.not_found`, `spatial.ambiguous_selector`, `spatial.not_enterable`,
`spatial.no_relation`, `spatial.no_parent`, `spatial.history_empty`,
`spatial.destination_gone`, `spatial.permission_denied`, `spatial.unsupported`,
`spatial.stale`, `spatial.remote_unavailable`, `spatial.scope_violation`,
`spatial.map_too_large`, `spatial.identity_conflict` — but gives them no numeric codes, while
this shell renders `Ono-Sendai-EXXXX` and scripts match on it.

`docs/contracts/errors.yaml` is the single taxonomy: families run E0001–E0002 (parse), E01xx
resolution, E02xx type, E03xx io, E04xx provider, E05xx external, E06xx remote, E07xx safety,
E08xx stream and E09xx adapter (v0.3, ADR-0053), with KUANG/11's K11xxx folded into the same
`ErrorCode` enum by ADR-0051. The highest allocated code is `Ono-Sendai-E0911`.

v0.4 §41 additionally recommends a separate `spatial-errors.yaml` beside the spatial registry.

## Decision

1. **A new family `spatial`, block E10xx**, allocated in the order §40 lists the names:
   `spatial.not_found` = `Ono-Sendai-E1001` … `spatial.identity_conflict` = `Ono-Sendai-E1014`.
   The family is added to the `families` list of `docs/contracts/errors.yaml` beside `resolution`,
   `provider` and `adapter`.
2. **The codes live in `docs/contracts/errors.yaml`, not in a `spatial-errors.yaml`.** §41's file
   list is explicitly "recommended"; a second error registry would let two files disagree about
   the same taxonomy, which is the drift `spec-check` exists to prevent, and every consumer —
   `ono_core::ErrorCode`, the generated reference, the `try`/`catch` surface — reads one file.
3. **Both identities are user-visible**, as everywhere else in this shell: the rendered
   diagnostic carries `Ono-Sendai-E10NN`, and the structured value carries `name:
   "spatial.…"`. The RED suites match on the dotted name, which is the identity §40 fixes.
4. **§40's "actionable next steps" go into the `help` field** of each entry, in the shape §40's
   own examples show (what was ambiguous, how many candidates, which spelling resolves it).
5. Exit status follows the existing mapping of v0.2 §16: a spatial refusal is a failed command,
   not a crash — non-zero, and `spatial.permission_denied` keeps the exit status
   `io.permission_denied` already has, so a script that classifies by status keeps working.

## Consequences

- The delivering increment adds fourteen entries to `docs/contracts/errors.yaml` and fourteen
  variants to `ono_core::ErrorCode`; `crates/ono-core/tests/error_taxonomy.rs` grows with them,
  and `spec-check` then holds registry and enum together.
- `crates/ono-cli/tests/spatial_contracts_missing.rs::should_register_the_whole_spatial_error_family_in_the_error_taxonomy`
  is the test that goes green when this lands; it already expects E10xx.
- E11xx stays free for the next tranche, so the block boundary keeps meaning something.
- A spatial condition that turns out to be an existing one must reuse the existing code rather
  than duplicate it under a spatial name — `spatial.permission_denied` exists because §40 names
  it, but a plain file read denied inside a spatial command is still `io.permission_denied`.

## Alternatives considered

- **Reuse the existing families** (`resolve.target_not_found` for `spatial.not_found`, …).
  Rejected: §40 names fourteen distinct conditions, and a script that wants to distinguish "this
  place is gone since you visited it" from "no such command" cannot do it through one code.
- **A separate `spatial-errors.yaml`, as §41 recommends.** Rejected for the drift reason above;
  recorded here rather than followed, since §41 says "recommended files", not MUST.
- **Numbering by section rather than by list order.** Rejected: §40's list is stable and
  ordered; section numbers are not a numbering scheme.
