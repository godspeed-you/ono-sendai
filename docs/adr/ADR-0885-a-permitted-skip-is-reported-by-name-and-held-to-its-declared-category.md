# ADR-0885: A permitted skip is reported by name and held to its declared category

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38.1, §38.2, §38.3, §38.4
- Relates to: ADR-0513, ADR-0514, ADR-0517, ADR-0552, ADR-0880, ADR-0883
- Decided by: agent (autonomous)

## Context

`canonical_ci.permitted_skips` in `docs/contracts/hardening/expected_test_skips.yaml` lists
skips whose outcome is a property of the host, with the condition that decides each (ADR-0517).
`cargo xtask skip-check`, which CI runs over every gate part's log, neither required nor forbade
them — and did not mention them either. It also matched an observed marker to the registry by the
test's name alone and ignored the marker's category.

The v0.6.2 run added three such rows (the mount comparison, Profile L's loaded machine, the `lo`
trace). The independent review pointed out the consequence: any of them could skip in CI on every
run, for any reason, and nothing would ever say so.

## Decision

1. **A skip is permitted for its reason.** An observed marker whose test is in `expected_skips`
   or `permitted_skips` but whose §38.4 category is not one the `declared:` list gives that test
   fails `skip-check`, naming both categories.
2. **Every permitted skip a run took is reported**, one line each, with the test id, the category,
   the marker's detail and the registry's condition —
   `skip-check: permitted skip taken — <id>: <category>: <detail> (it runs where: <condition>)` —
   on standard output, and in GitHub Actions also on the job's summary page
   (`GITHUB_STEP_SUMMARY`). The `ok` line counts them.
3. **A permitted skip taken in canonical CI does not fail the run.** Each row's condition is a
   host property the repository does not control — the runner's hard descriptor limit, whether
   the host held still, how busy it was — and ADR-0517's reasoning stands: requiring the skip is
   red on a runner that can supply the capability, forbidding it red on one that cannot. What
   changes is that the skip is no longer silent: a row that skips on every run is visible on every
   run's summary, which is the signal for a maintainer to arrange the capability or move the row.

## Consequences

`xtask/tests/scan.rs::should_fail_a_skip_whose_category_is_not_the_one_declared_for_its_test` and
`::should_report_every_permitted_skip_a_run_took_with_the_condition_that_allowed_it` encode it;
the existing skip-check tests are unchanged and pass.

A permitted skip can still happen on every CI run without failing it. That is a decision, and the
report is how it is kept honest; failing it would make the runner's `ulimit` a red build.

## Alternatives considered

**Fail any permitted skip in canonical CI.** It turns the list into `expected_skips` with the
opposite sign and makes a host limit a product failure (ADR-0517).

**Match markers by their full `<path>::<test>` id.** The marker carries only the test name
(ADR-0513), and changing that is a harness change outside this one; the category check closes the
larger gap.
