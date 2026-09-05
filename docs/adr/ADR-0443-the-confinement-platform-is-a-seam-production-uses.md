# ADR-0443: The confinement platform is a seam production uses, and a scan holds the rule

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §0.5.3, §2.3, §2.6, §16.1, §16.2, §16.4, §56.5, §59.7, §59.8, §65.4,
  Appendix D; base spec §31.15, §31.80; AGENTS.md §4 (no speculative generality), §7, §11;
  ADR-0283, ADR-0430, ADR-0442
- Decided by: agent (autonomous)

## Context

v0.4.1 §16.2 requires that "every syscall used to establish a mandatory security or resource
control MUST have its return value checked", and §65.4 names the opposite as a failure mode of
this release: *"Calling a confinement syscall, discarding its result and executing the plugin
anyway is forbidden."* §0.5.3 found seven such calls in one `pre_exec` closure in
`crates/ono-kuang-supervisor/src/sandbox.rs`, every one of them discarded, the closure ending in
an unconditional `Ok(())`.

Two things had to be decided, and they are the same decision seen from two sides.

**How does a test make a control fail?** §59.7 asks for an acceptance scenario in which
`PR_SET_NO_NEW_PRIVS` fails and the plugin never runs. ADR-0430 established that a failure proof
arranges its failure from *outside* the process, and found exactly one control that can be failed
that way — `setsid`, refused with `EPERM` for a process-group leader. It also recorded what that
buys and what it does not: *"`PR_SET_NO_NEW_PRIVS` does not fail on any Linux that has it … so
phase H4 still owes the injectable platform layer."* It does. `prctl(PR_SET_NO_NEW_PRIVS)` has no
external failure mode at all, and an unprivileged `setrlimit` failure needs a hard limit lowered
for the whole test binary.

**How does the rule survive the next syscall?** §16.2 is a rule about every member of an open
set, and the next member is added by someone who has not read §16.2. A review cannot hold that,
and neither can a test that enumerates today's controls.

## Decision

**`ConfinementPlatform` is one trait with one method, production passes `NativePlatform`, and the
caller supplies it.**

```rust
pub trait ConfinementPlatform: Send + Sync + 'static {
    fn install(&self, control: Control, plan: &ConfinementPlan) -> io::Result<()>;
}
```

Four properties, each of which is the reason for a piece of the shape:

**1. It is a platform, not a fault injector.** The trait has no method for failing, no mode, no
flag. `LoadConfig::confinement` defaults to `NativePlatform::shared()` and the shell never sets
it; a test provides an implementation that refuses one control, which is ordinary polymorphism
rather than a hook. Nothing in a shipped binary can ask for a control to fail, which is what
ADR-0430 rejected the alternatives for and what §2.3 is about. `TestHost::confinement` exposes the
same seam at the boundary a host actually uses, so the §59.7 scenario is driven through
`Supervisor::load` rather than around it.

**2. One method taking a `Control`, not one method per syscall.** The driver iterates
`PLATFORM_CONTROLS` and asks the platform for each; the requirement comes from the central table
of ADR-0442, so what a refusal *means* is data rather than control flow. A control this platform
does not implement answers `io::ErrorKind::Unsupported`, which is a refusal like any other — so a
mandatory control nobody implemented fails the spawn instead of passing silently (§2.6). Adding a
control is adding a match arm that must produce a `Result`; there is no arm that can drop one.

**3. Every raw syscall goes through `checked()`.** One function turns a libc return into a
`Result`, and it is the only thing in `platform.rs` that looks at a `-1`. That makes §16.2 a
property a scanner can state.

**4. `xtask`'s `check_confinement_syscalls` reports the defect, not the shape of correct code.**
It runs over `crates/ono-kuang-supervisor/src`, blanks comments and string literals, and for each
`libc::` *call* reads backwards over whitespace and `unsafe { … }` — which carries a value
through unchanged — to see what the call's value flows into. A `;`, a `}`, a block-opening `{` or
the start of the file means the call is a statement and its value is discarded; an `=` whose
left-hand name begins with `_` means it was bound to a name nothing can read. Anything else — an
argument, a named binding, a `match` scrutinee — is left alone, because this scan has no business
having an opinion about it. It runs in `spec-check`, so it runs on every gate.

