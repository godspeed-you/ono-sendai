# ADR-0891: A test script is written by a process of its own

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.4.1 §38.1, §65.10; AGENTS.md §11, §14
- Issues: #188
- Related: ADR-0520 (the bounded retry), ADR-0578 (exec `/bin/sh`, not the script)
- Decided by: agent (autonomous)

## Context

A test that writes a script and then has it executed can fail with `ETXTBSY` for a reason no
part of the product caused. `cargo test` runs a crate's tests as threads of one process; a thread
that starts a process copies the whole descriptor table into the child, and `O_CLOEXEC` only closes
the copy when that child reaches its own `exec`. If the copy is a descriptor open for writing to
the script, `execve` of the script is refused until then.

ADR-0578 fixed one crate by executing `/bin/sh` with the script as an argument, and recorded
"write through a temporary name and rename" as the right shape for the shared helper,
`ono_testkit::executable_script`. Issue #188 asks for one of the two everywhere.

**The rename does not close the race.** The kernel's refusal is per inode (`i_writecount`), and a
rename moves a name, not an inode: a stray descriptor to the temporary name points at exactly the
inode that is executed under the final name. This was checked rather than argued: a stress test
that runs six threads spawning `/bin/true` beside a thread that writes and executes three hundred
scripts saw `ETXTBSY` 8, 5 and 7 times with the in-place helper, and 9, 16 and 5 times with a
write-to-temporary-and-rename helper. `fsync` changes nothing either; the problem is who holds a
descriptor, not what is on disk. Closing in the same thread is what `std::fs::write` already did.

ADR-0578's route is not available everywhere either. It works when the test itself chooses the
program. Most of the affected suites instead put a stand-in (`journalctl`, `lsblk`, `rpm`, `cosign`,
a plugin's helper, a KUANG/11 artifact) where the *product* will find and execute it by path, and
the product does not go through `/bin/sh` — nor should it, for a test's sake.

## Decision

**A script a test will have executed is never opened for writing by the test process.**
`ono_testkit::executable_script` hands the bytes to a `/bin/sh -c 'cat > "$1"'` of its own, over a
pipe, and waits for it. The only descriptor ever open for writing to the file belongs to that
process (and the `cat` it runs), which no test thread forks from, so once it has been waited for,
no writer to the inode exists anywhere. The file is then made executable and renamed into place from
a staging name, so a script being replaced is never visible half-written.

Every test that writes a file some process then executes goes through that helper. Where the test
itself chooses the program, ADR-0578's `/bin/sh <script>` remains correct and is not changed.

The bounded retry of ADR-0520, `ono_testkit::while_text_file_busy`, is no longer needed around a
helper-written script, and the suites that wrapped their runs in it no longer do: a retry around a
race that cannot happen would only hide a genuine "not executable" defect. The function stays in
the testkit for a caller that runs a file it did not write.

## Consequences

Easy: `ETXTBSY` from a test-written script cannot occur, deterministically and under any load,
rather than being retried away. Encoded by
`crates/ono-testkit/tests/harness.rs::should_run_a_script_it_has_just_written_while_other_threads_are_starting_processes`,
which failed with the old helper (between one in a hundred and sixteen in three hundred runs) and
passes with this one.

Hard: writing a script now costs a process start (`/bin/sh` and `cat`), a few milliseconds each
on a quiet machine; the suites write at most a handful per test. `/bin/sh` and `cat` must exist,
which every host that runs this suite already requires.

The rule has to be followed by new tests. A test that writes an executable with `std::fs::write`
reintroduces the race; the helper's documentation says why.

## Alternatives considered

**Write through a temporary name and rename** (ADR-0578's suggestion, the issue's first option).
Measured above: it leaves the race exactly where it was.

**Take ADR-0578's route everywhere.** Only possible where the test chooses the program; not for a
stand-in the product executes by path.

**Serialise process creation in the test process** (a lock every spawn takes). Every spawn in every
crate would have to take it, including the ones inside the product under test; it cannot be
enforced.

**Keep retrying on `ETXTBSY`** (ADR-0520). Bounded and honest about what it retries, and still a
retry around a race that no longer needs to exist.

**`memfd`/`execveat`.** Executes a descriptor, not a path; the product executes paths.
