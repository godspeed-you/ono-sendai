# ADR-1082: A file recovery store that cannot be opened is a provider that says why

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.6 §2.3, §2.4, §10.3, §12.1, §12.2, §15.3, §44.3, §55.6 case 29, §56.3; Appendix A.3;
  ADR-0825, ADR-0842
- Issues: #144
- Decided by: agent (autonomous)

## Context

The session registered `ono.recovery.file-copy` only when `FileRecoveryStore::open` succeeded
(`crates/ono-cli/src/change/session.rs`). A store that opens but is unusable — not a directory,
not private, not writable — already registers its provider as `Unavailable` with the reason
(`FileRecoveryStore::unusable_reason`). A store that could not be opened at all — its directory
cannot be created, its path is a dangling symbolic link, it belongs to another user and cannot be
made private (§44.3) — left no provider behind. The coverage matrix then said nothing about the
file provider: a plan over a persistent file read as if no provider existed, and the one fact
that explains the gap, the store's error, was dropped. Nothing unsafe followed (a plan sealed as
protected refuses at `apply`, ADR-0842), but §55.6 case 29 asks that a provider that could not be
asked leaves a refusal behind rather than a silent gap.

A second gap stood behind the first: a compensation note replaced the row's note entirely, so even
a refusal the registry did carry vanished from a row whose mutation has a compensation (`copy
file a b --overwrite`).

## Decision

1. **The file provider is always registered.** Where the store cannot be opened, the session
   registers it over `FileRecoveryStore::unopened(root, error)`: a store that never touched the
   filesystem and whose `unusable_reason` is the open error. Its `availability()` is therefore
   `Unavailable` with that sentence, and `ProviderRegistry::unavailable()` turns it into a
   `ProviderRefusal` the coverage analysis carries, exactly as an unavailable ZFS or Btrfs provider
   already is.
2. **An unavailable provider never acts.** It is never asked to discover (the registry asks only
   available providers), so it never offers a candidate and never promises recoverability. The
   provider itself refuses `create`, `restore`, `cleanup` and `validate` with
   `recovery.provider_unavailable` while its store is unopened, and the executor refuses to create
   an asset through any provider whose availability is not `Available`, rather than calling it and
   reading the failure afterwards. A plan sealed as protected therefore still refuses at `apply`
   with `change.prepare_failed`, and its target is unchanged (§2.3, ADR-0825, ADR-0842).
3. **A row keeps the reason it is not protected.** Where discovery was inconclusive — some
   provider could not be asked — and the mutation has a compensation, the row's note is the
   compensation *and* who could not be asked: "compensated by …, which is not a rollback (§27.4);
   3 providers could not be asked about …: … ono.recovery.file-copy (the recovery store `…` could
   not be created: …)". Where discovery was conclusive the note is the compensation alone, as
   before.

## Consequences

- Reproduction of #144 (a scratch `XDG_DATA_HOME` whose `ono/change/recovery` is a dangling
  symbolic link, then `plan copy file a b --overwrite | to json`): the filesystem row now names
  `ono.recovery.file-copy` and the store's error; the plan is `unprotected`, never `protected`.
- Tests: `crates/ono-cli/tests/change_prepare_failure.rs::should_name_the_recovery_store_when_it_cannot_be_opened`,
  `…::should_refuse_to_mutate_when_the_recovery_store_cannot_be_opened_after_the_seal` (the
  ADR-0842 path for a store that cannot be opened, which had no test), and the existing
  `should_refuse_to_mutate_when_the_protection_the_plan_was_sealed_with_cannot_be_created`;
  `crates/ono-recovery-files/tests/store.rs::should_be_unavailable_with_the_open_error_when_the_store_could_not_be_opened`.

## Alternatives considered

- Keep the provider unregistered and add a synthetic refusal to the matrix — a second
  representation of "a provider that cannot run here" beside `ProviderAvailability`.
- Fail the session when the store cannot be opened — a shell that cannot protect files can still
  plan, inspect and apply what needs no file protection, and §17.1's `prefer` is designed for it.