The scan is deliberately narrow: one crate, the one §56.5 makes responsible for fail-closed
pre-exec setup. A scanner that also reported a dropped `libc::close` in an unrelated file would
be a scanner somebody turns off.

**One control changed its implementation to fit the rule.** `fd_hygiene` marks every descriptor
above 2 close-on-exec (`close_range(…, CLOSE_RANGE_CLOEXEC)`, with a `fcntl` loop where the
kernel is older) rather than closing them. Closing them would close the pipe the standard library
uses to report a `pre_exec` failure to the parent, and a refused control that reads as a
successful spawn is precisely §65.4.

## Consequences

Easy: §59.7's scenario is a three-line fake in a test, and there is one such case per mandatory
control the platform installs, generated from `Control::ALL` rather than typed — so a control
added later cannot escape by nobody remembering to write its case. The `EPERM`-from-outside proof
of ADR-0430 keeps working unchanged beside them, driving the real syscall.

Hard: `install` runs between `fork` and `exec`, so every implementation — including a test's —
must be async-signal-safe. The trait's documentation says so and the production implementation
allocates nothing (`io::Error::last_os_error` carries the errno inline), but the compiler cannot
enforce it. A test fake that allocated could deadlock on an allocator lock held at the moment of
the fork, and it would deadlock rarely.

Also hard: the scan is a lexical rule, not a compiler. It can be defeated by writing
`let value = libc::setsid();` and never reading `value` — which the compiler's own `unused`
warning then catches, so the two together are tighter than either. It cannot see through a macro.

Encoded by: `crates/ono-kuang-supervisor/tests/confinement.rs::should_report_the_failing_control_when_a_confinement_syscall_returns_an_error`,
`::should_check_the_result_of_every_control_the_table_marks_mandatory`,
`::should_start_the_plugin_when_a_best_effort_control_is_refused`,
`::should_not_exec_the_plugin_when_a_mandatory_confinement_control_fails` (ADR-0430's proof, now
un-ignored), `xtask/tests/scan.rs::should_report_an_unchecked_confinement_syscall_result`,
`::should_report_a_confinement_syscall_result_bound_to_a_discarded_name`,
`::should_accept_a_confinement_syscall_whose_result_becomes_a_value`,
`::should_find_no_unchecked_confinement_syscall_in_this_repository`.

## Alternatives considered

**Keep the calls inline and add `?` to each.** The smallest possible fix, and it makes §16.2 true
today. Rejected on §59.7: without a seam there is no way to fail `PR_SET_NO_NEW_PRIVS`, so the
acceptance scenario the specification names could not be written, and the rule would be held by
nothing but the diff that introduced it.

**A `#[cfg(test)]` branch inside the pre-exec closure.** Smaller than a trait and unavailable to
an integration test, which compiles against the crate as a dependency. It would also put a
never-shipped branch inside the one function whose correctness is the point.

**A table of function pointers, swappable by a test (ADR-0430's own first alternative).** Rejected
there for H0 and rejected here too: a mutable table of pointers is a hook a reviewer has to argue
about, and a trait the caller passes is the same indirection with an owner.

**A method per control on the trait.** More typing, and it moves the "which controls exist"
question out of the central table and into a trait definition, where §52.3 cannot check it.

**A scan that requires every `libc::` call to be wrapped in `checked(…)`.** Simpler to write and
wrong in the other direction: it would fire on `libc::mmap` bound to a name and inspected, on
`libc::fcntl` whose `-1` means "not open" rather than an error, and on every future call that
legitimately reads its return some other way. Reporting the discard is the rule §16.2 actually
states.
