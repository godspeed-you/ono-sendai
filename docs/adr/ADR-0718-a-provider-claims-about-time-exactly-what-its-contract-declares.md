# ADR-0718: A provider claims about time exactly what its contract declares

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §21.1, §21.5, §36.4; v0.2 §35.3, §36.5; ADR-0710, ADR-0711
- Decided by: agent (autonomous)

## Context

v0.5 §21.1 ends "Capabilities MUST be inspectable", and §36.4 fails the gate when a provider
advertises a temporal capability its contract metadata does not carry. ADR-0711 landed the
declarations — a `temporal:` block on all nineteen provider entries in
`docs/contracts/providers/` — and `xtask/src/temporal.rs` holds `docs/contracts/temporal/
sources.yaml` against them.

Every one of those checks reads YAML. Nothing compared the YAML with the Rust.

The gap was not theoretical: fourteen of the nineteen providers implemented no `temporal()` at
all, so `Provider::temporal`'s all-false default answered for them while their contracts claimed
`current_snapshot` and `checkpointable`, and in two cases `live_events`. That is the safe
direction — the code under-claimed — and it still made the source capability matrix a description
of a tree that did not exist. A reader of `sources.yaml` would have concluded that `linux.fs`
pushes filesystem events; a caller of `registry.temporal_of("file")` was told it does not.

## Decision

**The generated conformance suite compares the two.** `Surface` gains a `temporal: TemporalClaim`
field carrying the six booleans of §21.1 as the provider's `temporal:` block declares them,
`xtask::conformance` writes it from the contract, and `assert_surface` asserts that the running
provider's `Provider::temporal()` answers exactly that. §35.3 gets the check it has always had for
schemas: a declaration is only worth what something holds the implementation to.

`retained_history` is excluded, for the same reason `xtask/src/temporal.rs` excludes it from its
own comparison: it is a duration rather than a claim, and a source's effective retention is its
configuration's rather than its contract's.

**All fourteen gaps are closed in the direction the contract already stated.** Nothing was widened
to make a check pass. Where implementation and contract genuinely disagreed the *contract* moved
and an ADR says why — ADR-0715 for systemd's live transitions, ADR-0716 for the container engine's
event stream — and everywhere else the code now says what the contract already said:

| provider | what it now claims, and why it is honest |
|---|---|
| `linux.netlink` (interface, route) | `live_events`: it joins the rtnetlink multicast groups |
| `linux.netlink` (neighbour) | snapshot only: `RTMGRP_NEIGH` is not joined (ADR-0717) |
| `linux.sock-diag` | snapshot only: §22.4's last line, as a claim |
| `linux.mountinfo` | `checkpointable`: `/proc/self/mountinfo` is a complete list at an instant |
| `linux.fs` | `live_events`, not `checkpointable`: inotify is real, file content is outside |
| `linux.sysfs` | snapshot only: udev's hotplug socket is not joined |
| `linux.nss` | snapshot only: a directory behind NSS may keep history and cannot be asked |
| `linux.packages`, `linux.packages.rpm` | snapshot only: dpkg's and rpm's install logs are unread |
| `ono.session` (env) | snapshot only: the shell records its own changes as actions |
| `linux.resolver`, `ono.probe` | snapshot only, and not checkpointable: a lookup is an event |
| `ono.shell` | snapshot only: the shell's tables answer about now |
| `container-engine` | `live_events` and `historical_query` (ADR-0716) |
| `systemd` | `live_events` (ADR-0715) |

## Consequences

- The source capability matrix is now true of the tree in both directions, and stays true: a
  provider that gains or loses a capability without saying so in its contract turns the gate red,
  and so does a contract edited without the code.
- `cargo run -p xtask -- conformance` must be re-run when a `temporal:` block changes, the same as
  for any other part of a provider declaration.
- The check is per provider *declaration*, and one provider id may front several target groups
  with different blocks — `linux.netlink` fronts three. The generated case resolves each
  declaration to the Rust provider serving its targets, so the three are checked separately and
  the neighbour provider's honest `live_events: false` is not masked by the interface provider's
  `true`.
- **A capability the contract declares and the code cannot implement now fails loudly.** That is
  the point, and it is also the cost: a contract can no longer describe an intention.

Encoded by `crates/ono-cli/tests/provider_conformance.rs`, one
`should_advertise_exactly_what_<provider>_declares` case per declaration, generated from
`docs/contracts/providers/*.yaml`.

## Alternatives considered

**A static check in `xtask`.** It cannot see a runtime value. It could grep for a `fn temporal`
in the provider's crate, which proves that something was written, not that it answers what the
contract says.

**Widen the defaults so silence means `current_snapshot: true`.** It would have closed twelve of
the fourteen gaps in one line and it inverts §21.1's rule: a provider that says nothing must claim
nothing, because silence is what a provider that has not thought about time produces (ADR-0710).
