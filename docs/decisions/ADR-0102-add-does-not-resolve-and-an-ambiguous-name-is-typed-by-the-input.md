# ADR-0102: `add` acts on an unresolved name, and a name that resolves to several kinds is narrowed by the command's input type

- Status: accepted
- Date: 2026-08-27
- Spec refs: §6.1, §7.1, §16.5, §23.6; ADR-0068, ADR-0101
- Decided by: agent (autonomous)

## Context

`ProviderMutation` (ADR-0068) turns a selector into the objects to act on by asking the
target's provider to `resolve` it, so that a signal reaches `(pid, started)` and not a bare
pid, and a selector nothing answers to becomes one `io.not_found` row. Two commands of the
identity family did not fit that seam:

1. **`add user deploy`** creates `deploy`. Resolving the name first can only answer "no user
   answers to name deploy" — the honest not-found row for `remove`, and the wrong answer for
   every `add`. Spec §7.1 gives `add` the sense "create a membership or association"; the thing
   named is what the command brings into being or extends, not something that must already be
   there.
2. **`remove user root`** resolves `name == root` through `linux.nss`, which answers with two
   objects: the user `root` (`ono.user/1[0]`) and the group `root` (`ono.group/1[0]`). One
   selector, two kinds, and `remove user` would have written two rows and asked the provider to
   delete a group under the `user` target.

## Decision

### 1. `add` does not resolve its selector

For a contract whose verb is `add`, the selector's value travels unresolved as the identity of
an `ono.<target>/1` object — the same spelling the not-found row of ADR-0068 §2 already uses
for a name that resolved to nothing. The provider's `act` receives the name and decides what
`add` means for its target: create the account, or — with `--member` — extend the group
(`identity.yaml` documents both; ADR-0101 implements them). A provider that needs the object
to exist already answers `io.not_found` itself, per target, as any other failure.

Every other mutating verb keeps resolving first; `remove user nobody-such` stays the E0301 row
naming the account.

### 2. A resolution spanning several kinds is narrowed by the input type

When the resolved objects are of more than one schema, the ones kept are those of the schema
the contract's input type declares (`null | stream<ono.user/1>` → `ono.user/1`). When they are
all of one kind, nothing is filtered: a provider that answers a target with one kind of object
— including one whose schema is not the contract's, as a test fixture may — is never
second-guessed. The narrowing exists for the case where one name honestly means two things,
and only for that case.

## Consequences

- `add user`, `add group`, `add group --member` reach their provider with the name as written;
  `remove user root` and `set user root` act on the user alone.
- No provider changed for this: `linux.nss`'s `resolve` still answers a name with every kind
  that carries it, which `enter`/`trace` rely on.
- The `add` verbs of other families (`add route`, `add mount`, `add interface`, `add link`,
  `add host`) are not bound to a provider today, so their behaviour does not change; when one
  is delivered, its provider receives the unresolved name and applies its own semantics.
- Proven by `crates/ono-cli/tests/identity_missing.rs` (`add user`, `add group`,
  `add group --member`, one row for `remove user root`) and, for the untouched paths,
  `crates/ono-command/tests/mutations.rs`.

## Alternatives considered

- **Resolve `add` and treat not-found as "create".** Turns a provider's inability to see the
  account (an NSS timeout, a directory outage) into a creation attempt. Rejected.
- **Give `resolve` the target name.** A change to the provider trait and the remote protocol
  for a distinction the contract's input type already states. Rejected.
- **Filter by the input type always.** Broke the command crate's fixture-driven mutation tests,
  whose provider answers `service` with its own schema — a legitimate provider design the seam
  must not forbid. Rejected in favour of narrowing only ambiguous resolutions.
