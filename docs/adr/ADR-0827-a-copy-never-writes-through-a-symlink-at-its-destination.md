# ADR-0827: A copy never writes through a symlink at its destination

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §15.4, §43.5; spec v0.2 §9.1
- Decided by: agent (autonomous)

## Context

`copy file` wrote a regular file with `std::fs::copy`, which opens the destination by path and
follows a symlink there. §43.5: "Protection of one object followed by mutation of a replaced
symlink target is unacceptable." A plan freezes and revalidates the destination's own identity
(file type, device, inode, read without following links); the file a link points at is another
object nobody protected. The window between revalidation and the write remained.

## Decision

The regular-file copy opens its destination with `O_NOFOLLOW` (plus create and truncate) and
carries the permission bits over as `std::fs::copy` did. A link at the final component makes the
copy fail with the filesystem's own error, as a `failed` outcome a script can see. This holds for
`copy file` inside and outside a plan: the provider does not know which it is serving, and the
unsafe case is the same.

## Consequences

Copying onto a symlink is refused; copying onto the link's target by naming it still works. No
existing test or acceptance case relied on following the link. Test:
`crates/ono-cli/tests/files_symlink.rs`.

## Alternatives considered

Write a sibling temporary file and rename it over the destination. Rejected here: it would
silently replace an intended link with a regular file, which is a different change than the one
asked for. The Btrfs provider's restore does use temp-and-rename, because there the destination
is the object being put back.
