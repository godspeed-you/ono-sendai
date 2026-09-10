# ADR-0847: A file target records the storage object that holds it

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §7.1, §7.2, §7.3, §13.4, §14.3, Appendix B.1, Appendix B.8, Appendix B.9,
  Appendix B.10; ADR-0807
- Decided by: agent (autonomous)

## Context

§7.1 has a file target record the persistence domain that actually holds its state, and §7.2
revalidates it at `apply`. The shell resolved that domain from the mount table alone. For ZFS the
mount table is enough, because every dataset is a mount of its own (Appendix B.8). For Btrfs it is
not: a nested subvolume that was never mounted separately is invisible in `mountinfo`, so a file in
`@var/lib-app` recorded the mount's `subvol=` option, exactly as a file beside it did. Cases 290 and
319 found this against a real filesystem. Only the Btrfs provider can name the subvolume, through
`btrfs subvolume show` (Appendix B.9), and ADR-0807 already hands a provider's own resolution to
its discovery. What was missing was the same resolution for the domain the target records, for the
domain `apply` checks it against, and for `inspect plan --resolution`.

## Decision

1. `ProviderRegistry::resolve_at(path, fallback)` answers the persistence domain of a path: the
   resolution of the first available provider that maps the path to a storage object of its own,
   and `fallback` — Appendix B's reading of the mount table — where none does. Its rules are
   `discover_at`'s, without the discovery. A path the core refused is offered to no provider. A
   provider that answers "not mine" or could not establish the domain changes nothing (§56.3). A
   resolution made through another mount than the fallback's is set aside.
2. A provider whose resolution names the path itself as its object changes nothing either. A copy
   provider protects a file by copying it, and its answer says how it would protect the path, not
   what holds the path's state (Appendix B.1). The ZFS dataset and the Btrfs subvolume are storage
   objects distinct from the path, and they are what a file target records.
3. The shell's freeze, its §7.2 revalidation of the `persistence-domain` and `identity`
   preconditions, and `inspect plan --resolution` all go through one function
   (`change::world::file_domain`), so the domain a plan recorded is checked at `apply` against the
   same reading.

## Consequences

A file in a nested Btrfs subvolume records that subvolume, and two targets on either side of a
subvolume boundary record two domains (§14.3). A file on ext4, or on a filesystem no storage
provider maps, records the mount table's reading exactly as before. When the provider that made the
reading is unavailable at `apply`, the fallback differs, and §7.3 reports drift rather than
checking against a weaker reading. The tests are
`crates/ono-change-protection/tests/provider_resolution.rs` (a provider's own object, a copy
provider's answer, a failed resolution) and cases 290 and 319 against a real filesystem.

## Alternatives considered

Detecting nested subvolumes in the core resolver. Rejected: the kernel's mount table does not
show them, and Appendix B.8 forbids inferring storage identity from a path's shape. Letting the
first provider that answers win. Rejected: the copy provider answers for every persistent path, and
the recorded domain of every plan would have become a statement about the copy rather than about
the storage.
