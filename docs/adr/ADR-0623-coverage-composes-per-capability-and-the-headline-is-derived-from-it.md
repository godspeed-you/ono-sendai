# ADR-0623: Coverage composes per capability and the headline is derived from it

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §3.5, §7.4, §7.5, §8.1, §8.2, §8.3, §8.4, §8.5, §8.6, §11.7, §34
- Decided by: agent (autonomous)

## Context

§8.1 forbids one shape of answer outright — "coverage MUST NOT be represented by one global
boolean" — and §8.5 forbids the next one up: a reconstruction over several sources "MUST compute
the effective coverage per field/relation rather than simply selecting the strongest global label".
It then permits a renderer to summarise to `complete | partial | uncertain`, while requiring
`inspect` to expose source-level detail.

§8.6 attaches a user-visible consequence: `[PAST]` means supported coverage, `[PAST?]` means the
reconstruction is materially partial or uncertain, and "a prompt MUST NOT display `[PAST]` merely
because at least one event exists near that time".

§7.4 attaches a second: Ono may claim something did not exist only "if an authoritative or
sufficiently complete source had coverage capable of proving that absence".

Three requirements, one computation. The question is which is the primitive.

## Decision

**The per-capability composition is the primitive; everything else reads it.**

`CoverageSummary::compose(intervals, window)` groups the intervals by capability and, for each,
computes over the window:

- the union of the intervals whose completeness is `complete`;
- the union of those that are `complete` or `partial` — the ones that saw *something*;
- the holes in each.

The capability composes to `complete` when the complete union leaves no hole, `partial` when
anything covering reached any of the window, and otherwise to the weakest thing that was claimed
about it — `point_sample`, then `permission_denied`, then `unavailable`, then `unknown`. A point
sample is deliberately not a covering interval: §8.4 says a snapshot "cannot by itself explain
intermediate change", so it never fills a window and never suppresses a gap.

Gaps are the holes in the *covering* union, not in the complete one: a partial source did see
something there, and reporting that stretch as a hole would be as dishonest as hiding a real one
(§11.7). Each gap takes its reason from an overlapping non-covering interval where there is one —
`permission_denied` beats `provider_unavailable` beats `not_recorded` — so history that exists and
cannot be read never appears as history that does not exist (§34 `temporal.permission_denied`).

Everything else is derived:

- `headline()` is `complete` only when every composed capability is complete; `uncertain` when any
  is unknown, unavailable or denied, and when nothing composed at all; `partial` otherwise.
- `TemporalContext::prompt_marker()` is `[PAST]` for `complete` and `[PAST?]` for the other two.
  §8.6 then holds by construction: a single point sample composes to `point_sample`, which is not
  complete, so one event near the time cannot produce `[PAST]`.
- `can_prove_absence(capability)` is true only where that capability composed to `complete` **and**
  no gap belongs to it. §7.4's condition, and nothing weaker.
- `sources()` lists every contributing source, which is §8.5's "`inspect` MUST expose source-level
  detail".

## Spec deviation

- Section: v0.5 §35.3
- Text: "`ono.temporal-coverage/1` — scope, capability, from, until, completeness,
  sampling_interval, source, permission_state"
- Instead: `ono.temporal-coverage/1` carries exactly those fields and describes **one source's
  claim over one interval**. The composed summary that §3.9, §9.4, §16.4 and §35.2 refer to as
  `TemporalCoverageSummary` is a nested sub-record (`headline`, `capabilities`, `gaps`, `sources`,
  `from`, `until`) rather than a schema of its own, produced by `value::coverage_summary`.
- Why: §35.3's field list is per-interval — a single `from`, `until`, `completeness` and `source`
  cannot express a composition over several sources and several capabilities. §35 names no schema
  for the summary, and inventing `ono.temporal-coverage-summary/1` would add a public schema the
  specification does not ask for. The house convention for a structure that is part of another
  record and not addressable on its own is a nested `record` field, which is what
  `spatial-place.v1.yaml` does with `lifetime`, `identity` and `provenance`.

## Consequences

- A window covered completely for `service.state` and not at all for `process.existence` reports
  both facts, which is what §8.1 asks for and what a single label cannot say.
- `[PAST]` is expensive to earn, which is the intent of §8.6.
- Composition is O(intervals log intervals) per capability, over the intervals in one window. The
  session ledger's synthetic eviction interval participates like any other, so a long session's
  dropped morning composes into a gap rather than into silence (ADR-0621).
- Encoded in `crates/ono-temporal-core/tests/coverage.rs` and `tests/context.rs`.

## Alternatives considered

- **A single headline computed from the strongest source.** Explicitly forbidden by §8.5, and it
  is the thing that makes a half-covered map look whole.
- **Treating a point sample as complete coverage of its own instant.** True in a narrow sense and
  actively misleading in a window: §8.4 says so, and §7.4's absence claim would follow from it.
- **Deriving the prompt marker from an event count near the instant.** Named and forbidden in
  §8.6.
- **A separate `ono.temporal-coverage-summary/1` schema.** A public schema §35 does not list, for a
  structure that is never addressed on its own.
