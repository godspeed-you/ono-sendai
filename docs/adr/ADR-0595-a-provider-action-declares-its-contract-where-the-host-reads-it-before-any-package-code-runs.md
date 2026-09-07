# ADR-0595: A provider action declares its contract where the host reads it before any package code runs

- Status: accepted
- Date: 2026-09-07
- Spec refs: §31.22, §31.46, §31.75, §10.5; `docs/contracts/kuang/contributions.v1.yaml`
  (`command.action`, `action`); `docs/architecture/external-system-provider.md` §19.4, §20.3,
  §21.1, §21.2, §21.5, §21.6, §22.2; ADR-0012, ADR-0587, ADR-0591, ADR-0594
- Decided by: agent (autonomous)

## Context

The generic provider contract's §21.1 says an action "is a typed operation with declared
semantics" that MUST specify its identity, accepted target types, parameter schema, required
provider capabilities, required KUANG/11 privileges, whether it mutates state, its idempotency,
its result schema, a verification hook or the explicit lack of one, and prospective-change
metadata. The stated objective is that a host can understand an action's safety and type
contract *before* executing provider-specific code.

A `CommandContribution` carried the identity, the parameters (ADR-0587), the privileges
(`capabilities`), the output and a `risk`. It had nowhere to say whether the command mutates, what
repeating it does, which objects it accepts, how its outcome is verified, or what a plan of it
should warn about. The Kubernetes provider's board recorded exactly that: "a `CommandContribution`
has fields for none of the three" it needed. What the host could read before running the code was
a risk word, and a risk word is a warning, not a contract.

## Decision

**A command contribution may carry an `action` block, in the shape §21.1 asks for, validated at
load, and travelling with the contribution into the registry the host reads.**

```yaml
action:
  targets: [io.github.example.thing/1]     # schema ids, or ["*"]; each resolves at load
  mutates: true
  idempotency: conditionally-idempotent    # idempotent | conditionally-idempotent | not-idempotent | unknown
  result: stream<io.github.example.outcome/1>   # where it differs from output; validated like output
  verification: "the object is read back at its own endpoint and compared"   # or null: an explicit lack
  effects: [restarts-workload]
```

Identity is the command id, parameters are its `options`, privileges are its `capabilities`, and
the risk is its `risk`; the block carries the rest. Three rules are checked when the package
loads, and a package that fails one is `package.invalid` naming the command:

1. an action that `mutates` declares `risk: mutate` or `risk: destructive`, so the host's
   confirmation policy (§21.5) reads what the action admits;
2. an action that `mutates` declares at least one capability of risk `mutate` or `destructive`
   — `provider.mutate` (ADR-0594) for a change in the system a provider fronts — so the host can
   refuse the action before any package code runs. An action that could only ever run under a
   read grant is refused at load rather than trusted at the call;
3. every `targets` entry other than `*`, and `result` where it is a schema, resolves like a
   target's schema (ADR-0591).

`idempotency` defaults to `unknown`, which is not `idempotent` (spec §10.5): a package that did
not say is not retried on its behalf (§19.4, §20.3). `verification: null` is §21.6's "explicit
lack thereof" and is shown as such, never inferred.

A command with no `action` is not a provider action. A read declares none (§21.2).

## Consequences

A host can now answer, from the registry and before invoking anything, whether a contributed
command changes an external system, under which authority, on which kinds of object, whether it
may be repeated, and how the package will say whether it worked. The Kubernetes provider's
`set k8s-resource` and `remove k8s-resource` declare the block against this core revision.

The example package's `mutate` command declares a complete action, and two misbehaviour modes
declare defective ones. `crates/ono-kuang-sdk/tests/conformance.rs`:

- `should_refuse_to_load_an_action_that_mutates_without_a_declared_risk`;
- `should_refuse_to_load_an_action_that_mutates_under_a_read_capability_only`;
- `should_carry_a_valid_action_contract_through_the_handshake`.

What this does not do: it does not put the block into `ono.command/1` records or into `explain`.
Those read `CommandContract`, whose fields are the core command vocabulary of ADR-0012, and
widening that record is a change to a stable schema that deserves its own increment with its own
version decision. The loaded package carries the action (`LoadedPlugin::commands()`), which is
what the host consults at invocation.

## Alternatives considered

**Infer mutation from `risk`.** Rejected: a risk is a warning level, and `network.connect` is
itself `risk: mutate` while being what every read over the network declares. Mutation of the
*external system* is a separate fact, and §21.1 lists it separately.

**A second contribution kind, `actions`, beside `commands`.** Rejected: the provider contract's
§21.1 and this repository's ADR-0582 both make an action a command a user types with a verb the
shell already has, and a parallel registry would be the duplicate command system the Kubernetes
provider's §4 invariant 22 forbids in its own domain.

**Validate at invocation rather than at load.** Rejected for the reason ADR-0591 gives: the
contract is decidable from the declaration, and a defect that surfaces when an operator types the
command is a defect in front of the wrong person.
