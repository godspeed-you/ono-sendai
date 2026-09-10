# ADR-0805: A provider runs a program through a seam, and the seam replays real tool output

- Status: accepted — corrected by ADR-0833
- Date: 2026-09-09
- Spec refs: v0.6 §2.17, §12.3, §43.6, §54.4, Appendix G.2, Appendix G.3
- Decided by: agent (autonomous)

## Context

§12.3 says a recovery provider "MUST use direct process APIs, libraries, DBus, ioctls or other
structured interfaces" and "MUST NOT generate shell command strings from user-controlled values".
§43.6 adds that provider-generated asset names must be sanitised and must not contain user-
controlled command syntax. §2.17 states the same rule for the whole model.

There is no ZFS or Btrfs library binding in the workspace and none worth adding: both projects'
stable interface is their command-line tool with `-H`/`-p` machine output, and both are already
parsed that way by every tool that integrates with them. So "direct process APIs" here means
`execve` with an argument vector, and the risk §12.3 is guarding against is a provider that builds
one string and hands it to a shell.

§54.4 asks separately for real ZFS and Btrfs where the environment permits, and for mock providers
to remain useful for deterministic lifecycle testing. Appendix G.2 asks for truth tests over
deliberately misleading layouts. A provider tested only against fixtures somebody wrote by hand has
been tested against that person's belief about what `zfs list` prints.

## Decision

**One seam, `ono_change_core::ToolRunner`, with two implementations and one recording.**

```rust
fn run(&self, program: &str, argv: &[&str]) -> Result<ToolOutput, ErrorValue>;
fn is_available(&self, program: &str) -> bool;
```

There is no method taking a command line and no variant of `Execution` holding one, so §12.3 is a
property of the type. A dataset name containing `; rm -rf /` is one element of `argv` and arrives
at `execve` as one argument.

`ProcessRunner` is the real implementation: `std::process::Command` with a bounded wait and no
shell. `ScriptedRunner` is the test double, and the decision that matters is what it replays.

**It replays recorded output of the real tools.** `scripts/fs-fixtures.sh` builds a ZFS pool and a
Btrfs filesystem from nothing inside a disposable privileged container, exercises them, and records
the verbatim stdout, stderr and exit status of every command into
`crates/ono-recovery-{zfs,btrfs}/tests/fixtures/`. Fifty-eight files, from OpenZFS 2.4.1 and
btrfs-progs 6.16. The deterministic suite the ordinary gate runs parses those bytes; the gated
real-filesystem suite runs the same code against a live filesystem.

Two of the recordings are the reason this is worth the machinery:

- `zfs/rollback-refused.txt` is what ZFS says when a rollback would have to destroy newer history —
  "more recent snapshots or bookmarks exist", followed by their names. §13.6 forbids Ono adding the
  flag that stops it saying that, and the provider parses the refusal rather than guessing at it.
- `btrfs/nested-live.txt` and `btrfs/nested-in-snapshot.txt` are the same path inside and outside a
  read-only snapshot of its parent subvolume. The live one has contents; the one in the snapshot is
  **empty**. That is §14.3 demonstrated rather than asserted, and it is the fact the whole Btrfs
  provider exists to get right.

Appendix G.3's rule holds for the real suites: they build their own filesystem on a loop file and
refuse to run against anything they did not create.

## Consequences

The ordinary gate stays fast and hermetic — no root, no loop devices, no kernel modules — while
testing the real parsing against real bytes. A tool version whose output changes shape is caught by
regenerating the fixtures and watching tests fail, which is the useful failure.

`ScriptedRunner::calls()` records the argument vectors, which lets §43.6 be tested directly: a test
asserts that a hostile snapshot name arrives as one argument, and another asserts that no argument
vector the ZFS provider ever produces contains `-R`.

The fixtures are a recording of one version pair. Appendix G.4's version variance is handled
separately: each provider declares the versions it validated against and degrades to `Unsupported`
rather than executing semantics it has not tested.

## Alternatives considered

**Link `libzfs`.** Rejected: it is not a stable ABI, it is not present on a machine that has only
`zfsutils-linux`, and it would make the provider unbuildable where ZFS is absent.

**Hand-write the fixtures.** Rejected for the reason above, and because the Btrfs nested-subvolume
case is exactly the one a hand-written fixture gets wrong in the direction that loses data.

**Only run against real filesystems.** Rejected: §54.4 asks for both, the acceptance container
runs unprivileged with networking disabled, and a suite that cannot run in the ordinary gate is a
suite that stops being run.
