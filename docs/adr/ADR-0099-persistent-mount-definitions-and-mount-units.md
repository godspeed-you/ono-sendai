# ADR-0099: Persistent mount definitions live in `/etc/fstab`; `start`/`stop mount` are systemd mount-unit jobs

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §9.1 (Storage), §11.5, §16.5, §23.3, §23.5, §43, §50, §52; ADR-0006,
  ADR-0012, ADR-0068, ADR-0098
- Decided by: agent (autonomous)

## Context

`storage.yaml` declares five more mutations on `mount.manage`: `set mount` (remount),
`add mount` and `remove mount` ("a persistent mount definition"), `start mount` and `stop
mount` ("activate/deactivate a mount definition"). All five were `phase: planned`; the last two
were `stability: planned` with `validation_required: true`, because spec §52 marks the
`mount/start` and `mount/stop` cells as duplicates of `mount`/`unmount filesystem` whose
usefulness is to be validated before either spelling is stable.

The narrative spec never says where a persistent definition is kept, and it offers two real
candidates: `/etc/fstab`, which every Linux has and systemd's generator reads, and a systemd
`.mount` unit file, which only systemd reads. It also never says what "activate a definition"
is when the machine has no service manager.

## Decision

### 1. A persistent definition is a line of `/etc/fstab`

`add mount <source> <target> [--type T] [--option O]…` appends one `fstab(5)` line —
`source target type options 0 0`, spaces escaped as `\040` the way `getmntent(3)` decodes
them — and refuses with `io.already_exists` (E0303) when the table already defines that
target. The type is `--type`, else the type udev recorded for a block-device source
(ADR-0097 §3), else `auto`; the options are the `--option` list joined, else `defaults`.

`remove mount <target>` drops the line(s) defining that target and rewrites the table whole,
staged beside it and renamed into place, so no reader sees half a table; comments and every
other line stay byte for byte. A target the table does not define is `io.not_found` (E0301).

Both write a root-owned file, so unprivileged they are one `failed` row with E0302 and the
help "`/etc/fstab` is root's". Nothing is written before the refusal.

`fstab` is chosen over unit files because it is the one place every mount tool — the kernel's
`mount -a`, systemd's `fstab-generator`, an initramfs, a rescue shell — reads; a `.mount` unit
would be invisible to all but one of them.

### 2. A defined mount is an object

`StorageProvider::resolve` answers from the kernel's mount table *and* from `/etc/fstab`, one
object per target. A mount that is defined but not active resolves, so `start mount
/mnt/data`, `remove mount /mnt/data` and `unmount filesystem /mnt/data` all name it; the last
still answers E0301 because the kernel has nothing there. `get mount` is unchanged: it lists
what is mounted.

### 3. `set mount` is `mount(2)` with `MS_REMOUNT`

The `--option` list and `--read-only` become flags and data exactly as in ADR-0098 §2;
`--read-only false` clears `MS_RDONLY`. The kernel's answer is the row.

### 4. `start`/`stop mount` queue the mount unit's job through systemd

The unit is the mount point escaped as `systemd-escape --path --suffix=mount` does —
`/` is `-.mount`, `/mnt/data` is `mnt-data.mount`, other bytes `\xNN` (systemd.unit(5)) —
and the job is `StartUnit`/`StopUnit` over D-Bus (spec §23.3), reusing `ono-provider-systemd`'s
`SystemdBus`. No service manager → `provider.unavailable` (E0401); polkit's refusal → E0302;
an unknown unit → E0301. No "already active" pre-check: the service manager decides, and its
answer for an active root mount is the refusal the user should see.

The storage provider therefore depends on `ono-provider-systemd`. A fixture-backed provider
takes any `SystemdBus` through `StorageProvider::with_units`.

### 5. Phases and stability

All five commands move to `phase: C`. `set`/`add`/`remove mount` stay `experimental`.
`start`/`stop mount` become `experimental` with `validation_required: true` kept and the §52
note intact: the behaviour is delivered; the question whether two spellings should survive is
still owed to §52's validation, and an experimental id is what a spelling under review is.

## Consequences

- `add`/`remove mount` are idempotence-aware: E0303 on a duplicate, E0301 on an absent
  definition, rather than a second line or a silent no-op.
- `start mount` on a machine whose definition is in `fstab` works because
  `systemd-fstab-generator` has already made the unit; on a machine without systemd it is
  E0401, which is the honest answer, not a fallback `mount(2)` that would ignore the table.
- Tests: `crates/ono-cli/tests/storage_missing.rs` (`set`/`add`/`remove`/`start`/`stop mount`,
  un-ignored); `crates/ono-provider-linux/tests/storage.rs` (fstab append/remove against a
  fixture table, resolving a defined-but-unmounted target, mount-unit jobs against a recorded
  service manager); unit tests for the `fstab(5)` decoder and the unit-name escaping.

## Alternatives considered

- **systemd `.mount` unit files under `/etc/systemd/system`.** Invisible to `mount -a`, the
  initramfs and non-systemd hosts; and `add mount` would need `daemon-reload`. Rejected.
- **`start mount` as `mount(2)` from the fstab entry.** Would bypass the unit's dependencies
  and ordering on a systemd host and fabricate a service-manager-free activation path the
  spec (§23.3) does not want. Rejected; where systemd is absent, E0401.
- **Withdraw `start`/`stop mount` now, as the §52 note suggests.** Withdrawing a declared
  command is a contract decision the RED suite pins the other way; delivering it as
  experimental keeps both options open. Rejected for now.
- **Resolving definitions in `get mount`.** `get mount` says what is mounted; a defined mount
  with `target` set would claim otherwise. Rejected; only `resolve` sees definitions.
