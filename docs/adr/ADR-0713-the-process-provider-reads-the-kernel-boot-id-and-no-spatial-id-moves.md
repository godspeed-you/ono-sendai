# ADR-0713: The process provider reads the kernel boot id, and no SpatialId moves

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §5.2, §22.1, §25.4, §25.5, §26.1; v0.4 §2.16, §2.17, §10.2, §42.1, §42.2
- Decided by: agent (autonomous)

## Context

`ono_spatial_core::BootIdentity` is built from a host name and a boot string, and its own
documentation names the source: "on Linux, `/proc/sys/kernel/random/boot_id`. Reading it is the
caller's job." v0.5 §25.5 makes that reading normative — "`boot_id` or equivalent MUST separate
clock domains" — and §5.2 adds the consequence: "Historical references MUST NOT accidentally
resolve a dead process to a later process reusing the same PID."

Two facts about the state of the tree had to be established before deciding anything, because the
decision turns on them.

**First: the boot id is already read, in one place.** `ono_cli::spatial::find::local_scope`
(`crates/ono-cli/src/spatial/find.rs:497`) reads `/proc/sys/kernel/random/boot_id` and builds the
`SpatialScope` every local observation is projected into. `Projection::boot()` takes the boot
identity from that scope, and `Projection::identity_of` puts it into the digest of every process
identity. So a process `SpatialId` on a real machine has been keyed on the kernel boot id since
v0.4. The path is hard-coded and absolute, which is why no fixture, no `--root` proc tree and no
container namespace can influence it, and why the *provider* — the thing that actually reads
`/proc` — had no access to it at all.

**Second: `btime` is in the digest, indirectly.** `procfs::boot_time_seconds` reads `/proc/stat`'s
`btime`, `ProcessProvider::started` turns it into the process's wall-clock start instant, and
`identity_of` uses that instant as the `start_time` component. `btime` is a wall-clock second, so
§25.4's clock jump moves it.

## Decision

### 1. The provider reads the boot id, from the `/proc` it was pointed at

`procfs::boot_id(proc_root)` reads `<proc_root>/sys/kernel/random/boot_id`, trims it, and answers
`None` for an empty or unreadable file — a container with a restricted `/proc`, a fixture that
declares none, a kernel that does not publish it. `ProcessProvider` reads it once at construction
beside `btime`, exposes it as `boot_id()` beside a new `boot_time_seconds()`, and takes it from
`with_boot_id(..)` where a test states it.

### 2. The precedence lives in `BootIdentity`, not in the provider

```rust
BootIdentity::of(host, boot_id, boot_time_seconds)
```

Kernel boot id first, boot second as the fallback, `unknown_boot` where neither is available — the
meaning `unknown_boot` already had, kept. The provider gathers the facts and the spatial layer
composes them, which is what both crates' documentation already says of themselves (§2.16), and it
means the fallback rule exists once rather than once per caller.

`ono-provider-linux` therefore gains **no** dependency on `ono-spatial-core`; the tests that assert
the composition take it as a dev-dependency. No provider crate in this tree depends on the spatial
layer and this change does not start.

### 3. The fallback is visible, and says which fact it used

`BootIdentity::from_boot_time` renders `<host>/btime:<seconds>` rather than `<host>/<seconds>`, and
`BootIdentity::evidence()` answers `KernelBootId`, `BootTime` or `Unknown`. §25.5 accepts
"`boot_id` **or equivalent**", and the two Linux offers are not equivalent to each other: a boot id
is random, minted once and untouched for the whole boot; `btime` is arithmetic on the wall clock
and moves when the clock is corrected. Something that has to decide whether two observations share
a clock domain needs to know which of the two is keeping them apart, and a silent fallback would
make a weaker identity indistinguishable from a stronger one. `is_known()` keeps its meaning and is
now expressed as `evidence() != Unknown`.

### 4. No `SpatialId` moves

This is the decision the brief asked to be taken deliberately, and the answer is that it does not
have to move.

The digest input `identity_of` uses for `boot` comes from the scope, and on a real Linux machine
that scope has carried the kernel boot id since v0.4 — `local_scope` was already reading the file
this ADR adds a provider-side reader for. Adding the reader to the provider changes what the
*provider* can report; it changes nothing about what the spatial layer digests, because the spatial
layer never asked the provider. Every process `SpatialId` this tree has ever computed is the same
today as it was yesterday, and no test's expectations changed.

The honest answer would have been the opposite if the boot component had been `unknown` or `btime`
in production. It was not.

## Consequences

- Two same-pid processes on either side of a reboot are two identities, and the test that proves it
  drives the provider over two fixtures whose `/proc` publishes two different boot ids —
  `should_give_two_processes_with_one_pid_two_identities_across_a_reboot`. It is a real test rather
  than a tautology: with the boot id unread, both fixtures fall back to the same `FIXTURE_BOOT_TIME`
  and the two identities collide.
- `ProcFixture::new()` now writes `/proc/sys/kernel/random/boot_id`, as a Linux host does.
  `ProcFixture::without_boot_id()` is the restricted-`/proc` case and `ProcFixture::booted_as(..)`
  is the reboot. No existing test's expectations changed: a boot id is not a field of
  `ono.process/1` and nothing else in the fixture moved.
- **A defect this work found and did not fix.** `started` — half of a process's identity — is
  `btime + starttime/HZ`, and `btime` is a wall-clock second. A backward NTP step changes `btime`,
  so a *running* process observed by a shell started after the correction digests to a different
  `SpatialId` than the same process observed before it. §25.4 requires a detected backward jump to
  produce a diagnostic coverage annotation and forbids reordering inside a source sequence; here it
  silently splits one object into two. Fixing it means keying the identity on the tick count rather
  than the derived instant, which moves every process `SpatialId` in the tree, so it is its own
  increment with its own ADR and its own list of changed tests. The boot id read here is a
  prerequisite for that fix, not the fix.
- The remaining seam is `local_scope`. It reads the boot id from an absolute path in `ono-cli`,
  which means a historical or containerised projection cannot be given a different one. Routing it
  through `ProcessProvider::boot_id()` belongs to whoever owns `ono-cli`'s spatial session, and it
  would not move a `SpatialId` on a real machine because both reads return the same value.

## Alternatives considered

- **Putting the boot id into the digest as a new component.** It is already there, through the
  scope. A second `boot_id` component would change every process id for no gain.
- **Making the provider return a `BootIdentity` directly.** Requires `ono-provider-linux` to depend
  on `ono-spatial-core`, an edge no provider crate has, to save the caller one function call.
- **Falling back to `btime` silently, rendering it like a boot id.** Then `evidence()` could not
  exist and a clock-domain decision would rest on a fact whose weakness is invisible — the exact
  shape §2.17 calls out.
- **Refusing to build an identity at all without a boot id.** `unknown_boot` already means "this
  host cannot say", and Tier C is the honest degradation. Refusing would make Ono unusable in a
  container with a restricted `/proc`, which §22.8's "must remain useful on a normal Linux account"
  rules out.
