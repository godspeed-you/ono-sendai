# ADR-0929: v0.6.3 is its milestone, each issue is its requirement, and the referee is repaired first

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.6.1 §3.3, §23, §24, §31; AGENTS.md §5.2, §9, §14; ADR-0425, ADR-0862
- Decided by: agent (autonomous)

## Context

The user asked for v0.6.3 — "Language and Contract Stabilization" — to be implemented, with the
GitHub milestone `v0.6.3` named as the authoritative work inventory. As for v0.6.2 (ADR-0862), no
narrative specification for v0.6.3 exists: `docs/specs/` holds none, and only the user adds one
(AGENTS.md §5.2). The milestone's description is:

> Language and Contract Stabilization. Scope: command contracts, errors and diagnostics; the shell
> language, pipelines and completion. Purpose: stabilize Ono's command semantics and language
> behaviour before changing higher-level system abstractions.

It held 20 open issues at the start of the run: #137, #149, #159, #163, #168, #170, #171, #173,
#174, #175, #176, #177, #178, #180, #183, #191, #192, #193, #214 and #223. Several were written
against older trees (2026-09-02 … 2026-09-15), so an open issue is not proof that its defect still
exists. Every one was reproduced against `implementation` at `c6554961` before any change; the
evidence is in each issue's closing comment and in `docs/releases/v0.6.3.md`. None turned out to be
already satisfied: in particular #191 — which #223 described as working — still resolves
`get process | mine | take 1`'s `mine` as a program, because the test #223 pointed to covers a
function at the head of a pipeline only.

The baseline gate was not green on the machine the run started on.
`crates/ono-cli/tests/services_logs.rs` reads the host's real journal (4 GB there), and
`should_filter_journal_events_by_priority_when_where_composes_over_the_typed_stream` ran into its
10 s watchdog every time, alone and on a quiet machine. That is issue #280, filed under `v0.6.4`.
AGENTS.md §9 rule 4 and §14 rank repairing the referee above every feature, and a release whose
`release-check` cannot pass where it is qualified cannot be called release-ready.

## Decision

1. **The milestone `v0.6.3` is the release inventory**, exactly as ADR-0862 decided for v0.6.2:
   each issue body is a normative requirement, its exit test is binding and a proposed mechanism is
   a suggestion an ADR may replace; v0.6.1's cross-cutting rules (§24 scope, §31 gates, §32
   compatibility) apply unchanged.
2. **Each issue is revalidated before it is worked**: reproduced, found already satisfied, or found
   superseded. Only the first leads to code; the other two close with the evidence and a
   regression test.
3. **#280 joins the release as a referee repair, and only that.** Its test-only fix — a
   deterministic `journalctl` at the external boundary — is what lets the gate decide anything on
   this machine. #281 (predicate pushdown), its sibling, is a product change and stays in `v0.6.4`.
   #280 is moved into the `v0.6.3` milestone so the inventory says what the release contains.
4. **The traceability record is `docs/releases/v0.6.3.md`**: every issue, the commits and ADRs that
   close it, the tests and acceptance cases that prove it, and the verification result. The
   workspace declares 0.6.3; promotion to `main` and the tag remain the user's (AGENTS.md §12.1).

## Consequences

- A problem found on the way that is not in the milestone goes to `docs/STATE.md` → *Found, not yet
  filed*, not into the release.
- Should the user later add a v0.6.3 narrative specification, it outranks this record.

## Alternatives considered

**Run the gate with `services_logs.rs` excluded, or on a machine with a small journal.** Either
would make the referee green by not asking it the question; AGENTS.md §14 forbids weakening the
harness, and a gate that depends on which machine runs it is the defect #280 names.

**Leave #280 in `v0.6.4` and fix it anyway.** The milestone would then misstate what v0.6.3 ships.
