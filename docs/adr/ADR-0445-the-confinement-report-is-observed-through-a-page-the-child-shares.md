# ADR-0445: The confinement report is observed, through a page the child shares

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.3, §2.6, §16.4, §16.5, §54.1, §54.2, §56.5, Appendix D; base spec §31.16,
  §35.3; AGENTS.md §11, §16 (`unsafe` needs a `// SAFETY:` comment and an ADR); ADR-0442,
  ADR-0443, ADR-0444
- Decided by: agent (autonomous)

## Context

v0.4.1 §16.5 asks the supervisor to build a report for every spawn with five columns — `control`,
`required`, `attempted`, `result`, `platform_detail` — and states one invariant:

> A successful plugin spawn MUST imply every `required=true` control has `result=applied`.

Most of the controls are installed between `fork` and `exec`, in a child that then either execs
the artifact or dies. Whether `setsid` succeeded is a fact only that child ever knew. The standard
library's failure path carries one `errno` to the parent and nothing at all on success, so a
report built in the parent would be a report of what the parent *asked for*, which is exactly the
claim §0.5.3 found the code making and §2.6 forbids:

> If Ono cannot determine whether a plugin control was installed … it MUST report an explicit
> unknown/refusal state rather than claim success.

## Decision

**The parent maps one page of `MAP_SHARED | MAP_ANONYMOUS` memory before the fork, holding one
`AtomicU64` per `Control`, and the child writes each outcome as it installs it.**

The page survives `fork` as the same physical memory in both processes, so a relaxed atomic store
in the child is a load in the parent. The child's side is one store per control — no allocation,
no lock, legal in a `pre_exec` context — and the parent reads the page afterwards whether the
spawn succeeded or failed. `Outcomes` owns the mapping and unmaps it in `Drop`; `Send` and `Sync`
are asserted because every access goes through an `AtomicU64`, which is what that type is for, and
both impls carry the `// SAFETY:` account AGENTS.md §16 requires.

Four consequences of building the report from observation rather than intent:

**A control the child never reached is `not_attempted`, never `applied`.** A mandatory failure
stops the sequence, and the rows after it say so. §2.6 in one word.

**A control the platform does not implement is `skipped`, not `failed`.** `install` answering
`io::ErrorKind::Unsupported` means nothing was refused because nothing was there. Either way it is
not `applied`, so a mandatory one still abandons the spawn — but a best-effort one reads honestly,
which is what `should_mark_a_best_effort_control_that_was_not_available_as_skipped_rather_than_applied`
is about.

**The report has a row per control the tier *claimed*, and none for the rest.** Appendix D closes
with "The UI/documentation MUST never infer the last four rows from the first rows", and a spawn
report listing `filesystem_isolation` at all would be an invitation to read a row about a control
nobody installs as an outcome. The `not_provided` rows are a statement about the tier and live in
the tier's table (ADR-0442), where a reader goes to ask what the tier is.

**`is_confined()` is the invariant, and it gates the child.** `sandbox::spawn` returns a report
only alongside a child, and only when `unmet()` is `None`. §16.5's sentence is not a property the
report describes; it is the reason the report and the child come back together.

**The report reaches the operator without `RUST_LOG=debug`.** `inspect plugin <id>` carries
`confinement` as a list of records beside `execution_tier` and `execution_boundary` (§54.2,
ADR-0448). `platform_detail` is the operating system's error text and nothing else, which is what
§16.5's "MUST not expose secrets" comes to in practice: an `errno` is a fact about the kernel.

## Consequences

Easy: the same page serves §16.3's error (which control failed), §16.4's diagnostic (a best-effort
failure that did not stop the spawn), §16.5's report and §54.1's refusal. One mechanism, four
requirements.

Hard: this is `unsafe` in a crate that denies it by default, and the mapping is process-lifetime
memory shared with a child. It is 168 bytes, mapped per spawn and unmapped when the report's owner
drops, so a leak would be bounded and visible; `debug_assert` on the `munmap` result makes a
mistake in this file loud in a debug build rather than silent in both.

Also hard: a child that is killed between `fork` and its first store leaves the page as
`not_attempted` throughout, and the spawn is then refused for the first mandatory control. That is
the right answer — nothing is known to be installed — but the reported control is the first one
rather than the reason, and the reason is whatever killed the child. The message says
`an earlier mandatory control failed first` in that case, which is honest and unhelpful; a better
answer needs the exit reason the parent gets from `spawn`, and that is not this increment's work.

Encoded by: `crates/ono-kuang-supervisor/tests/confinement.rs::should_report_the_state_of_every_control_after_a_successful_spawn`,
`::should_mark_a_best_effort_control_that_was_not_available_as_skipped_rather_than_applied`,
`::should_never_hand_back_a_plugin_whose_report_is_not_confined`,
`crates/ono-kuang-supervisor/src/report.rs::tests::should_carry_an_outcome_written_through_the_shared_page`,
`::should_refuse_to_call_a_spawn_confined_when_a_required_control_was_not_applied`,
`crates/ono-cli/tests/plugins.rs::should_show_the_execution_tier_and_its_controls_when_a_plugin_is_inspected`.

## Alternatives considered

**A pipe from child to parent instead of shared memory.** `write` is async-signal-safe and the
descriptor is inherited, so it would work. Rejected for the descriptor: it has to be created,
inherited, kept clear of the `fd_hygiene` control, read without blocking and closed in both
processes, where a page of atomics has no such surface. The pipe would also have to be drained
before `wait`, or a full buffer would deadlock the spawn.

**Build the report in the parent from what it asked for.** What a report of "the sandbox we
configured" would be, and what the code effectively claimed before §0.5.3. It cannot answer §16.5's
`result` column at all.

**Return the report only on failure.** Half of §16.5, and the wrong half: `inspect plugin` on a
*running* instance is where an operator asks what is in force, and "it started, so presumably
everything" is the inference Appendix D's last sentence forbids.

**Store the outcomes in a file under the instance's private directory.** Not async-signal-safe
(`open` and `write` are, but the path formatting is not, and it allocates), and it would put a
security record on a filesystem the plugin can write.
