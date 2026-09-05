# ADR-0027: The v0.3 adapter layer is the next tranche, not a change to this one

- Status: accepted
- Date: 2026-08-26
- Spec refs: v0.3 §0, §0.4, §0.5, §1.1–§1.75, §2; v0.2 §12.4, §31.58, §50
- Decided by: agent (autonomous)

## Context

`docs/specs/ono_sendai_shell_spec_v0.3_external_command_adapters.md` arrived mid-run. It specifies an
**External Command Adaptation Layer**: a fourth command layer between native Ono commands and
arbitrary executables, letting selected Unix tools (`ss`, `ip`, `ps`, `lsblk`, `journalctl`,
`systemctl`, `find`, `git`, `curl`, `lsof`, …) emit canonical Ono records without being
reimplemented or heuristically scraped. Its product principle is *"adapt before replacing;
normalize concepts, not commands; fall back honestly"* (§0.3, §1.75).

Two questions had to be settled before any code moved: what it does to the current run's stopping
rule, and what it does to the decisions already made.

## Decision

### 1. It is additive, and it is the next tranche

The document says so itself (§0):

> "For the original autonomous-implementation experiment, the v0.2 specification remains the
> immutable initial input. This v0.3 document is a **new product input for a later revision** and
> must not be back-merged into the frozen v0.2 file."

and again in §0.5, that v0.3 "specifies a product increment after the frozen v0.2 baseline".

Therefore:

- **`docs/ACCEPTANCE.md` and `scripts/release-check.sh` keep measuring v0.2.** The stopping rule
  of AGENTS.md §15 is unchanged. v0.3 does not move the finish line of the current run, and the
  current run is not permitted to stop early because a larger document exists.
- **v0.3 is planned, not deferred.** Its work is decomposed below and carried in `docs/STATE.md`
  as a named tranche, so it is picked up in order rather than rediscovered.
- Where v0.3 constrains something being built *now*, the constraint is honoured now. Building a
  planner that v0.3 would have to tear out would be the expensive kind of shortcut, and §0.4
  names the integration points precisely enough to build towards.

### 2. Nothing already built is wrong; five decisions grow

| Decision | What v0.3 does to it |
|---|---|
| ADR-0011 name resolution | Inserts adapter negotiation after step 5 (§0.4). `ono:` is already native-only and `exec:` external-only, so §1.18's `ono:ss` spelling is unavailable: forced adaptation gets its own keyword. A superseding ADR settles the spellings of `raw` (§1.17) and forced adaptation (§1.18). |
| ADR-0013 execution model | The one that changes most. `External` is typed `Bytes -> Bytes`; §1.1 and §1.5 need `Bytes -> Value`. The contiguous-external-run fusion into one OS pipeline must still happen for `ss \| grep` (invariant 10) but must not when the head is adapted and demand is `Structured` — so `OutputDemand` is computed **backwards from the consumer before stages are fused**. |
| ADR-0006 error model | Extends by exactly one family. §1.65's eleven `ADAPTER_*` names become dotted `adapter.*` codes in a new `E09xx` block, each mapped to one of the twelve existing kinds. **No new kind**: ADR-0006 says adding one is a breaking change, and `external`, `provider`, `permission` and `resolution` cover all eleven. |
| ADR-0016 provenance | Record-level provenance is insufficient for §1.8, which additionally requires executable path and version, the user's invocation, the rewritten invocation, adapter package and version, decoder id, environment delta — and *per-field* exactness (`exact / normalized / inferred / omitted`), which ADR-0016 explicitly declined. Grows an additive `adapter: Option<AdapterProvenance>`. |
| ADR-0022 KUANG contributions | Its `external_adapter:` block encodes v0.2 §31.58's *command-backing* adapter, whose `command:` field the v0.3 model has no value for — there the tool's own token is the entry point. The block widens; `roles: [adapter]` in the manifest is reusable as is. |

ADR-0015's threat model gains a row: **adapter substitution**, a shadowing binary receiving
another tool's decoder. §1.62 forbids exactly this ("a binary named `ss` that is not iproute2
**MUST NOT** receive the iproute2 decoder solely because of its basename"), so matching pins
executable *identity*, not the command token.

