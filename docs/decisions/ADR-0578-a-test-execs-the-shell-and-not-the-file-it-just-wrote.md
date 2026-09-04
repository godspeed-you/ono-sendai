# ADR-0578: A test execs the shell, and not the file it just wrote

- Status: accepted
- Date: 2026-09-04
- Spec refs: v0.4.1 §65.10, §38.1; AGENTS.md §11, §14; ADR-0520
- Issues: none (found by the CI run of `67d097f`)
- Decided by: agent (autonomous)

## Context

`ono_model_broker::tests::should_stop_waiting_when_the_budget_runs_out` failed on GitHub Actions
while every local run was green:

```
left:  Unavailable("`/tmp/ono-test-30580-2/slow` could not be started: Text file busy (os error 26)")
right: Timeout(1s)
```

The test writes a shell script into a scratch directory, marks it executable and has the broker
exec it. `std::fs::write` closes its descriptor, but closing it in *this* thread is not enough: any
other test that spawns a process forks, the child inherits every descriptor open at that moment,
and `O_CLOEXEC` only clears it when that child reaches its own `exec`. In the window between the
two, a descriptor still open for writing points at the file this test is trying to execute, and
the kernel answers `ETXTBSY`.

The window is small, the suite is large and parallel, and the machine that hits it is whichever one
is busiest — which is CI. `crates/ono-cli/tests/spatial_contracts.rs` already carries a paragraph
about the same race, worked out the same way, for a different test. That is the signal this is a
rule rather than an incident.

## Decision

**A test that needs a script to run does not exec the script.** It execs `/bin/sh` and hands the
script over as an argument:

```rust
provider.command = vec!["/bin/sh".to_owned(), script.to_string_lossy().into_owned()];
```

`/bin/sh` is not a file any test writes, so no test can hold a write descriptor to it, and the
script becomes data that `sh` opens for reading — which no `ETXTBSY` applies to. The execute bit
becomes unnecessary and the `chmod` goes with it.

This is not a retry and not a sleep. §38.1 asks that a test result mean what it says, and a retry
around `ETXTBSY` would leave a genuine "not executable" defect looking like a slow start.

## Consequences

Easy: the three broker tests are deterministic under any load, and one fewer thing makes a green
local gate disagree with a red CI run — which is the failure mode the session of 2026-09-04 spent
five red runs on.

Hard: the rule has to be known to be followed, and this ADR is the only place it is written down.
Other suites still exec a file they wrote — `ono-kuang-sdk`, `ono-kuang-supervisor`, `ono-cli` and
`ono-provider-linux` each have at least one, and `ono_testkit::executable` is the shared helper
several of them use. None has been observed failing this way, so none is changed here: §4 keeps a
fix to the defect in front of it, and the survey is recorded under *Found, not yet filed* in
`docs/STATE.md` for the increment that decides whether the helper should take the same route.

Encoded by `crates/ono-model-broker/src/lib.rs` — `should_answer_from_a_command_provider`,
`should_report_a_command_that_does_not_speak_the_protocol` and
`should_stop_waiting_when_the_budget_runs_out`.

## Alternatives considered

**Retry the spawn on `ETXTBSY`.** It hides the one case where the error is real, and §65.10's rule
against a test that passes without exercising its subject is the same rule from the other side.

**Serialise the tests that write executables.** It makes the suite slower for every run to fix a
race that only exists between a write and an exec, and it does not help a suite that grows a new
one.

**Change `ono_testkit::executable` to write through a temporary name and rename.** A rename does
close the window for the file that is finally exec'd, and it is the right shape for the shared
helper — but it changes a helper five crates depend on, in a commit about a broker test. That is
the survey recorded on the board, not this fix.
