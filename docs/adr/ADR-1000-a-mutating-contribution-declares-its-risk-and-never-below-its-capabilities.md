# ADR-1000: A mutating contribution declares its risk, and never below its capabilities

- Status: accepted
- Date: 2026-11-10
- Spec refs: §31.22, §31.46, §31.75; K11P §6.6, §15.3; `docs/architecture/external-system-provider.md`
  §21.5; `docs/contracts/kuang/contributions.v1.yaml` → `registration_checks.risk-metadata`,
  `command.fields.risk`; `docs/contracts/capabilities.yaml` → `kuang_capabilities`;
  ADR-0587, ADR-0594, ADR-0595; issue #156
- Decided by: agent (autonomous)

## Context

`contributions.v1.yaml` has said since it was written that "every mutating command … declares its
risk". ADR-0587 carried the declaration into the registry entry and showed it in `help`, but did
not make it required, because that increment was constrained not to refuse a package that
installed. So a contribution declaring `capabilities: [network.connect]` — a `mutate` capability —
and no `risk:` registered, and `help` showed no risk at all (#156).

The registry entry is also what the host's confirmation policy reads (ADR-1001). A risk that may be
left out is a confirmation that may be left out.

Two questions were open:

1. What makes a command "mutating"? The contract says it and never defines it.
2. May a package declare a risk *lower* than the capabilities it needs carry?

ADR-0587 argued that a capability cannot tell a read from a write over `network.connect`, and that
risk is the package's own statement about the world. ADR-0594 then gave a write in a fronted
system a capability of its own, `provider.mutate`, precisely so that the capability set *can* say
it. K11P §6.6 states the rule for the permission a capability is granted under: "The host MUST
derive a minimum risk from the underlying capabilities. A package MAY raise the risk but MUST NOT
lower it."

## Decision

At registration — `ContributedCommand::into_contract`, the step every on-disk command declaration
passes before it becomes a registry entry — the host computes the **floor**: the highest risk, in
`kuang_capabilities` of `docs/contracts/capabilities.yaml`, among the capabilities the command
declares.

- A command whose floor is `mutate` or `destructive` and that declares no `risk:` is
  `package.invalid`. The refusal names the package, the command id, the capability that sets the
  floor and its risk, and the help names the line to add.
- A command whose declared risk is below its floor is `package.invalid` the same way. A package may
  raise the risk its capabilities carry, never lower it — K11P §6.6's rule, applied to the command
  that uses the capability as well as to the permission that grants it.
- A command whose floor is `read` or `observe`, or that needs no capability, may still say nothing:
  only a mutating command must declare (`registration_checks.risk-metadata`).
- An id that is no capability does not set a floor here; `capabilities-exist` refuses it at load.

A refused declaration is reported by `get plugin` with the other refusals and never reaches
`get command`, `help` or completion, as ADR-0282 does for every unreadable declaration. The rest of
the package's declarations are unaffected.

## Consequences

- A command that only reads over `network.connect` must now declare `risk: mutate` or higher. This
  over-states a read in `help`. It is the honest consequence of `network.connect` being classed
  `mutate` — an outbound connection can carry anything — and `mutate` asks for no confirmation
  (ADR-1001), so the cost is a label. A package that wants a read to read as a read contributes it
  as a *target*, which is read by construction (§21.2) and declares no capability.
- A command needing `filesystem.write` or `recovery.restore` (`destructive`) must declare
  `risk: destructive`, which ADR-1001 makes ask for `--confirm` on every run.
- **Compatibility.** A third-party package whose command declares a `mutate` or `destructive`
  capability without a `risk:` line, or with one below the capability's, installed before and is
  now refused at registration: its command is missing from `get command` and `get plugin` reports
  why. The fix is one line in the contribution. The Kubernetes provider already declares
  `risk: mutate` and `risk: destructive` on its two mutating commands and is unaffected by this
  rule.
- The check runs on the on-disk declaration. The hello a running package sends is validated by the
  supervisor (ADR-0595) and is not held to this floor; ADR-1001's confirmation gate therefore reads
  the hello's own declared risk as well, so a destructive command is gated either way.
- Tests: `crates/ono-command/tests/contributed_risk.rs` (missing risk for a `mutate` and a
  `destructive` capability, a risk below the floor, a declared risk, a reading command);
  `crates/ono-cli/tests/plugins.rs::should_refuse_and_report_a_mutating_contributed_command_that_declares_no_risk`;
  acceptance case 160.

## Alternatives considered

- **Require a risk, accept any value.** ADR-0587's reading: risk is the package's statement, so
  any statement satisfies the rule. Rejected: the declaration is what the confirmation policy
  reads, so a package needing `filesystem.write` could declare `read` and never be confirmed — the
  one outcome "a plugin must not bypass native security and mutation boundaries" forbids. Since
  ADR-0594 a package can say a write is a write with a capability; it no longer needs to say it
  with a lower risk.
- **Default a missing risk to the floor.** Quietly registers what the contract says must be
  declared, and leaves `help` showing a risk nobody stated. Rejected: "do not implicitly grant",
  and §10.5 — unknown is not a value.
- **Enforce in the supervisor's hello validation as well.** The correct long-term home for a check
  that both declaration paths share, but every hello the test hosts and the example package build
  would need the same lines, which is a separate increment outside this one's files. Recorded in
  the run report as a follow-up.