### 3. Where v0.2 and v0.3 read differently

- v0.2 §12.4: adapters "MUST not rewrite user commands opaquely". v0.3 §1.14: an adapter "MAY
  rewrite an invocation only inside its declared semantic surface" (`ip address` → `ip -j
  address`). These reconcile: *opaque* is the operative word, and §1.8 and §1.23 require the
  rewritten invocation in provenance and in `explain`. Rewriting is permitted **only** while it
  is visible.
- v0.2 §50 forbids parsing unstable human-readable output "unless clearly documented as an
  explicit adapter fallback". v0.3 §1.9 Tier C makes that a supported first-party strategy under
  version constraints and fixtures. The v0.2 rule therefore binds **providers**, not adapters —
  `ono-provider-api`'s doc comment says so and stays true.
- v0.2 §12.4 shows a user-run `adapter register`. v0.3 §1.24/§1.26 populate the registry from
  KUANG/11 packages with declared capabilities, which a user-run registration would bypass
  (§1.22). `adapter register` becomes a KUANG-mediated local install, not a runtime escape.

### 4. The tranche

Foundation before any tool, machine protocols before brittle parsers (§1.69), each increment
named by its §1.67 work package:

1. `ADAPT-001` `OutputDemand` in the planner, reported by `explain` before any adapter exists.
2. `ADAPT-003` the raw bypass of §1.17 — byte-identical output, no renderer, status preserved.
3. `ADAPT-002` the registry and deterministic conflict resolution (§1.24, §1.25).
4. `ADAPT-007` adapter provenance, the whole §1.8 field list.
5. `ADAPT-004`/`005` plan execution and the streaming decoder; §1.20's rule that decoder success
   never turns a failing child into a success.
6. `ADAPT-006` the version probe and its cache (§1.46).
7. The `adapter.*` error family (§1.65).
8. `ADAPT-009` the manifest schema and `docs/contracts/adapters/` (§1.44, §1.66).
9. `ADAPT-010` the fixture conformance harness (§1.47, §1.68).
10. `ADAPT-008` `process.exec` capability enforcement, `declared-invocations-only` (§1.22).
11. Executable identity pinning (§1.62).
12. Tier A tools: `lsblk`/`findmnt`/`lsns`, `ip`, `journalctl`, `systemctl` (§1.33–§1.37).
13. Tier B: `ps`, `find`/`stat`/`df`/`du`, `git`, `lsof` (§1.34, §1.38–§1.40, §1.42).
14. Tier C: `ss`, `curl` (§1.32, §1.41).
15. Integration surfaces: `type`/`inspect`/`get command` (§1.23, §1.61), `explain`'s adaptation
    plan and diagnostic states (§1.23, §1.57), history's `adapter` field (§1.58), completion
    after an adapted stage (§1.59), remote negotiation (§1.54), script determinism (§1.53).

New crate `ono-adapter` — registry, negotiation, plan, `OutputDemand`, decoder API — deliberately
outside `ono-provider-api`, because §1.3 separates the two: a provider answers a `Query`, an
adapter negotiates an `ExternalInvocation` against an `OutputDemand`.

## Consequences

- The current run finishes v0.2 and then starts this tranche. `docs/STATE.md` carries it so no
  part of the analysis depends on a session surviving.
- `docs/ACCEPTANCE.md` gains a v0.3 section when the tranche starts, not before: a box that
  nothing yet proves would make the release checklist lie.
- The pipeline planner being built now computes demand backwards from the consumer, because
  retrofitting that into a forward planner is the one change §1.5 says must not be
  "an after-the-fact renderer trick".

## Alternatives considered

- **Treat v0.3 as the new baseline and restart planning against it.** Rejected: §0 forbids
  back-merging and calls v0.2 the immutable initial input; v0.3 is silent on the language, the
  object model, KUANG/11 and everything else, so reading it as a replacement would discard the
  product.
- **Fold v0.3 into the current release gate.** Rejected: it would move a finish line the user
  set, on the strength of a document that says it is for a later revision. If the user wants it
  in this run, that is one sentence from them and this ADR is superseded.
- **Record it and move on without a decomposition.** Rejected: the analysis cost real work and
  the decomposition is the part that survives a compaction.
