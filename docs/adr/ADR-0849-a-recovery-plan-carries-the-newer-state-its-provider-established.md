# ADR-0849: A recovery plan carries the newer state its provider established

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §13.6, §24.3, §24.5, §56.1, §56.3, §62.8, Appendix C.3, Appendix C.4; ADR-0846
- Decided by: agent (autonomous)

## Context

A recovery plan's newer-state analysis (Appendix C.3) decides what §24.5's gate asks for. The
builder produced it from one source only: the caller's observation function, asked about every
object in the candidate restore scope. That scope includes what the asset covers, and a ZFS
snapshot covers its dataset by name. The shell observes paths, so the dataset came back
`Unknown`, and §56.3's gate blocked every dataset rollback as unestablished. §13.6's acceptance —
the refusal naming the newer snapshots a rollback would destroy — was therefore unreachable, and
the `recovery.destroyed-history` risk finding never fired.

The facts the gate needed were there. The ZFS provider examines its dataset before it offers a
rollback and states, in its fragment's newer-state impact, the snapshots, bookmarks and clones the
rollback would destroy and the bytes written since the snapshot, each of §56.1's twelve facts
fail-closed. The builder never read that impact. Case 318 found it against a real pool.

## Decision

1. Where the chosen fragment carries a completed newer-state impact, the builder takes from it the
   provider-native objects the recovery would destroy and the bytes it would discard, beside any
   the caller named. The risk assessment counts the same objects.
2. A scope entry that is not a path — a dataset, a subvolume reference — is a storage object. When
   the chosen provider established the newer state of its storage, the observer is not asked about
   such an entry, and the provider's items about it are carried in the plan.
3. Every object the observer was asked about stays the observer's to classify, whatever it
   concluded. It reads the object as it is now and knows which write was the plan's own
   (Appendix C.4); a provider comparing the object against its asset does not, and its reading of
   that object is not added.
4. A provider that established nothing changes nothing: every scope entry is observed, and what the
   observer cannot answer blocks as §56.3 requires.

## Consequences

A dataset rollback that would destroy newer history reaches §24.5's gate with every fact
established and is refused with `recovery.destructive_history_not_accepted` until
`--accept-newer-state-loss` is given (§13.6). File recovery is unchanged: its scope is paths, and
the observer classifies them as before. The tests are
`crates/ono-change-recovery/tests/builder.rs::should_carry_the_newer_state_a_provider_established_for_a_whole_dataset_rollback`,
the existing `crates/ono-cli/tests/change_recovery.rs` (a first version that merged the file
provider's items for observed paths refused a legitimate recovery there), and case 318 against a
real pool.

## Alternatives considered

Teaching the shell's observer to read datasets. Rejected: it would duplicate the provider's §56.1
examination in the shell, with a second reading that could disagree with the one the provider acts
on. Dropping non-path entries from the scope unconditionally. Rejected: a provider that established
nothing would then leave its storage objects unexamined, and §56.3 requires that to block.
