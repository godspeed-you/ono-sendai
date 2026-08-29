# ADR-0238: The dry run the mutation road always honoured

- Status: accepted
- Date: 2026-08-29
- Spec refs: v0.2 §11.6 and §17.4 (asking without obeying; a script never waits for a prompt),
  §16.5 (per-target outcomes), §50 (a delivered capability is documented and reachable);
  ADR-0068 (the mutation road), ADR-0088 (the network write paths), ADR-0233 (a declared option
  is honoured)
- Decided by: agent (autonomous)

## Context

ADR-0088 says of the nine network write paths: "`--dry-run` answers `skipped` with what would
have been sent." It does — `ProviderMutation` reads `--dry-run`, marks the `Action`, and
`ono-provider-netlink/src/act.rs` checks `is_dry_run()` at every one of its eight actions. The
mutation road honours it everywhere: files, processes, identities, packages, storage, services,
containers, plugins and the session's own tables all check it.

And no command in `docs/spec/commands/` declared the option, so:

```text
$ ono -c 'add route 10.99.8.0/24 --gateway 10.99.7.2 --dry-run'
ono: Ono-Sendai-E0202 type.unknown_field `add route` has no option `--dry-run`
```

ADR-0233 closed the direction where a *declared* option is honoured by nothing. This is the other
direction: a mechanism implemented everywhere, documented in an ADR, and reachable from nowhere —
which §50 makes the same kind of defect, because a capability a user cannot spell is not
delivered. It was found while writing the acceptance case for ADR-0088's own example.

## Decision

**The nine network write paths declare `--dry-run`**: `add`/`remove`/`set route`,
`set`/`start`/`stop`/`add`/`remove interface` and `stop socket`. They are the paths this
increment can *prove* honour it — `act.rs` checks `is_dry_run()` before every request it would
send, and acceptance case `123-privileged-network-writes` runs the dry run and then checks with
`ip(8)` that nothing was created.

An unprivileged dry run succeeds where the real mutation is refused, and that is the point: it
never reaches the kernel, so it answers `skipped` with the request it would have made, on any
machine, without privilege.

**The remaining fifty-four mutating commands are left undeclared, deliberately and for now.**
Declaring `--dry-run` on a command whose provider does not check `is_dry_run()` would make
`--dry-run` *act* — the worst failure this option can have, and the exact opposite of what a user
asking for it wants. Each family needs its own check that its `act` honours the flag before it
may advertise it, and that is one increment per family, not a sweep inside this one (AGENTS.md
§4). The gap is recorded here so the next reader finds it stated rather than discovers it.

## Consequences

- `add route 10.99.8.0/24 --gateway 10.99.7.2 --dry-run` answers
  `skipped — would add the route 10.99.8.0/24 via 10.99.7.2`, exit 0, on an unprivileged machine.
  ADR-0088's own example is now something a user can type.
- All nine write paths meet a kernel in `123-privileged-network-writes`: the eight rtnetlink ones
  against a dummy interface it creates and removes, and `stop socket` against a real established
  connection between the `git daemon` and the `curl` the image already ships.
- Help, completion and `docs/reference/commands.md` describe the option, because they are
  generated from the registry that now declares it.
- `spec-check`'s option rule (ADR-0233) covers the new options: `"dry-run"` is named in
  `ono-command`, and its behaviour is asserted by three tests and one containerised case.
- Encoded by `should_answer_a_dry_run_of_a_route_addition_without_asking_the_kernel_for_it`,
  `should_answer_a_dry_run_of_an_interface_change_without_asking_the_kernel_for_it`,
  `should_still_refuse_the_same_mutation_when_it_is_not_a_dry_run`, and acceptance case
  `123-privileged-network-writes`.

## Alternatives considered

- **Declare it on all sixty-three mutating commands at once.** It is one `sed` and it would turn
  `--dry-run` into a lie for every provider that does not check the flag: the user asks what
  would happen and the system does it.
- **Contribute `--dry-run` implicitly for every mutating verb, in the binder rather than in the
  registry.** It removes the duplication and it breaks the rule the whole harness rests on —
  `docs/spec/` is the contract, and `docs/reference/`, help and completion are generated from it.
  A synthetic option would be invisible to `spec-check` and to the reference pages.
