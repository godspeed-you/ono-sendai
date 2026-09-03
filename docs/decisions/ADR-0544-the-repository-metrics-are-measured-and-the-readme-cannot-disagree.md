# ADR-0544: The repository metrics are measured, and the README cannot disagree

- Status: accepted
- Date: 2026-09-03
- Spec refs: v0.4.1 §50.1, §50.2, §50.3, §50.4, §38.1, §38.2, §65.10
- Decided by: agent (autonomous)

## Context

§50.1 names five volatile counts — crates, unit and integration tests, acceptance cases, ADRs,
generated command contracts — and forbids duplicating them "across README/docs without automated
verification". §50.2 asks `xtask` to compute them and gives the shape: `crates=30`, `tests=…`.
§50.3 lets the README keep the numbers "because they are useful evidence" and requires the gate to
fail when they disagree. §50.4 is the one that changes what the numbers are called.

The README carried none of them. So this is not a reconciliation: it is deciding what to measure,
what to call it, and what a reader is entitled to conclude.

## Decision

**`cargo xtask metrics` measures nine figures**, and the README carries them between two markers
that `--write` regenerates and `spec-check` compares.

**`tests` counts test functions declared, and says so.** This is §50.4's demand taken at its word:
*"A README statement such as 'N tests pass' MUST not count skipped cases as proof of execution."*
A static reading of the tree cannot know what a run executed, and a figure named `tests` with a
sentence claiming they pass is exactly the substitution §65.10 calls skip-as-pass. So the metric
is declaration, the README says "declared" in the prose above the block, and two figures stand
beside it rather than being folded into it:

- `tests_that_can_skip` — every test the tree shows can announce a skip, which is the same reading
  `docs/spec/hardening/expected_test_skips.yaml` is held against;
- `expected_ci_skips` — how many the canonical CI environment expects to take. §38.2 prefers this
  number small, and putting it on the front page is the cheapest way to keep it that way.

A reader who wants "how many passed" is pointed at `cargo test`'s own summary, and the block never
offers a substitute for it.

**`crates` and `workspace_members` are both reported.** §50.2's example says `crates=30` and the
tree has thirty library crates under `crates/` and thirty-two workspace members, because `xtask`
and `fuzz` are members too (ADR-0313). One figure would have had to mean one of those and imply
the other; two figures mean what they say.

**String literals are removed before test attributes are counted.** `xtask/tests/scan.rs` holds
Rust fixtures inside raw strings, at the left margin, because the scanners it tests read Rust. A
naive count adds about fifteen tests that do not exist. A metric nobody can reproduce is a metric
nobody believes, so the counter blanks `"…"` and `r#"…"#` spans first — and a test asserts it,
because that is the one place this generator could be quietly wrong.

**The block is generated; the paragraph above it is not.** §50.3: "The original narrative text
around it remains human-owned." `write_readme` replaces exactly the span between the markers, and
a test asserts the prose either side survives and that a second run changes nothing.

## Consequences

- Adding a test, a case or an ADR now makes the README stale until `cargo xtask metrics --write`
  is run, and the gate says so with both readings side by side and the command that fixes it.
  That is friction, and it is the friction §50.1 is asking for: the alternative is a number that
  drifts silently for a year.
- The failure message prints what the README says and what the tree says, indented, so the
  correction is a reading rather than an investigation.
- Two figures are measured that §50.1 does not name — `workspace_members` and `commands` — because
  both were about to be typed into prose somewhere and §50.1's list is a floor.
- The counter reads the same tree the scanners walk, through `scan::rust_sources`, which was made
  public for this. A metric measured over a different set of files from the one the gate walks is
  a metric about a different repository.
- Encoded by `xtask/tests/metrics.rs::should_compute_every_metric_the_readme_states`,
  `::should_count_executed_tests_apart_from_skipped_ones`,
  `::should_fail_when_the_readme_disagrees_with_the_computed_metrics` — the issue's exit test —
  `::should_report_a_readme_that_carries_no_generated_block_at_all`,
  `::should_rewrite_the_block_and_leave_the_prose_around_it_alone` and
  `::should_not_count_a_test_written_inside_a_string_literal`.

## Alternatives considered

- **Count what `cargo test` reports.** Rejected for the gate: it needs a run, and `spec-check` is
  the static half of the gate by design. §38.3's `skip-check` already reads a run's output, and
  that is where an executed-versus-skipped figure belongs if one is ever wanted.
- **Keep the numbers out of the README entirely.** Rejected: §50.3 says they are useful evidence,
  and a project that will not state how much of it exists is asking a reader to take its word.
- **Put the block in `docs/` and link it.** Rejected: the number a reader distrusts is the one on
  the front page, and a metric nobody sees is not evidence.
- **Fold the skip count into the test count.** Rejected by §50.4 in as many words.
- **Exclude the scanner's own test files from the count.** Rejected: it would drop about eighty
  real tests to avoid about fifteen false ones, and the honest fix — not counting inside string
  literals — is twenty-five lines.
