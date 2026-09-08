# ADR-0711: The source capability matrix is declared beside the spatial claims

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §21.1, §21.5, §22.1–§22.8, Phase T10; v0.2 §35.3, §36.5, §47; v0.4 §42
- Decided by: agent (autonomous)

## Context

v0.5 Phase T10 names "source capability matrix" among its deliverables, and §21.1 requires
temporal capabilities to be inspectable. `docs/contracts/providers/*.yaml` is where every other
per-provider claim already lives: the targets, the capability ids, the schemas, the `conformance:`
behaviour of a bare snapshot, and v0.4 §42's eight `spatial:` claims. There is no second registry
for provider facts and there should not be.

The matrix is also the place where the tree's honesty about itself is easiest to lose. §21.5 is
blunt about `exhaustive_events` — "Providers MUST NOT advertise it merely because events usually
arrive" — and §22.1 forbids the procfs provider claiming historical process coverage at all. Both
are rules about what a document may say, so the document has to exist for them to bind.

## Decision

Every entry in `docs/contracts/providers/*.yaml` carries a `temporal:` block beside its
`conformance:` and `spatial:` blocks: the seven capability keys of §21.1 verbatim, plus one
`coverage:` word for what the source fundamentally is —

```
snapshot            states what is true now and keeps nothing
event_stream        pushes changes as they happen
historical_query    answers about instants before it was asked
none                cannot be asked about time at all
```

The vocabulary and the rules that bind it are documented in `linux-procfs.yaml`'s header, which is
where the registry already explains `conformance:`.

What the nineteen entries say is the tree as it is, not the sources as they could be:

- **`exhaustive_events` is false everywhere.** Netlink drops messages on receive-buffer overrun,
  inotify's queue overflows, and journald rate-limits. Every one of them "usually" delivers.
- **`live_events` is true for exactly three entries**: netlink's interface and route providers,
  which subscribe to `RTMGRP_LINK`, the address groups and the route groups, and `linux.fs`, which
  watches with inotify. Everything else is polled by the runtime and says so. Notably
  `systemd` says `live_events: false` although §22.2 asks it to contribute live transitions: it
  reads unit state on demand and subscribes to no signal, and a capability the source offers and
  the code does not use is not a capability.
- **`historical_query` is true for exactly one entry**: `systemd-journal`. It is the only source
  here that answers about instants before anything asked.
- **`causal_tokens` is true for exactly one entry**: `systemd`, on the job identity of ADR-0712.
- **`linux.procfs` claims `current_snapshot` and `checkpointable` and nothing else**, which is
  §22.1 written down: process appearance and disappearance are the recorder's to derive from two
  snapshots, with provenance saying `snapshot_diff`.
- **`retained_history` is null everywhere.** journald's retention is `journald.conf`'s to state
  and no provider here reads it; an unknown bound stays unknown rather than becoming "forever"
  (§35.3).

## Consequences

- The matrix is a declaration and the trait implementation is the other half of it. Four providers
  implement `Provider::temporal()` in this increment — `systemd`, `systemd-journal`,
  `systemd-logind` and `linux.procfs` — because those are the ones in the crates this work
  package owns. `linux.fs`, the three netlink providers, `linux.sock-diag`, `linux.nss`,
  `linux.mountinfo`, `linux.sysfs`, `linux.packages`, `linux.resolver`, `ono.probe`,
  `ono.session`, `ono.shell` and `container-engine` still answer the trait default, which claims
  *less* than the document. Under-claiming in code against an over-claiming document is the safe
  direction of drift — nothing reads a capability that is not there — and it is temporary.
- **The drift check is not yet written.** `cargo xtask spec-check` compares the `spatial:` block
  against `ono-spatial-core` in both directions; the equivalent for `temporal:` against
  `Provider::temporal()` belongs in `xtask/src/contracts.rs` and must land in the increment that
  finishes the trait implementations, because a check added now would fail on the fourteen
  providers above and could only be satisfied by editing crates outside this package.
- A KUANG/11 package contributing a historical query provider (§37.5) declares the same seven keys
  through its manifest, so a plugin and a built-in provider describe themselves in one vocabulary.

## Alternatives considered

- **A separate `docs/contracts/temporal/sources.yaml`.** A second place to look for a fact about a
  provider, and a second place for it to go stale. §36.1's `temporal.yaml` is for the temporal
  subsystem's own vocabulary, not for per-provider claims.
- **Deriving the matrix from the running registry.** Then it could never be wrong and could never
  catch anything: the point of a contract is that the implementation is measured against it.
- **Claiming what each source is capable of rather than what the provider implements.** The
  document would then describe Linux, not Ono, and a consumer choosing a source by capability
  would choose one that refuses.
