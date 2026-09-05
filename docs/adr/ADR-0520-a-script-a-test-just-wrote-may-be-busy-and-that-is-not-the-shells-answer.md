# ADR-0520: A script a test just wrote may be busy, and that is not the shell's answer

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §2.7 (tests report execution truth), §38.1, §39.1 (shared helpers), §65.10;
  ADR-0008 (exit status conventions)
- Issues: #27, #7
- Decided by: agent (autonomous)

## Context

Two issues, filed separately, three months of sightings apart, one defect.

**#27.** `ono-process::should_run_a_text_script_without_a_shebang_through_the_shell` answered exit
**126** once, under a `cargo test --workspace` with a container build beside it. 126 is
ADR-0008's *"found and not executable"*, for a script the test wrote and chmodded `0755` two
statements earlier. The issue's own note — *"points at a race between writing it, marking it
executable and spawning it"* — is right about where and wrong about what.

**#7.** `should_report_a_failing_streamed_child_after_its_records` failed once and was never
reproduced. It runs the shell against a fake `journalctl` the test writes, and asserts the shell
reports `Ono-Sendai-E0501` for the shim's non-zero exit. If the shim cannot be exec'd, the shell
reports something else entirely and the assertion never gets its chance.

The race is neither `write`-then-`chmod` nor `chmod`-then-`spawn`. `cargo test` runs a crate's
tests in **threads of one process**, and a thread that `fork`s between another thread's `open` and
`close` of a file inherits the write descriptor. Until that child `exec`s, `execve` on the file
answers **`ETXTBSY` — text file busy**. Both suites spawn processes constantly, so both supply
their own forks; a container build beside them supplies more.

`ETXTBSY` reaches `spawn::exec_failure`'s catch-all arm and becomes `ExitStatus::NOT_EXECUTABLE`,
which is the right status for a shell to report and the wrong thing for a test to believe. §2.7:
a test reports execution truth, and "another thread of this test binary was holding the file" is
not a fact about the shell.

## Decision

**A test that runs a script it just wrote waits out a writer, and answers everything else at
once.**

`ono_testkit::while_text_file_busy(busy, attempt)` runs `attempt` again while `busy` says *this
answer* is the machine reporting a busy file, up to one second. `ono_testkit::executable_script`
writes and chmods, so the two suites that had spelled that by hand share one definition (§39.1).

Two properties make it a fix rather than a retry loop:

* **`busy` is asked about the answer, not about the attempt.** Both call sites recognise the
  machine's own words — the diagnostic carries `Text file busy` from the `io::Error` — so every
  other failure is returned on the first attempt, unretried. A script that genuinely cannot be run
  fails as fast as it did before, with the message it always had.
* **The window it waits out is the distance between a `fork` and its `exec`**, which is
  microseconds. One second is four orders of magnitude of headroom, and a file still busy after it
  is a finding, answered as one.

## Consequences

Easy: both tests are deterministic for the reason they were flaky. Twelve repetitions of
`external_command` and ten of `adapters` on a machine loaded to 6 by six concurrent workspace
builds produced no failure.

Hard: this is a retry, and a retry is the shape of a paper over a defect. What keeps it from being
one is the predicate. The answer being retried is not "the test failed" but "the operating system
said the file was busy", which is a sentence about the machine that no amount of correctness in
the shell would change. The alternative — making the window zero — is not available: nothing in
this process can stop another thread forking while a file is open, and no rewrite of the write
makes the inherited descriptor go away.

Also hard, and worth a later look rather than a fix here: **`ETXTBSY` becomes 126 in the shell
too.** A user who runs a script somebody is still writing is told "found and not executable" about
a file that is executable, and "text file busy" is what happened. That is a product wording
question in `spawn::exec_failure`'s catch-all arm, not a test one, and it is recorded rather than
changed in a `fix` for two flaky tests.

Encoded by: `crates/ono-testkit/tests/harness.rs::should_run_a_script_again_while_another_thread_still_holds_it_open`,
which forces the condition by holding a write descriptor and asserts the raw `ETXTBSY` before it
does, and `::should_answer_a_failure_that_is_not_a_busy_file_on_the_first_attempt`; plus
`crates/ono-process/tests/external_command.rs::should_run_a_text_script_without_a_shebang_through_the_shell`
and `crates/ono-cli/tests/adapters.rs::should_report_a_failing_streamed_child_after_its_records`
themselves.

## Alternatives considered

**Retry on any failure.** It would have fixed both sightings and hidden the next real one. The
predicate is what makes this honest, and it costs one `contains`.

**Copy the script to a second path and run that.** `ETXTBSY` is a property of the inode, and a
copy is written the same way with the same window.

**Give each test its own process (`--test-threads=1`, or one binary per test).** It removes the
forks that cause the race and multiplies the suite's wall time by the number of tests. The race is
not the test suite's fault; serialising the suite to avoid it is paying for the wrong thing.

**Mark both tests `#[ignore]` until someone reproduces them.** An ignored test is untracked
unfinished work (AGENTS.md §7), and the reproduction is now written down: hold a write descriptor
open and `execve` the file.
