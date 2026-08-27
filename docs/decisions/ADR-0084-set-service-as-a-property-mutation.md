# ADR-0084: `set service --enabled` is the property form of enable/disable, and a `set` without a property is a usage error

- Status: accepted
- Date: 2026-08-27
- Spec refs: §7.1, §9.1, §11.5, §16.5, §23.3, §43, §52 (`service/set` = "M"); ADR-0006, ADR-0068
- Decided by: agent (autonomous)

## Context

Spec §52 marks `service/set` as a plausible mutation and §9.1 defines no such command, so
`docs/spec/commands/service.yaml` declares `ono.service.set` as experimental with one property,
`--enabled <bool>`, "whether the unit starts at boot". ADR-0068 §3 binds the command as soon as
the systemd provider advertises `service.manage`, and the provider already carried out
`enable`/`disable` through `EnableUnitFiles`/`DisableUnitFiles` (spec §23.3) — but only under
those operation names. `set service nginx --enabled false` therefore reached the provider as the
operation `set` and came back as one `failed` row with `E0402 provider.unsupported`, which is a
wrong answer for a declared command (spec §50).

Two things had to be decided: how a property mutation reaches a provider, and what
`set service <unit>` with no property at all means.

## Decision

1. **A property travels as an action argument, under the option's name.** The mutation seam
   already forwards every declared option of the contract into the `Action`
   (`ProviderMutation`, ADR-0068 §3). A provider that delivers `set <target>` reads the
   properties it knows from `action.argument(<name>)` — the systemd provider maps
   `enabled: true|false` onto the same unit-file change `enable`/`disable` make, with the same
   `skipped` when the unit file already is that way and the same `dry-run` answer. A property
   of the wrong type, or a `set` that carries none the provider can change, is
   `provider.unsupported` naming the property — the "cannot attempt" answer of the provider
   contract, not a per-target failure.

2. **`set <target> <selector>` with no property is a usage error before anything is resolved.**
   `ProviderMutation` refuses it with `E0201 type.mismatch` ("needs a property to set, and none
   was given") whose help lists the contract's property options (`--enabled` for a service),
   exactly as `start service` with no selector is refused. This holds for every `set` the
   registry dispatches — a `set` that changes nothing is not something a provider should have
   to answer per target, and the option surface is the contract's to know. `--dry-run`,
   `--confirm` and `--provider` are not properties and do not count as one.

3. **The outcome contract is unchanged.** A refused change (polkit, `E0302`) or a unit that
   does not exist (`E0301`) is one `ono.action-result/1` row per target and exit status 1
   (ADR-0006, ADR-0068 §2); the piped form `get service X | set service --enabled false` acts on
   the piped identities like every other mutation.

## Consequences

- `crates/ono-cli/tests/services_logs_missing.rs` — the four `set service` cases — and
  `crates/ono-provider-systemd/tests/service.rs` (`set` with `enabled`; `set` with nothing)
  encode this. Acceptance case 038 exercises the E0401 path in the container, which has no
  service manager.
- Other families deliver `set <target>` by reading their properties from the action's
  arguments; nothing in the command crate names a property.
- The refusal of point 2 is generic to the verb `set`; a family whose `set` legitimately takes
  no option would need its own binding — none is declared today.

## Alternatives considered

- **Translate `--enabled true|false` into the operations `enable`/`disable` in the command
  crate.** Rejected: the command crate would then know the properties of every target, which is
  the provider's knowledge (spec §27.1), and a second property would need a second translation.
- **Let the provider answer the property-less `set` per target as a `failed` row.** Rejected:
  it is a usage error (nothing was asked), it would cost a resolution round trip to say so, and
  a `failed` row for a request that asked for nothing misreports the system's state.
