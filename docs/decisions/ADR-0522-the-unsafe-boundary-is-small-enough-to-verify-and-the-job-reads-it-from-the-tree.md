# ADR-0522: The unsafe boundary is small enough to verify, and the job reads it from the tree

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §42.1 (unsafe boundary focus), §42.2 (Miri), §42.3 (sanitizers), §42.4
  (failure handling), §66.5 (green for the release commit), §43.3, §44.3
- Issues: #94
- Decided by: agent (autonomous)

## Context

§42.1 makes an argument rather than a request: unsafe code is *intentionally* concentrated in a
narrow process crate, and v0.4.1 "MUST exploit that architecture with targeted verification"
rather than admire it. The tree bears the premise out — eighteen library crates carry
`#![forbid(unsafe_code)]`, and every `unsafe` block in the workspace lives in eight files across
four crates: `ono-process` (`spawn`, `signals`, `terminal`, `plan`), `ono-kuang-supervisor`
(`platform`, `sandbox`, `report`), `ono-provider-net` (`resolver`, which is `getaddrinfo`), and
`ono-value`.

That is small enough to verify properly and too small to be worth checking by reading.

## Decision

**Two scheduled jobs, because the two tools answer different questions, and the sanitizer job's
crate list is read from the tree rather than typed into a workflow.**

`.github/workflows/verification.yml` runs daily.

**Miri** interprets safe Rust and finds undefined behaviour in what safe code does with memory —
aliasing, provenance, uninitialised reads. §42.2 names the areas: value ownership and sharing,
parser data structures, protocol serialization, and whatever else runs without an unsupported
syscall. It runs `ono-value`, `ono-parser`, `ono-protocol`, `ono-kuang-protocol` and `ono-core`,
with `-Zmiri-strict-provenance` on. §42.2 excuses the process and job-control layer explicitly,
and it has to: Miri implements neither `fork` nor `execve` nor `ioctl`.

**The sanitizers** run the real thing on the real kernel, which is the only way to reach the
syscall wrappers Miri cannot execute. AddressSanitizer and UndefinedBehaviorSanitizer, in a
matrix, over the four crates that hold `unsafe`, with the standard library rebuilt under the
sanitizer so the instrumentation reaches across it.

**The crate list is a gate assertion, not a workflow comment.**
`should_declare_an_address_and_undefined_behaviour_sanitizer_job_for_the_release_commit` walks
`crates/`, finds every file with an `unsafe` block, and requires the sanitizer job to name the
crate it belongs to. §42.1's whole argument is that the boundary is small enough to verify; a
crate that grows an `unsafe` block and is not in the job is a crate outside the argument, and the
gate says so on the commit that adds it rather than a release later.

§42.4 makes a reproducible finding a release blocker, so neither job is allowed to fail and report
green — asserted, because `continue-on-error: true` is one line and a quiet way to turn a release
blocker into a warning.

Both jobs use nightly and say why on the line, under the exception ADR-0521 added to
`check_tool_versions`: neither workflow builds an artifact, so §44.3's rule about the toolchain a
*release* is built by is untouched.

## Consequences

Easy: the four crates that can have a memory-safety defect are the four crates that get looked at,
every night, by the two tools that can see one. §66.5's requirement that the jobs exist and be
green for the release commit is a schedule on the default branch.

Hard, and the honest limit: **neither job has been run.** They are asserted by tests that read the
workflow, not by a run — Miri and `-Z build-std` under a sanitizer are long jobs on a nightly
toolchain, and the first scheduled run is what will say whether the crate list needs narrowing.
Two outcomes are likely and both are useful: Miri may refuse a crate that reaches a syscall
through a dependency, and ASan may report a leak in a test that forks, which is why
`detect_leaks=0` is set already. Narrowing after evidence is the correct order; asserting that the
jobs exist before there is evidence is what §66.5 asks for.

Also hard: `-Z build-std` rebuilds the standard library for each sanitizer, so each job is tens of
minutes. Daily is affordable; per-push would not be, which is why §42.3 says scheduled.

Encoded by: `xtask/tests/supply_chain.rs::should_declare_a_miri_job_covering_every_unsafe_boundary_module`,
`::should_declare_an_address_and_undefined_behaviour_sanitizer_job_for_the_release_commit`.

## Alternatives considered

**Run Miri over the whole workspace.** It cannot: `ono-process` is `fork` and `execve`, and
`ono-provider-linux` reads `/proc`. §42.2 names the subsets for that reason, and a job that fails
every night on unsupported syscalls is a job people stop reading.

**One sanitizer job with both sanitizers.** They cannot be combined in one build, and a matrix
reports which of the two found something without reading a log.

**Put the crate list in the workflow and review it.** It is three lines of YAML and it goes stale
the first time somebody adds an `unsafe` block anywhere else. The gate can read the tree; a
reviewer has to remember to.

**Wait until a run has passed before committing the workflows.** The run needs the workflows to
exist, and §66.5 requires them to exist for the release commit. Committing them with the limit
stated is more honest than holding them back until a green run somebody has to trigger by hand.
