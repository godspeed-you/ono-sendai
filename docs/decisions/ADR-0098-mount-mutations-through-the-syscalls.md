# ADR-0098: `mount` and `unmount filesystem` through mount(2) and umount2(2); a creating verb names its object

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §9.1 (Storage), §11.5, §16.5, §23.5, §43, §50; ADR-0006, ADR-0068
- Decided by: agent (autonomous)

## Context

`storage.yaml` declares `mount filesystem <source> <target>` (options `--type`, repeatable
`--option`, `--read-only`) and `unmount filesystem <target>` (`--lazy`, `--force`, input
`null | stream<ono.mount/1>`), both on `mount.manage`. ADR-0068 §3 made a mutating verb reach
a provider's `act` the moment the provider advertises the capability, so the storage provider
can deliver both without the command crate changing — with one exception the mutation seam
did not foresee.

`ProviderMutation` resolves a selector into an object before acting: `stop process 4419` asks
the provider which process that is, so the identity is complete. `mount filesystem tmpfs
/mnt/x` cannot be resolved that way. The filesystem is not mounted yet, `source == tmpfs`
matches nothing, and the seam would answer E0301 `no filesystem answers to source tmpfs` to
every mount request, privileged or not.

Every test of this family runs unprivileged. The observable behaviour when the kernel needs
CAP_SYS_ADMIN is fixed: one `ono.action-result/1` row, `status: failed`,
`Ono-Sendai-E0302` (io.permission_denied), exit 1 — and it has to come from the real syscall,
not from a uid check that pretends to know what the kernel would say.

## Decision

### 1. A creating verb names its object; nothing is resolved

In `crates/ono-command/src/impls/mutate.rs`, a verb whose semantics create what it names —
`add` ("Create a membership or association") and `mount` ("Attach a filesystem or resource"),
`docs/spec/verbs.yaml` — takes its object from the selectors as written, in contract order:
`ObjectId(ono.<target>/1, [source, target])`. The selectors also travel in the `Action` as named
arguments (`source`, `target`), so the provider reads them by name rather than by position.
With no selector written the refusal is the usual "needs something to create". Piped input,
where the contract allows it, is unchanged.

Every other mutating verb keeps resolving, because for them an object that does not exist is
the `failed` E0301 row ADR-0068 §2 fixes — `unmount filesystem /not/a/mount` is exactly that
row, produced before any privileged call because `/proc/self/mountinfo` already says there is
no mount there.

### 2. The storage provider calls the kernel

`linux.mountinfo` advertises `mount.manage` (`Risk::Mutate`, elevation required) and answers:

- **`mount`**: `mount(2)` with the source, the target, the type (`--type`, or the type udev
  recorded for a block-device source when omitted — ADR-0097 §3; neither known is a `failed`
  row asking for `--type`), the flags, and the data string. The option list is kept as the
  list the user wrote, one `--option` per element (spec §23.5); the elements that are kernel
  flags (`ro`, `noexec`, `nosuid`, `nodev`, `noatime`, `relatime`, `bind`, `remount`, …)
  become `MS_*` bits and the rest is joined into the data string for the filesystem.
  `--read-only` is `MS_RDONLY`.
- **`unmount`**: the target is the `target` argument or the identity of the `ono.mount/1` that
  was resolved or piped in. If the kernel's mount table has no mount at that path the row is
  `failed` E0301 with the path as its target; otherwise `umount2(2)` with `MNT_DETACH` for
  `--lazy` and `MNT_FORCE` for `--force`.

The syscall's errno is the row's error through the crate's usual translation: `EPERM`/`EACCES`
→ E0302 with the help line "mounting and unmounting need CAP_SYS_ADMIN", `ENOENT` → E0301,
`ENOTDIR` → E0304, anything else → E0401 retryable. `--dry-run` is a `skipped` row saying what
would be done. Nothing checks the uid first: the kernel's answer is the answer.

## Consequences

- `mount filesystem` and `unmount filesystem` are delivered commands: `help` shows them bound,
  an unprivileged attempt is a structured refusal with exit 1, a privileged one mounts.
- `add <target> …` for every other family also names its object from its selectors from now
  on, which is what "add" means; a family that needs its provider to *resolve* on `add` would
  be contradicting its own verb.
- The row's `target` is `ono.mount/1[/]` (ADR-0068 §2), not the bare path; the tests that
  asserted the bare path were corrected in a `test:` commit.
- Tests: `crates/ono-cli/tests/storage_missing.rs` (`mount filesystem` ×2, `unmount
  filesystem` ×3, un-ignored).

## Alternatives considered

- **Refuse unprivileged mounts before the syscall from `geteuid()`.** Wrong under
  CAP_SYS_ADMIN without uid 0, and under user namespaces; and it would "fake the refusal" the
  tests forbid. Rejected.
- **Shell out to `mount(8)`/`umount(8)`.** Spec §23.3 and §50 forbid parsing their output,
  and their exit status would collapse E0301/E0302/E0304 into one code. Rejected.
- **Let `mount filesystem` resolve `target` against the filesystem records.** The target is
  a directory, not a filesystem; the seam would resolve the wrong thing or nothing. Rejected
  in favour of the creating-verb rule, which `add mount` needs as well.
