# ADR-0964: The root place learns its kernel from a provider

- Status: accepted
- Date: 2026-11-09
- Spec refs: spec §8.1 (`kernel` target), §35.3; v0.4 §2.16, §2.17, §7.1; ADR-0142, ADR-0553
- Decided by: agent (autonomous)

## Context

v0.4 §7.1 gives the root place — the `SystemPlace` a session stands in before it has moved — the
fields `os`, `kernel` and `uptime`. ADR-0142 declared them and left them null: no provider
answered for an operating system, a kernel release or an uptime, and §2.16 forbids the spatial
layer from reading `/proc` itself and becoming an undocumented source of system truth. `look
--all` at the root has shown three nulls since (#207).

Spec §8.1 lists `kernel` among the system targets — "the running kernel, its version and its
parameters" — and the target registry carried it as `planned` with no schema. The facts are a
few kernel files away: `/proc/sys/kernel/osrelease`, `version` and `hostname`, `/proc/stat`'s
`btime`, `/proc/uptime`, `/proc/sys/kernel/random/boot_id`, and `os-release(5)` for the system
the kernel boots. The process provider already reads three of them for its own identity.

## Decision

**A kernel provider, `linux.kernel`, serves the `kernel` target** with one `ono.kernel/1` record
per host: `release`, `version`, `architecture` (from `uname(2)`), `hostname`, `os` and `os_id`
(from `os-release(5)`, `/etc` before `/usr/lib`), `boot_id`, `booted` and `uptime`. `get kernel`
is its command (`docs/contracts/commands/system.yaml`, capability `kernel.read`, acquisition
local and cheap). A field whose file is unreadable is null; a `/proc` with no `osrelease` makes
the provider unavailable rather than a record with nothing in it.

The record's identity is the boot id — the kernel mints it once per boot and the wall clock
cannot move it (v0.5 §25.5) — with the release as ADR-0553's fallback where a restricted
`/proc` withholds the boot id.

**The root place fills `os`, `kernel` and `uptime` from that record.** `look --all` at the root
asks the providers for `kernel` and hands the record to the system record, which takes `os`,
`release` and `uptime` from it. Where no record comes back — the provider unavailable, or the
view a reconstruction of the past, where today's kernel is not evidence about the coordinate —
the three fields stay null, which §2.17 and §35.3 make the honest answer. The spatial layer
reads no kernel file; it asks, as §2.16 requires.

## Consequences

Easy: `get kernel | select release os uptime` answers what `uname -r`, `/etc/os-release` and
`uptime` do, as one typed record; the root place's description is finally about the machine; a
remote session standing on a linked host asks that host's kernel provider and shows that host's
kernel, with nothing read locally.

Hard: the target registry moves `kernel` from `planned` to delivered, which is a contract change
the reference pages and the conformance suites regenerate from. `architecture` comes from
`uname(2)` of the running machine even when the provider is rooted at a fixture, because the
kernel publishes no file for it; a fixture test does not assert it.

Encoded by `crates/ono-provider-linux/tests/kernel.rs`, the generated provider and command
conformance entries for `linux.kernel` and `ono.kernel.get`,
`crates/ono-cli/tests/spatial_identity.rs::should_describe_the_system_from_the_kernel_provider_at_the_root`
and acceptance case 097.

## Alternatives considered

**Read `/proc/sys/kernel/osrelease` in the spatial layer** — rejected by ADR-0142 already:
§2.16 forbids it, and a display field is not identity, which is the one exception ADR-0141 made
for the hostname.

**A `get system` producer composing host, kernel and uptime** — the spec's target is `kernel`
(§8.1), and `ono.system/1` is a *place*, composed by the spatial layer from what providers say
(ADR-0142). A second system-shaped record would be the same composition under another name.

**Put the fields on `ono.process/1`'s provider**, which already reads `btime` and `uptime` — a
process record is not where a user looks for the kernel release, and `linux.procfs` declares
processes; one provider, one subject.
