# ADR-0513: A skip names its category, and a silent return is a failure

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §38.1 (three visible outcomes), §38.4 (skip reason taxonomy), §65.10
  (skip-as-pass), Appendix G (test result contract); AGENTS.md §10, §11
- Issues: #88
- Decided by: agent (autonomous)

## Context

ADR-0428 gave a skip one greppable marker and stopped there, deliberately: it recorded that
detecting the early return itself would need a scanner to guess, and *"a check that guesses is a
check people work around."*

v0.4.1 closes both halves of that gap by name. §38.1 requires `PASS`, `FAIL` and `SKIP(reason)`
to be distinguishable and forbids *"a function returning early while the test harness reports
success without any skip marker."* §38.4 fixes six stable categories, because §38.2's expected
skip set compares categories and free text cannot be compared. §65.10 states the defect flatly:
*"A test returning before its assertion path without an explicit skip outcome is forbidden."*

The tree held both defects. Eight skips announced a reason and no category. Forty-one tests
returned before asserting anything and said nothing at all — ten in `storage.rs`, eleven in
`network.rs`, eighteen in `services_logs.rs`, six in `spatial_identity.rs`. And one hand-rolled
`eprintln!("skipped: …")` in `spatial_map.rs` had survived ADR-0428's scan for a release, because
the scan read only the line the macro opened on and that one carried its string on the next line.

## Decision

**A skip carries a category from §38.4, and a test that returns before its assertion path without
one fails the gate.**

### The helper takes a category

`ono_testkit::skipped(SkipReason, detail)` replaces `skipped(reason)`. `SkipReason` is the closed
set of §38.4 — `missing_kernel_feature`, `missing_privilege`, `unsupported_arch`,
`unsupported_distribution`, `external_tool_unavailable`, `fixture_not_applicable` — and the
detail beside it stays free text, as §38.4 permits. The marker is one line:

```text
SKIPPED should_cross_a_mount_boundary: fixture_not_applicable: this host reports no mount below `/`
```

Appendix G writes the marker as `SKIP <category>: <detail>`; this one keeps ADR-0428's `SKIPPED`
token and adds the test name in front, because §38.2's registry compares *IDs and categories* and
an ID the marker does not carry is an ID the verifier would have to guess. Appendix G asks for an
API *equivalent to* its sketch, and a superset of its fields is that.

`require(condition, reason, detail) -> TestPrerequisite` is Appendix G's prerequisite helper. It
announces the skip when the condition fails, so `if require(…).unmet() { return; }` is a return
path that has already emitted the canonical signal.

### The gate reads the return

`scan::check_unannounced_skips` reports a bare `return;` inside a `#[test]` function. It does not
guess, because the specification names the escapes and all three are readable from the source:

* **a `return` inside a closure or an `async` block** leaves that block, not the test. Detected by
  the block's opening line, so `sink.send(..).await.is_err()` in a fixture is untouched.
* **a block that asserted before it returned** has reached an assertion path, which is exactly
  what §65.10 asks it to reach. `should_either_read_the_route_table_or_say_why_not` asserts that
  an unavailable provider gave a reason and then returns; that is the test doing its job.
* **a block on whose path the skip was announced** — either it calls `skipped`/`require` itself,
  or the guard that opened it calls a helper *in the same file* whose body does. `unprivileged()`
  in `network.rs` prints the marker and returns `false`, and its twenty-one callers write
  `if !unprivileged() { return; }`. That is Appendix G's *"the return path has already emitted the
  canonical explicit skip signal"*, and the only way to see it is to read the helper — so the scan
  reads it.

Everything else is a test whose summary line says `ok` for a run in which it asserted nothing.

`check_silent_skips` also learned to read the line after a macro that opens with no argument on
it, which is how the `spatial_map.rs` announcement had stayed invisible.

### What the forty-one became

Two of them are one edit each rather than forty-one: `records_or_unavailable` and
`one_failed_row` in `services_logs.rs` announce `external_tool_unavailable` where they answer
`None`, which covers all eighteen of that file's returns without eighteen copies of one line.
`spatial_identity.rs` gained the `unprivileged()` shape its six neighbours in `storage.rs` and
`network.rs` already had. The rest name their category at the site.

## Consequences

Easy: `cargo test 2>&1 | grep SKIPPED` now answers *which* prerequisite was missing in a
vocabulary a registry can compare, which is what #89 needs and what §38.2 requires. A new silent
skip cannot be added: the gate names its file and line.

Hard: the scan is line-based and therefore has a horizon. A guard whose announcing helper lives in
another file — a `support/mod.rs` rather than the suite — is not recognised, and the honest
workaround is to announce at the site. That is a narrower rule than a type system would give and a
wider one than ADR-0428 was willing to write, and the difference is that the specification now
names the escapes instead of leaving the scanner to invent them.

Also hard, and deliberate: `SkipReason` is closed. A precondition that fits none of the six has to
be argued into one of them, which is the point — §38.2's comparison is only meaningful over a
vocabulary that does not grow to fit whatever a test wanted to say.

Supersedes ADR-0428's *Alternatives considered* entry **"Detect the early return itself"**.
ADR-0428 remains accepted for everything else it decided.

Encoded by: `crates/ono-testkit/tests/harness.rs::should_name_the_test_the_reason_and_the_category_when_a_skip_is_announced`,
`::should_offer_a_require_helper_that_records_an_unmet_prerequisite`;
`xtask/tests/scan.rs::should_reject_a_test_that_returns_before_its_assertion_path_without_a_skip`,
`::should_accept_a_test_that_announces_its_skip_before_it_returns`,
`::should_accept_a_guard_whose_own_helper_announced_the_skip`,
`::should_accept_a_branch_that_asserted_before_it_returned`,
`::should_leave_a_return_inside_a_closure_alone_when_scanning_for_unannounced_skips`,
`::should_reject_a_test_that_announces_a_skip_on_the_line_after_the_macro`,
`::should_report_this_repository_as_announcing_every_skip_it_takes`.

## Alternatives considered

**Keep the free-text reason and let the registry match on the text.** §38.2 compares an expected
set to an observed one; two runs that phrase the same missing mount differently would be two
different skips. A category is what makes the comparison mean anything.

**Parse the test files with `syn` instead of scanning lines.** The rest of `xtask/src/scan.rs`
reads lines, so this would add a parser dependency and a second mechanism for one rule. It would
also lose the file:line a red gate needs to be actionable without extra bookkeeping. If a second
rule needs a syntax tree, both should move together.

**Report every early return and let the suites add `#[allow]`s.** An exemption a test writes for
itself is an exemption every test acquires. The three escapes above are properties of the code
rather than annotations on it, so nothing can be silenced by asking.

**Make the skip a hard failure and delete the outcome.** §38.2 prefers zero expected skips and
§38.3 makes an undeclared one fail — but a host without a second mount cannot be argued into
having one, and a test that fails there teaches people to skip the suite instead of the test.
#89 is where the count becomes an assertion.
