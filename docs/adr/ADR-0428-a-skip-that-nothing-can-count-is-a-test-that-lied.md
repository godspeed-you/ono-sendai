# ADR-0428: A skip that nothing can count is a test that lied

- Status: accepted
- Date: 2026-09-01
- Spec refs: AGENTS.md §10 (the gate decides whether an increment is sound), §11 (tests assert
  outcomes), docs/ACCEPTANCE.md §3 (a box ticked by nothing is the one failure forbidden)
- Decided by: agent (autonomous)

## Context

Some tests cannot run their subject on every host. `storage.rs` asserts what an unprivileged user
is refused, and as root there is no refusal to observe. `spatial_storage.rs` needs a second mount
to cross, and a host may have exactly one. `spatial_navigation.rs` needs `git` on `PATH`.

Each of them handled it the same way and wrote it differently: eight hand-rolled
`eprintln!("skipped: …")` lines in six files, in eight formats — `skipped:`, `skipped the
external half:`, some naming the precondition, some naming the section. Then each returned early.

`cargo test` has two outcomes, and this is neither. The summary counts every one of these as
`ok`, beside the tests that actually asserted something. Roughly thirty tests reach the suite
total that way on a host that cannot meet their preconditions, and nothing in a green run says
so. This is the defect `docs/ACCEPTANCE.md` §3 names in a different costume: coverage claimed by
something that proved nothing.

## Decision

**A test that gives up on its precondition announces it through `ono_testkit::skipped(reason)`,
and `xtask spec-check` refuses any other spelling.**

The helper prints one marker on stderr, naming the test and the reason:

```
SKIPPED should_show_the_source_device_and_filesystem_when_the_place_is_a_mount_boundary: this host reports no mount below `/` to enter
```

The test name comes from the thread `cargo test` runs it on, so the marker cannot go stale by
being copied. `SKIPPED` is one greppable token, so "how much of this run was real?" is a question
with an answer — `cargo test 2>&1 | grep -c SKIPPED` — rather than a thing nobody knows.

The gate rule is deliberately narrow: `scan::check_silent_skips` rejects a test *announcing* a
skip its own way. It does not try to detect a test *deciding* to skip. No scanner can tell a
precondition guard from ordinary control flow, and one that guessed would either cry wolf or
teach people to phrase guards so it looks away. What the gate can insist on is that the decision
leaves a record.

A skip stays a last resort. Where the precondition can be arranged, arranging it is the fix —
ADR-0417 already settled that a test needing a running process spawns one.

## Consequences

Easy: a skipped test is visible in the run that skipped it and countable afterwards. A new one
cannot be added quietly, because the gate names the file and line of any hand-written notice.

Hard: `ono-testkit` is now a dependency of any test that can skip, which it already was
everywhere this applies. And the count is still a number a human has to read — nothing fails on
it. Making a skip fail the gate was rejected below.

Encoded by: `xtask/tests/scan.rs::should_reject_a_test_that_announces_a_skip_with_its_own_print`,
`::should_report_this_repository_as_announcing_every_skip_through_the_helper` (the whole
repository, so a regression is caught where it lands),
`crates/ono-testkit/tests/harness.rs::should_name_the_test_and_the_reason_when_a_skip_is_announced`.

## Alternatives considered

**Make a skip fail the gate under an environment variable** — the honest version of this needs a
host where every precondition holds, and there is none: the gate runs on developer machines, and
the acceptance container runs the binary against `docker/acceptance/cases/`, not `cargo test`. A
flag nothing sets is a flag that rots.

**`#[ignore]` with a reason** — already checked by `scan::check_unfinished_work`, but static: it
would skip on hosts that *can* run the test, which loses more coverage than it reports.

**Detect the early return itself** — a precondition guard and an ordinary `return` are the same
Rust. The scanner would have to guess, and a check that guesses is a check people work around.
