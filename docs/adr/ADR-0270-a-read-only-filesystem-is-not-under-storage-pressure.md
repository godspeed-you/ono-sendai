# ADR-0270: A read-only filesystem is not under storage pressure

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.4 §26.2, §26.3, §2.11, §35.3
- Decided by: agent (autonomous, `close-spat`)

## Context

§26.2's storage rule promotes a filesystem "near capacity", and the engine implemented it as
`used / size >= 90%`. On any Ubuntu host with snaps that promotes twenty squashfs images at once:
`enter storage; look` listed twenty landmarks, every one of them at `100% used`, none of them a
reason to look. §2.11 makes a landmark "a reason to attend to a place"; twenty of them that are
never actionable is the alert board §26.3 exists to prevent.

## Decision

**The storage-pressure rule does not fire for a filesystem the provider reports as
`read_only: true`.** A read-only image is full by construction — a squashfs snap is 100% used the
moment it is mounted — so "near capacity" is not a fact about it: nothing can fill it and nothing
can be freed from it.

**Only an explicit `true` suppresses the rule.** §35.3 makes unknown `null`, and `null` is not a
claim that the filesystem is read-only. A provider that does not answer the question leaves the
rule exactly as it was.

The field is already in the contract and already served: `ono.filesystem/1` and `ono.mount/1` both
declare `read_only`, and `ono-provider-linux` fills it from `/proc/self/mountinfo`.

## Consequences

- The `storage_pressure` entry of `docs/contracts/spatial/landmarks.yaml` records the guard, so the
  contract and the engine say the same thing.
- A read-only filesystem that is genuinely a problem — a root filesystem remounted read-only after
  an I/O error — is still surfaced, as §26.2 says it should be: as a *change* of state, through
  `recently_changed`, not as storage pressure.
- Encoded by `ono-spatial-query/tests/landmarks.rs::should_not_promote_a_read_only_filesystem_that_is_full_as_storage_pressure`,
  `::should_still_promote_a_writable_filesystem_above_the_threshold` and
  `::should_still_promote_a_full_filesystem_that_does_not_say_whether_it_is_writable`.

## Alternatives considered

- **Raising the threshold** — would hide a real full disk to hide twenty images that are not one.
- **Excluding by filesystem type (`squashfs`, `iso9660`)** — a list that is wrong the moment a new
  read-only filesystem exists. `read_only` is the property the rule actually depends on.
