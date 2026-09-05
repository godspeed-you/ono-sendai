# ADR-0246: A command declares its verb, its target and its argument mode

- Status: accepted
- Date: 2026-08-29
- Spec refs: §7 (the verb registry), §8 (targets), §27 (the command registry), §53 (transforms);
  ADR-0009 (argument modes), ADR-0012 (registry conventions), ADR-0124 (bare-name spatial verbs)
- Decided by: agent (autonomous)

## Context

`xtask::contracts::check_commands` cross-checks each command's `verb` against `verbs.yaml`, its
`target` against `targets.yaml`, and its `argument_mode` against the grammar ADR-0009 fixes. Each
of the three read

```rust
let verb = string_at(command, "verb").unwrap_or_default();
if !verb.is_empty() && !verbs.contains(&verb) { … }
```

so a command that simply omitted the field was checked against nothing and passed. Nothing is
dead today — every one of the 184 commands declares all three — but the guard could not fire on
the one input it most needs to catch: a contract that says nothing at all. `help`, completion and
the pre-flight check all read these fields, and a command that leaves one out makes each of them
guess.

The reason this needed deciding rather than just fixing is ADR-0124. The spatial verbs take the
bare name — `look`, `near`, `map` — and it would be an easy mistake to read "bare name" as "no
target word, therefore no `target` field", or "not a `get`, therefore no `verb`". A field is
about the registry; a bare spelling is about the parser. They are not the same question.

## Decision

**`verb`, `target` and `argument_mode` are required keys of every command entry in
`docs/contracts/commands/*.yaml`.** A command that omits any of them is reported by `spec-check`.

`target: null` remains a declaration and stays valid: a transform operates on whatever the
pipeline carries and names no target (spec §53), and writing that down is how a reader tells it
apart from an oversight. The distinction the check draws is between *declared null* and *absent*.

The bare-name spatial verbs are unaffected: `look` declares `verb: look`, `target: place` and
`argument_mode: words` exactly as `get process` declares its three, because ADR-0124 changed how
the line is *typed*, not what the registry knows about it.

## Consequences

`spec-check` now fails on a command with a missing `verb`, `target` or `argument_mode`, which is
what the three cross-checks were always meant to do. The registries in this repository already
satisfy the rule, so nothing changed in `docs/contracts/`.

Encoded by `xtask/tests/contracts.rs::should_reject_a_command_that_declares_no_verb`,
`::should_reject_a_command_that_declares_no_target`,
`::should_reject_a_command_that_declares_no_argument_mode` and
`::should_accept_a_command_whose_target_is_explicitly_null`.

## Alternatives considered

- **Defaulting the missing field** — `argument_mode: words` when absent, say. Rejected: a default
  that is applied silently is exactly the guessing the registry exists to stop, and it would make
  the ADR-0009 disagreement check pass by construction.
- **Leaving the guards as they were**, on the grounds that no command omits a field today.
  Rejected: a check that cannot fail is not a check (ADR-0159 made the same finding about a dead
  branch), and the cost of the rule is one line per command that is already written.
