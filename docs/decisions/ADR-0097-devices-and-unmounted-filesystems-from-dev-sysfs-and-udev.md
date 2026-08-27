# ADR-0097: Devices and unmounted filesystems, from `/dev`, sysfs and udev's database

- Status: accepted
- Date: 2026-08-27
- Spec refs: §8.1, §9.1 (Storage), §10.5, §23.5, §28.6, §35.3, §50; v0.3 §1.35; ADR-0012,
  ADR-0027, ADR-0079
- Decided by: agent (autonomous)

## Context

`docs/spec/commands/storage.yaml` declares `get device` (output `stream<ono.device/1>`,
option `--kind`) and `get filesystem --mounted`, whose doc reads "Restrict to filesystems that
are or are not currently mounted"; `filesystem.v1.yaml` says `target` is "the mount point, or
null when the filesystem is not currently mounted". Neither was built: `ono.device/1` sat in
`deferred.yaml`, and `get filesystem` enumerated `/proc/self/mountinfo` only, so `--mounted
false` could never answer with anything.

Spec §9.1 says only "enumerate filesystems/devices where provider supports it" and §28.6 has a
Mount carry a `ref<ono.device/1>`. What a device *is*, where the list comes from, and how a
filesystem that is not mounted can be seen without parsing `lsblk` (spec §50, AGENTS.md §6) are
open. The v0.3 util-linux adapter pack knows `lsblk --json` as a documented adapter fallback, so
there is a temptation to build the provider on it.

## Decision

### 1. `ono.device/1` is a node under `/dev` with the kernel's numbers

`docs/spec/schemas/device.v1.yaml`: identity `[path]`; fields `path`, `name`, `kind`
(`block` | `char`), `major`, `minor`, `size` (bytesize, block devices only, nullable) and
`subsystem` (nullable). The major/minor pair is the field that matters: it is the same pair
`/proc/self/mountinfo` prints for a mount's backing device, so the reference in `ono.mount/1`
and `ono.filesystem/1` is resolvable by number, not by guessing a path.

### 2. The device provider reads `/dev` and `/sys/dev`, never a program

`linux.sysfs` (`crates/ono-provider-linux/src/device.rs`) walks `/dev` recursively,
`lstat`s every entry, keeps block and character nodes, skips symlinks (so `/dev/disk/by-uuid/…`
does not duplicate `/dev/sda2`) and takes `major`/`minor` from `st_rdev`. Sysfs adds what stat
cannot know: `/sys/dev/{block,char}/<major>:<minor>/size` (512-byte sectors, as the kernel
defines the attribute) and the `subsystem` symlink's target name. Missing sysfs is null in both
fields, not an error (spec §10.5, §35.3). `--kind` is honoured in the provider; anything else is
the pipeline's `where`.

A device sysfs lists but `/dev` has no node for is not a record. `get device` enumerates the
nodes under `/dev`, which is what its contract and its help say; a container with a sparse
`/dev` sees a short list, not an invented one.

### 3. An unmounted filesystem is a block device udev found a signature on

`get filesystem` answers every filesystem the machine can see: the mounted ones from
`/proc/self/mountinfo` + `statvfs(3)` as before, and the unmounted ones from the block devices
`/dev/disk/by-uuid/` and `/dev/disk/by-label/` link to that are not the source of any mount.
Those symlink farms exist because udev probed the device and found a filesystem signature; the
provider already reads them for `uuid` and `label`. Whether a device is mounted is decided by
`major:minor` (the node's `st_rdev` against the mount table's device field), so
`/dev/mapper/x` and `/dev/dm-0` are the same device.

The filesystem `type` of an unmounted device comes from udev's database,
`/run/udev/data/b<major>:<minor>`, line `E:ID_FS_TYPE=…` — the structured key/value store
libudev reads, world-readable, written by the same probe that made the symlink. Where it is
absent the type is unknown, and `type` is `required`, so the device is not reported as a
filesystem: a record with a fabricated type would be worse than no record (spec §35.3).

An unmounted filesystem's `target`, `size`, `used`, `available` and `read_only` are null;
`device` is the node path. `--mounted true` keeps the mounted ones, `--mounted false` the
unmounted ones, no flag keeps both — the summary says "mounted or not".

### 4. `lsblk --json` stays an adapter, not a provider source

The util-linux adapter pack (v0.3 §1.35, ADR-0027) is the way to run `lsblk` and get
`ono.block-device/1` records; it is not the way the storage provider learns about devices.
Spec §50's "explicit adapter fallback" clause covers the user typing `lsblk`, not a core
provider spawning it. The provider reads kernel interfaces and udev's store and nothing else.

## Consequences

- `get device`, `get device --kind char`, `get device | where path == "/dev/null"` answer
  from `/dev`; `/dev/null` is char 1:3 everywhere, which is what the tests pin.
- `get filesystem` may now list more records than before on a machine with an unmounted
  partition; every extra one has `target: null`. Tests that count mounted filesystems filter on
  `target != null` or use `--mounted true`.
- The provider registry gains `linux.sysfs` (`docs/spec/providers/linux-procfs.yaml`);
  `deferred.yaml` loses `ono.device/1`.
- Tests: `crates/ono-cli/tests/storage_missing.rs` (`get device` ×4),
  `crates/ono-cli/tests/options_and_selectors_missing.rs`
  (`should_return_only_unmounted_filesystems_when_mounted_is_false`),
  `crates/ono-provider-linux/tests/storage.rs` (fixture with an unmounted device).

## Alternatives considered

- **`lsblk --json` through the adapter layer as the device source.** Structured, but a core
  provider would then depend on util-linux being installed and on a spawned process for a
  question the kernel answers directly; and `get device` must work in a container that has no
  `lsblk`. Rejected.
- **Probing superblocks in the provider (`libblkid` or a Rust reimplementation).** Reading a
  block device needs privilege an unprivileged shell does not have, and udev has already done
  the probe. Rejected.
- **Enumerating `/sys/dev/block` and `/sys/dev/char` as the device list.** Complete from the
  kernel's side, but it lists devices with no node under `/dev` (and `path` would have to be
  invented from `uevent`'s `DEVNAME`), and the char index has thousands of entries for ttys.
  `/dev` is what the user can open. Rejected as the primary list; sysfs enriches it.
