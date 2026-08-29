# ADR-0281: A registry entry says where it came from

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.64, §31.22, §31.68, `docs/spec/kuang/contributions.v1.yaml`
- Decided by: agent (autonomous, C4-kuang)

## Context

Spec §31.64 says every registry entry records an origin, and that `inspect command`,
`inspect schema` and `explain` expose it:

```text
origin = core | plugin(package-id, version) | remote-provider(...)
```

`docs/spec/kuang/contributions.v1.yaml` adds two rules the narrative leaves implicit: origin is
set by the host at registration and is not a manifest field, so a package cannot declare itself
core; and a contributed command is declared with the *same* metadata schema a core command uses.

Nothing in `ono-command` carried an origin. `CommandContract` was built from
`docs/spec/commands/*.yaml` and had no room for anything else, so a contributed command could not
enter `CommandRegistry` even in principle: there would have been no way to tell it apart from a
command Ono ships. That is the blocker under B-kuang-3, and it has to be lifted before §31.68's
lazy placeholders can exist.

## Decision

`ono_command::Origin` is part of the command contract:

```rust
pub enum Origin {
    Core,
    Plugin { package: String, version: String },
}
```

- `Display` writes `core` and `plugin(dev.example.echo, 0.1.0)` — the spelling §31.64 uses, and
  the spelling every user-visible surface prints.
- `RawCommand::into_contract` always produces `Origin::Core`. A contract document never names its
  own origin, whether it was read from `docs/spec/commands/` or from a package's
  `contributions/`; the host re-attributes a contribution with `CommandContract::with_origin` at
  registration.
- `ono.command/1` gains a required `origin` field, so `get command` and `find command` answer it
  for every entry.
- `help <command>` prints `origin` in its CONTRACT block and carries it in `to_value`.
- `explain` carries `origin` on every stage plan and in `to_value`; the rendered plan prints an
  `origin` row **only when the origin is not core**, because a core command's origin is the
  answer nobody asks for and a contributed one's is the first question about it.
- `resolve command <word>` answers `core`. It resolves a *head word* through the shell's own
  resolution order (spec §6.5, ADR-0011), and that order is the shell's, not a package's.

`remote-provider(...)` is the third value `contributions.v1.yaml` names. It is deliberately not an
arm of this enum: the remote registry projection of spec §31.40 constructs nothing today, and an
arm nothing can build is a placeholder. It is added when §31.40 registers its first entry.

## Consequences

- A contributed command can now live in `CommandRegistry` beside a core one and still be told
  apart by every surface that shows commands. That is what B-kuang-3 needs.
- `ono.command/1` grew a required field. It is additive — no existing field changed meaning — so
  the schema keeps major version 1.
- Encoded by `crates/ono-command/tests/registry.rs::should_attribute_every_embedded_command_to_the_core`,
  `::should_write_a_package_origin_as_the_package_and_its_version` and
  `crates/ono-command/tests/meta.rs::should_say_a_core_command_was_contributed_by_the_core`.

## Alternatives considered

- **A separate plugin-command registry.** Rejected: §31.64 is explicit that there are no
  KUANG/11-private registries — "a contributed command is a command, and the same `get command`
  finds it". A second registry would make every consumer ask twice and get the shadowing rules
  wrong in two places.
- **`origin` as an optional string on the YAML contract.** Rejected by
  `contributions.v1.yaml`: "Origin is set by the host at registration and is not a manifest
  field. A package cannot declare itself core." A parseable field is a field a package can lie
  in.
