# ADR-1002: A hello declaration the registry would refuse refuses the invocation

- Status: accepted
- Date: 2026-11-10
- Spec refs: spec §17.4, §31.22, §31.46, §31.75; external-system-provider §21.5; K11P §6.6, §7.5
- Issues: #156
- Decided by: agent (autonomous)

## Context

ADR-1000 refuses, at registration, a contributed command that needs a `mutate` or `destructive`
capability and declares no risk or a lower one. ADR-1001 confirms a `destructive` contribution
by the gate a core command uses, and for a command a running package declares only in its
startup hello it reads the hello's own declaration (`hello_contract` in
`crates/ono-cli/src/plugins.rs`).

That reading turned a refused declaration into no declaration: `hello_contract` returned `None`
for a hello the registry would refuse, and `None` meant "no confirmation policy". A package
whose hello says `risk: read` for a command needing `filesystem.write` therefore ran that command
without `--confirm`, once a person had enabled the destructive permission — which K11P §7.5
allows only at a terminal, and after which, by the shell's own words, "individual
mutate/destructive commands keep their own confirmations". Found by the orchestrator's review of
the #156 workstream, reproduced by the test below (the command answered `wiped`).

## Decision

**A command the running package declares in its hello is held to the registry's rules when it is
invoked: where the registry would refuse the declaration, the invocation is refused with the
registry's own reason (`package.invalid`), before anything is sent to the package.** The hello is
a second declaration of the same command, not a way around the first.

The example package states the risk its capabilities imply in every hello declaration, as an
honest package does, and its `--misbehave=action-without-risk` fixture says `risk: None` in so
many words.

## Consequences

- No route to a contributed command skips the confirmation its capabilities call for: on disk
  (ADR-1000), by its registry entry and its `<package>:<command>` spelling (ADR-1001), and now
  by its hello.
- A third-party package whose hello declares a mutating command without a risk, and that has no
  declaration on disk, loses that command at invocation with the same message `get plugin` gives
  for the disk declaration. The fix is the same `risk:` line.
- The supervisor's own hello validation (ADR-0595) still does not apply the floor at load; the
  host applies it where the declaration is read for a decision. ADR-1000's follow-up — one check
  shared by both paths in the supervisor — remains the tidier home and is not needed for safety.
- Test: `crates/ono-cli/tests/plugins.rs::should_refuse_a_command_whose_hello_understates_its_risk_without_reaching_the_package`
  (pty: the destructive permission is enabled by answering `y`, the capability granted, and
  `echo:wipe` with and without `--confirm` is refused as `package.invalid` and never reaches the
  package's audit trail; RED: the command answered `wiped` without `--confirm`). No acceptance
  case: enabling a destructive permission needs a person at a terminal, which the container cases
  do not have.

## Alternatives considered

- **Raise an understated hello risk to the floor silently.** Would gate the command correctly,
  but registers a statement nobody made, which ADR-1000 rejected for the disk declaration
  ("do not implicitly grant", unknown is not a value).
- **Refuse the package at load in the supervisor.** The right long-term home, but it changes
  what every test host accepts and is a protocol-level change beyond this fix; the refusal at
  invocation closes the hole now.
