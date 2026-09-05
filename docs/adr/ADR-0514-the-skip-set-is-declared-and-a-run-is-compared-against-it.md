# ADR-0514: The skip set is declared, and a run is compared against it in both directions

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §38.2 (canonical CI expectation), §38.3 (unexpected skip is failure), §38.4
  (taxonomy), §52.1 (`expected_test_skips` is one of the seven registries), §52.2 (single source
  of truth), §65.10, Appendix G
- Issues: #89
- Decided by: agent (autonomous)

## Context

ADR-0513 made every skip announce itself with a category. §38.2 and §38.3 are what turn that
record into a gate: the expected skip set MUST be declared for the canonical CI environment, and
a test that becomes skipped when it was expected to run MUST fail.

The gap this closes is #14's, in a different place. Five acceptance cases had never run in the
container, and the suite was green the whole time, because nothing compared what ran against what
was supposed to run. A skip is the same defect one layer down.

One thing had to be established before any of it could work. `cargo test` captures `println!` and
`eprintln!` and prints them **only for tests that failed**. A skip belongs to a test that passed,
so through the macro the marker existed in every run and was visible in none of them — a full
`cargo test --workspace --all-features` on this machine grepped zero `SKIPPED` lines while three
tests were emitting them.

## Decision

**`docs/contracts/hardening/expected_test_skips.yaml` declares both lists, `xtask spec-check` holds the
first against the tree, and `xtask skip-check <log>` holds the second against a run.**

### The marker reaches the harness output

`ono_testkit::skipped` writes to `std::io::stderr()` directly rather than through `eprintln!`.
libtest's capture replaces the macros' sink, not the stream, so the marker now appears in the run
that took it. §38.1 asks for the skip to be *visible*, and an emission nobody can see is not that.

### Two lists, because §38.2 and §38.3 ask two questions

* **`declared:`** — every test in the tree that *can* announce a skip, with its §38.4 category and
  the prerequisite a host has to supply. Sixty-nine rows today. `scan::check_expected_skips` reads
  the tree and compares in both directions on every gate run: a skip nobody declared fails, and a
  row whose test no longer skips fails too. So the registry cannot fall behind the suite, and
  §38.2's *"their IDs and reasons MUST be listed"* stays true rather than becoming true once.

  The tree is read for both the direct announcements and the indirect ones. `unprivileged()` in
  `network.rs` prints the marker and returns `false`, and eleven tests skip through it without
  naming a category themselves; the registry has to hold what a run can produce, not what is
  written at one of the two places.

* **`canonical_ci.expected_skips:`** — what a run on the `ubuntu-latest` runner is expected to
  emit. §38.2 prefers zero and this is three, and none of the three is coverage anybody lost:
  `ono-testkit`'s own suite emits two markers to prove the marker's shape, and
  `acts_as_the_shell_when_a_role_is_requested` is the re-exec entry point the PTY tests drive,
  which `cargo test` also runs once with no role to act as. Every *environmental* skip is expected
  to be zero.

### The verification step is a step, not the static check

§38.3 permits *"the CI gate or an explicit skip-verification step"*, and this is the step:
`scripts/gate.sh` tees the test run and, when `ONO_CANONICAL_CI=1`, runs
`cargo xtask skip-check target/gate-test.log`. `.github/workflows/ci.yml` sets the variable; a
developer machine prints the observed count instead.

That split is the whole of the honesty here. The expectation is declared *for one environment*. A
developer machine without systemd legitimately skips what CI does not, and a gate that failed on
it would teach people to skip the gate — which is the failure mode §65.10 is about, arrived at
from the other side. What every machine still enforces is the static half: a skip the registry
does not declare is red everywhere.

## Consequences

Easy: "how much of this run was real?" is a question with two answers now — the count, and whether
it is the count somebody decided on. A prerequisite the CI image stops supplying turns the run
red at the test that lost it, naming the category, instead of quietly subtracting a test from the
suite.

Hard, and the first thing that will happen: the `canonical_ci` list is a **prediction**. It was
measured on the reference developer machine, which supplies every prerequisite the sixty-nine rows
name — live systemd with a readable journal, rtnetlink and sock_diag, an unprivileged user, a
second mount, `find` and `look` on `PATH`. The `ubuntu-latest` runner is believed to supply the
same set and has never been measured emitting these markers, because the markers did not reach a
log until this commit. The first CI run either confirms the prediction or names the prerequisite
the image does not supply, and `skip-check`'s message says which row to move and why. A red run
there is the mechanism working, not a regression — but it is a red run somebody has to read, and
saying so here is cheaper than being surprised by it.

Also hard: sixty-nine rows are a file somebody maintains. The maintenance is bounded — the gate
tells you exactly which row to add or delete, with the id already formatted — and it is the price
of §38.2's "IDs and reasons MUST be listed". A registry that summarised instead of enumerating
could not answer §38.3's reverse question at all.

Encoded by: `xtask/tests/scan.rs::should_fail_on_a_skip_the_expectation_does_not_declare`,
`::should_fail_when_a_declared_skip_no_longer_happens`,
`::should_accept_a_run_whose_skips_are_exactly_the_declared_ones`,
`::should_report_this_repositorys_observed_skips_as_exactly_the_declared_set`.

## Alternatives considered

**Declare a count rather than a set.** "Three skips are expected" cannot answer §38.3's reverse
question: three skips of which three were different tests is the same number and a different run.

**Put the verification in `spec-check` and drop the log.** `spec-check` is static; it can say a
skip is *permitted* and never that it *happened*. The two halves are different questions and the
static one alone is what let #14 sit for a release.

**Set an environment variable for the skip log instead of writing to stderr.** It works, and it
adds a channel that only the gate reads — so a developer running `cargo test` by hand would still
see nothing, and the marker's visibility is the property §38.1 asks for. Writing past the capture
gives every run the same output.

**Fail the gate on any skip, everywhere.** §38.2 prefers zero and this repository is close to it,
but a host without a second mount cannot be argued into having one. The rule that everyone can
keep is the one that is enforced where the environment is controlled and reported where it is not.
