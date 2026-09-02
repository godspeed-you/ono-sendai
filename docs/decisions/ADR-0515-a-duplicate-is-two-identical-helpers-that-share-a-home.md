# ADR-0515: A duplicate is two identical helpers that share a home

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §39.1 (shared helpers are production-quality test infrastructure), §39.2 (no
  helper divergence), §39.3 (helper contracts), §39.4 (test code review); AGENTS.md §11
- Issues: #90
- Decided by: agent (autonomous)

## Context

ADR-0427 settled when a helper may be shared — *"only when every copy of it is byte-for-byte
identical"* — moved the helpers that qualified, and named what it left behind: *"the remaining 152
lines of identical helpers in groups of three are left for the crates that have no support module
yet."*

§39.1 asks for the rest, and §39.2 asks for the thing ADR-0427 could not provide: a gate check.
*"The gate MUST include a lightweight structural check preventing reintroduction of known
duplicated helper definitions where a canonical helper exists. This check SHOULD target
semantics/signatures rather than fragile exact source strings."*

Written and run, the rule found fifty-five groups. Reading them apart is where the design is,
because three of the classes it found must not be consolidated and one of them is ADR-0427's own
worked example.

## Decision

**A pair is a duplicate when three things hold, and each of them is one of ADR-0427's reasons
written as code.** `scan::check_duplicate_helpers` runs in `spec-check` on every gate run.

### 1. The signature and the body match, after formatting is removed

Comments, whitespace and line breaks go; parameter *names* go, leaving what the helper takes and
answers. That is §39.2's "semantics/signatures rather than fragile exact source strings" — the
same helper reflowed by rustfmt is still the same helper, and one that differs by a budget or a
panic message is not.

The name is not compared. ADR-0427 found one name over eleven behaviours; this is the same defect
from the other end. `listening_tcp` in `socket_decoding.rs` and `unowned_listener` in
`socket_process_join.rs` were one function under two names — a helper somebody could not find, so
they wrote it again.

### 2. The body is self-contained

A body that calls a function its own file defines means whatever that function means.
`files.rs::single_result` is byte-identical to three others and calls `files.rs::text`, which
names an `ActionResult` field in its panic — which is exactly why ADR-0427 left it where it was.
Two identical bodies over two different callees are two behaviours, and moving one would change
the diagnostic a reader depends on. The check reads the callees and stays out of it.

### 3. The copies share a home

§39.2 asks for the check *"where a canonical helper exists"*, and a home is
`crates/<crate>/tests/support/mod.rs` for one crate's suites or `xtask/tests/support/mod.rs` for
the automation's. A pair spanning two crates has no home that does not put a crate's own types
into `ono-testkit`, which ADR-0427 rejected because the testkit is meant to be neutral about the
crates it serves. Seven such pairs exist — `record` and `socket_with` between `ono-spatial-index`
and `ono-spatial-query`, `process` between `ono-graph` and `ono-provider-linux`, `within` between
`ono-protocol` and `ono-remote`, `map` between `ono-render` and `ono-value`, and
`ono-spatial-core::projection::service` against `ono-spatial-index::index::service_record` — and
they are named here rather than reported, so the boundary is a decision somebody made instead of a
gap somebody left.

A body under eighty normalised characters is left alone. A two-line accessor is not a helper
anybody consolidates, and a rule that reported it would be a rule people turn off.

### What moved

Twenty-four groups, into five support modules — three of which are new
(`xtask/tests/support/mod.rs`, `crates/ono-adapter/tests/support/mod.rs`,
`crates/ono-render/tests/support/mod.rs`, `crates/ono-spatial-render/tests/support/mod.rs`) — plus
the four that already existed. Two renames were needed, and both are the finding of §39.2 rather
than a tidy: six suites had written the same `ono(home, script)` under two names, and it is now
`support::ono_at_home` because `ono` already means `ono_testkit::ono` and a second meaning for one
word is how the drift started; the plugin suites' `ono` is `support::ono_with_plugins`, because it
is a different helper with the same name.

`SleepChild` moved with its richest form, the one from `processes.rs`. `spatial_pins.rs` and
`spatial_topology.rs` keep theirs: one waits for the child to settle and the other gives it a
duration nothing else on the host shares, so neither is a copy.

## Consequences

Easy: one definition per job, and a new copy cannot be added quietly — the gate names both
locations and the module to move it to. §39.4's review rule is now a gate rule, so the argument
happens when the copy is written rather than at review.

Hard: the check is line-based and compares text, so it can be defeated by rewriting a body to mean
the same thing differently. That is the honest limit of a "lightweight structural check", and it
is the one §39.2 asks for; a rule that understood semantics would need to run the helpers.

Also hard: three of the four new support modules exist for one or two helpers each. ADR-0427
declined that trade — *"adding one for six lines would cost more than it saves"* — and this
reverses it, because the saving is no longer the six lines. It is that the gate now has somewhere
to point when the seventh copy is written.

Encoded by: `xtask/tests/scan.rs::should_report_two_test_helpers_that_do_the_same_job_under_different_names`,
`::should_leave_two_helpers_that_differ_alone_when_scanning_for_duplicates`,
`::should_leave_a_helper_alone_when_it_calls_its_own_files_helper`,
`::should_leave_two_crates_helpers_alone_when_they_share_no_home`,
`::should_report_this_repository_as_using_the_canonical_helper_everywhere`.

## Alternatives considered

**Report every identical pair, including the cross-crate ones.** It would name seven real
duplicates the repository cannot fix without a dependency edge ADR-0427 refused. A gate rule whose
only available answer is an exemption is a rule that trains people to write exemptions.

**Compare by name instead of by body.** That is what a reviewer does, and it is what ADR-0427
proved wrong: `ono` had eleven implementations and `rows` thirteen. A name says two people wanted
the same thing, not that they got it.

**Unify the near-identical variants too, with a parameter for the difference.** Four suites'
`ono` differ only in a timeout, so one helper with a budget argument would cover them. It would
also change what four suites do, which is the change AGENTS.md §11 forbids inside a refactor, and
ADR-0427 rejected it for the same reason. `ono_within` already exists for a suite that wants to
name its budget.

**Leave the check to review (§39.4's own wording is SHOULD).** The fifty-five groups were written
by people who would have said no if asked. What was missing was somebody asking.
