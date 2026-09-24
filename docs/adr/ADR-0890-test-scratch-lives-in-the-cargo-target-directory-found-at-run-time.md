# ADR-0890: Test scratch lives in the cargo target directory, found at run time

- Status: accepted
- Date: 2026-09-24
- Spec refs: v0.6 §15 (file recovery provider), Appendix B.7 (volatile filesystems); v0.4.1 §38.1
- Issues: #143
- Decided by: agent (autonomous)

## Context

`ono_testkit::scratch()` read `CARGO_TARGET_TMPDIR` from the process environment and fell back to
`std::env::temp_dir()`. Cargo sets that variable only while it *compiles* an integration test or
benchmark (`env!`), never in the environment of the running test, and never at all for the
`ono-testkit` library the helper is compiled into. So every call landed in `/tmp` — on the
development machine a quota'd tmpfs. A tmpfs is the volatile filesystem v0.6 §15's file recovery
provider refuses to treat as a persistence domain, so a suite scratching there tests the refusal
instead of the feature; `crates/ono-recovery-files/tests/support` carried its own scratch type,
built from `env!("CARGO_TARGET_TMPDIR")`, to get around it.

The issue suggested a macro, so the constant is expanded in the calling test. `scratch()` has
about 550 call sites in about 100 files, and every one of them would change.

## Decision

**`scratch()` finds the cargo target directory at run time and scratches in `<target>/tmp`, the
directory cargo names `CARGO_TARGET_TMPDIR`.** The rule, in order:

1. `CARGO_TARGET_TMPDIR` from the environment, when a runner sets it;
2. otherwise the nearest ancestor of `std::env::current_exe()` that holds a `CACHEDIR.TAG` — the
   marker cargo writes into every target directory, whatever `CARGO_TARGET_DIR` or `--target`
   made of the path below it — joined with `tmp`;
3. otherwise (a doc test, which rustdoc links in a temporary directory of its own) the
   workspace's own `target/tmp`, derived from the testkit's manifest the way `ono_binary()`
   derives the binary's path.

The system temporary directory is no longer a fallback at all. The call sites keep their shape.

## Consequences

Easy: every suite's scratch is on the filesystem the build is on, which is persistent, and the
per-crate workaround goes (`crates/ono-recovery-files/tests/support` uses the shared helper).

Hard: `<target>/tmp` grows with whatever a crashed test fails to remove, exactly as `/tmp` did;
`cargo clean` now reaches it. A directory tree with a stray `CACHEDIR.TAG` above a test binary
that cargo did not build would be taken for a target directory — cargo is the only thing that
builds these binaries.

`ono_testkit::SocketPopulation` still derives its socket directory the old way; Unix socket paths
are limited to 108 bytes, so moving it deeper into the tree is a separate decision, not part of
this one.

Encoded by `crates/ono-recovery-files/tests/store.rs::should_give_a_suite_scratch_space_on_the_filesystem_cargo_builds_into`,
which runs in a crate other than the testkit and compares against that crate's own compile-time
`CARGO_TARGET_TMPDIR`.

## Alternatives considered

**A `scratch!()` macro expanding `env!("CARGO_TARGET_TMPDIR")` at the call site.** Correct, and a
550-site churn across the workspace for a value that is recoverable at run time; it would also
fail to compile in unit tests (`#[cfg(test)]` inside a library), where cargo does not set the
variable.

**Deriving the target directory from the testkit's `CARGO_MANIFEST_DIR` only.** Wrong under
`CARGO_TARGET_DIR`, which CI and developers set; it is kept as the last resort only.

**Keeping `temp_dir()` as the fallback.** It is the defect: a silent fallback to a tmpfs is what
made the recovery suites test the wrong thing.
