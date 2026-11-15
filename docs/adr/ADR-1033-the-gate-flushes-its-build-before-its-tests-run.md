# ADR-1033: The gate builds its tests, flushes the build's writes, and then runs them

- Status: accepted
- Date: 2026-11-15
- Spec refs: v0.4.1 §38.1 (watchdogs are liveness bounds), §38.3; ADR-0852, ADR-0853, ADR-0889
- Issues: #311 (its qualification)
- Decided by: agent (autonomous)

## Context

`scripts/gate.sh`'s test step ran `cargo test` alone, which compiles and then runs. Several suites
put a liveness watchdog of 20–30 s on each `ono` they start (`ono_testkit::Shell`, scaled by the
load average). On a quiet disk they finish far inside it. `acquisition.rs` takes 11.5 s for all 22
tests, and `authorized_clients.rs` is similar.

During #311's qualification on this host (a Hyper-V VM whose disk reports itself rotational, with
write-back throttling on), three gates stopped in exactly those suites:

- the gate of `7d90f92f` (`acquisition.rs`, six watchdogs);
- the first run of `91706f92` (`acquisition.rs`, three);
- the first run of `d0c41e91` (`authorized_clients.rs`, `add client-key` past 20 s).

Each followed a build. A sampler beside the third recorded the dirty page cache rising to 6 GB at
up to 128 MB/s of writes in the gate's first minutes. Writing 8 GB beside `acquisition.rs`
reproduces it at will. The run took 134 s instead of 11.5 s with three watchdogs, and the shells
were sampled in `D` state on `rq_qos_wait` and `folio_wait_bit_common`: their own writes, and the
`fsync` `add client-key` makes, were queued behind the flush of the build. Twice the CPU load, with
no write load, costs 20 s and passes. The tests were not hanging, and the shell was not slow; the
kernel was flushing what cargo had just written.

That is the normal shape of a gate after a code change, not an accident of one run: every gate of
a changed tree compiles, then tests.

## Decision

**The test step builds the selected tests (`cargo test … --no-run`), runs `sync`, and then runs
them.** The flush happens before the first test, not underneath it. Nothing is skipped, and no
bound changes. The flush would have happened anyway; it now costs its time before the tests
instead of their watchdogs. The second `cargo test` finds everything built and only runs.

## Consequences

Easy: a gate after a change no longer depends on how fast the disk drains the build. A gate of an
unchanged tree pays nothing extra: nothing is built, and `sync` returns at once.

Hard: a gate after a large build now waits for the flush, tens of seconds on this disk, before it
tests. A machine with another project writing heavily at the same time can still slow the tests.
That is beyond what a gate controls, and the watchdog message already says so (v0.4.1 §38.1).

Encoded by the qualification record of #311 (`docs/runs/pty-reliability-hardening-2026-11-15.md`);
a gate script is proven by the runs it makes, as ADR-0852 and ADR-0853 were.

## Alternatives considered

**Longer watchdogs for the suites that write.** Not adopted: the stall is the length of a flush,
which nothing bounds, so a longer bound would only move the failure. A bound that is right on a
quiet disk stays right once the flush is out of the way.

**Scaling the watchdog by I/O wait as it is scaled by load.** Not adopted: it would hide a real
stall in the shell's own I/O as readily as one in the build's. Removing the cause is cleaner.

**`sync` in `release-check.sh` only.** Not adopted: a developer's gate after a change is where the
build happens, and it hit the same suites (`7d90f92f`).
