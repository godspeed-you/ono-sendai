# ADR-0934: A budget of zero is refused before the stage reads

- Status: accepted
- Date: 2026-10-07
- Spec refs: v0.4.1 §21.4, §22.1, §22.2, §22.3, §53.1, §54.1, §67.5; ADR-0453, ADR-0455, ADR-0537,
  ADR-0938
- Decided by: agent (autonomous)

## Context

v0.4.1 §21.4 requires three error families — `resource.item_limit`, `resource.byte_limit`,
`resource.materialization_limit` — and `docs/contracts/errors.yaml` declares the third as
`Ono-Sendai-E1103`: "the stage requires finite input, or its budget admits nothing at all
(v0.4.1 §22.2, §22.3)". Nothing constructed it; `docs/contracts/hardening/refusals.yaml` recorded it
`raised: false` (issue #183).

§22.2: "A value of zero means 'no values permitted', not unlimited." `limits.materialize_items` and
`limits.materialize_bytes` both admit 0 (`limits.yaml`, `min: 0`). Under either, `sort` read its
first value, charged it, and refused with E1101 "reached its 0 values budget after 1 values" — a
refusal that needed input to happen, and that never came over an upstream that is finite by
declaration but slow, although the answer was fixed by the configuration before a value existed.

## Decision

**A materializing stage whose budget admits nothing refuses with `resource.materialization_limit`
before it reads a value.**

1. `ono_pipeline::Transform` gains `materializes()`, false by default and true for the five
   transforms that retain their input against the budget — `sort`, `group`, `join`, `diff`,
   `measure` (Appendix E's global classes, the same five `should_bound_every_transform_that_buffers
   _its_whole_input` names). `count`, `reduce`, `last` need finite input but hold constant state,
   so a zero budget does not concern them, nor any streaming stage (`take`, `where`, `each`).
2. `ValueStream::transform` checks, after the finiteness check and before `apply`: a materializing
   transform under limits with `max_items == 0` or `max_bytes == 0` is refused synchronously, so
   the stage never starts. `materialize_with`, the helper, applies the same rule to its budget.
3. The refusal is constructed in `ono-pipeline` (`budget::admits_nothing`): message
   "`sort` must hold its whole input, and its budget admits nothing: `limits.materialize_items` is
   0 values", help naming §22.2 and the setting, metadata `stage`, `ceiling`, `limit` (0) and
   `setting`. When both are zero the item setting is named. There is no `consumed`: nothing was.
4. `refusals.yaml` records it `raised: true` with `explains: [stage, ceiling, limit, setting]` and
   `says: "admits nothing"`, so the reporter shows the fields under the message (ADR-0938).
5. A budget that admits something is still *reached*: `limits.materialize_items = 1` refuses with
   E1101 on the second value, as before.

## Consequences

- E1103 has a real path; the census no longer carries an unraised code in the resource family.
- The finite-input refusal stays `stream.unbounded_operation` (E0801), and it is checked first: an
  unbounded upstream under a zero budget is refused for being unbounded. Changing E0801 to E1103
  would break the code scripts and `crates/ono-pipeline/tests/boundedness.rs` have matched on since
  v0.2 §11.1 — the deviation ADR-0453 already recorded, restated below.
- Another transform that starts retaining its input must return `materializes() == true`, or a zero
  budget reaches it as E1101 on its first value — still a refusal, never unlimited.

Encoded by `crates/ono-pipeline/tests/budget.rs`:
`should_refuse_every_materializing_transform_before_it_reads_when_its_budget_admits_nothing`
(over a declared-finite source that never sends: a refusal that waited for a value would hang),
`should_refuse_through_the_helper_before_it_reads_when_the_budget_admits_nothing`; through the
binary by `crates/ono-cli/tests/resource_limits.rs`:
`should_refuse_a_materializing_stage_whose_item_budget_admits_nothing`,
`should_refuse_a_materializing_stage_whose_byte_budget_admits_nothing`,
`should_leave_a_stage_that_holds_nothing_untouched_when_the_budget_admits_nothing`,
`should_still_refuse_with_the_item_ceiling_when_the_budget_admits_one_value`; and acceptance case
`190-materialization-limits`.

## Spec deviation

- Section: v0.4.1 §67.5
- Text: "`local://~ > unbounded-source | sort timestamp` / `error resource.materialization_limit:`
  / `sort requires finite input, but the upstream stream is unbounded`"
- Instead: that refusal stays `stream.unbounded_operation` (`Ono-Sendai-E0801`), as ADR-0453
  decided; `resource.materialization_limit` is raised for the other half of errors.yaml's
  definition — a budget that admits nothing at all.
- Why: §53.1 — "Exact names may be reconciled with existing naming conventions, but the failure
  classes MUST remain distinct." The unbounded upstream has had a stable code since v0.2 §11.1;
  re-pointing it would break every script matching it, and the zero budget is the distinct class
  E1103 can name without taking a code from anything.

## Alternatives considered

- **Check the budget inside each stage before its first read.** Equally early, but five copies of
  the same check, and the refusal would travel as a stream failure after the stage had started.
- **Treat zero as "the stage may not run" by refusing the setting.** §22.2 makes zero a valid
  value with a meaning; refusing to store it would make the meaning unreachable.
- **Raise E1103 for the unbounded upstream, as §67.5's example does.** See the deviation.
