# ADR-0594: Provider mutation is a capability of its own, because reaching a system is not changing it

- Status: accepted
- Date: 2026-09-07
- Spec refs: §31.16, §31.18, §31.19, §31.21, §31.37, §31.75, §31.80;
  `docs/contracts/capabilities.yaml` (`kuang_capabilities`), `docs/contracts/kuang/capabilities.v1.yaml`;
  `docs/architecture/external-system-provider.md` §18.4, §21.1, §21.5, §27.1, §27.2; ADR-0022,
  ADR-0573
- Decided by: agent (autonomous)

## Context

A KUANG/11 provider package that fronts an external system reaches it over the network broker:
`ADR-0573` decided the host brokers a connection and not a protocol, so a package speaks its own
HTTP (or whatever) over a stream of bytes. `network.connect` is the grant for that stream, scoped
by host and port.

The consequence, which the Kubernetes provider's ADR-0024 recorded as a finding it could not fix
from its own side: **an operator who grants a provider the connectivity to read a system has, in
the same act, granted it the connectivity to write to one.** Both a `GET` and a `PATCH` travel as
bytes through the one grant, and the broker cannot tell them apart on the wire — it is a byte
stream carrying a protocol the host does not parse. The two mutating commands the Kubernetes
provider ships therefore declared `network.connect`, because it was the only honest capability
available: `service.mutate` and `remote.mutate` carry scope keys belonging to other domains, and
an unknown capability id fails the manifest as `package.invalid`.

`risk: mutate`/`risk: destructive` on the command, `dry_run` defaulting to true, and the host's
confirmation policy are real safety mechanisms, but they are not a security boundary: they govern
what the operator is warned about and asked to confirm, not what authority the package holds. A
package that never declared a risk, or a compromised one, still has the transport authority to
send a mutation the moment it holds `network.connect`.

The generic provider contract states the boundary that was missing. §18.4: "Mutation
authorization" is its own concern. §21.1: a provider action declares "required provider
capabilities" and "required KUANG/11 privileges" as distinct things. §27.2 scopes network access
to reaching the system, and says nothing that makes reaching it the authority to change it.

## Decision

**A thirtieth capability family, `provider.mutate`: the authority to change state in the external
system a provider package fronts. It is core's, not Kubernetes's, and it is enforced by the host
at every invocation of a contribution that declares it, before any package code runs.**

- `risk: mutate`, `elevation: none`, in both `docs/contracts/capabilities.yaml` and
  `docs/contracts/kuang/capabilities.v1.yaml`, and `Capability::ProviderMutate` in the runtime
  registry. `Capability::ALL` is now thirty; `spec-check` compares the registry against the two
  contracts in both directions.
- Three scope keys, all **advisory**: `instances` (the provider instances the package may change,
  `kubernetes:prod-eu`), `resources` (the resource classes, `apps/Deployment`) and `actions` (the
  action classes, `apply`, `delete`, `scale`). They are advisory for the reason the capability
  contract's own rule demands they be labelled so: the host cannot see which instance or resource
  a byte on a brokered connection addresses, so the broker checks these on the package's own
  `capabilities.check` call and the family-level grant is the boundary. A scope Ono cannot enforce
  is never presented as one it can (§31.16, §31.80).
- Enforcement is the mechanism that already gates a command's declared capabilities: the
  supervisor evaluates every `capabilities` entry of an invoked command against the policy before
  the handler runs, denies with `capability.denied`, and audits the denial as loudly as a success
  (§31.37). A mutating provider command declares `provider.mutate`, and the host does the rest.

**A read-only grant does not authorize mutation, and a mutation grant does not authorize
transport.** A package that changes state declares and is granted both `network.connect` (to
reach the system) and `provider.mutate` (to change it). Granting one is not granting the other.

## Consequences

The eight attacks the family exists to withstand, and where each is answered:

1. `network.connect` only → reads succeed, a mutation command is refused before it runs;
2. `provider.mutate` only → the package cannot open a connection, so nothing reaches the system;
3. both → the mutation proceeds and is audited;
4. wrong provider instance → the package's own `capabilities.check` against the `instances` scope
   answers `Denied`, advisory-enforced and audited;
5. wrong resource/action → the same, against `resources`/`actions`;
6. dry-run is not a bypass → `dry_run` is an argument to a command that already required the
   grant to be invoked at all;
7. hidden provider code cannot reach mutation through a read handler → a read is a contributed
   *target* answered through `provider.query`, which declares no capabilities and opens no write;
   a write is a contributed *command* whose `provider.mutate` the host checks;
8. denial produces a structured error (`capability.denied`) and both the denied and the allowed
   attempt are in the trail.

No Kubernetes concept entered core: `provider.mutate` is meaningful for every external-system
provider, and an unrelated future provider (AWS, a database) uses it unchanged. The Kubernetes
provider migrates its two mutating commands from `network.connect` to declaring both
`network.connect` and `provider.mutate`, which is a change in that repository against this core
revision.

The tests, in `crates/ono-kuang-sdk/tests/conformance.rs`, drive the example package's
`command.mutate` (which declares `provider.mutate`) through the test host:

- `should_refuse_a_provider_mutation_command_without_the_provider_mutate_grant` — refused at the
  call, nothing emitted, the attempt audited as denied;
- `should_run_a_provider_mutation_command_with_the_provider_mutate_grant` — permitted, the
  handler runs, the success audited.

## Alternatives considered

**Scope `network.connect` by HTTP method.** Rejected: the host does not parse the protocol on the
brokered connection (`ADR-0573`), so a method scope would be a scope Ono cannot enforce presented
as one it can — the precise thing §31.16 forbids.

**Leave it to `risk`/`dry_run`/confirmation.** Rejected: those are a warning and a default, read
by the operator and by host confirmation policy, and they gate what a *correct* package asks to
do. They are not authority, and a boundary that a buggy or hostile package can step over by
omitting a field is not a boundary (§31.80's "capability scope is broader in implementation than
UI suggests", read the other way).

**Make it enforceable rather than advisory in its scope.** Rejected as impossible today for the
reason above, and recorded rather than hidden: the family-level grant is the enforced boundary,
the scope narrows an honest package and is audited, and the day a host brokers the protocol rather
than the bytes the scope can be promoted to `broker` enforcement without changing the grant.

**Put `kubernetes.mutate` in the Kubernetes package.** Rejected: an unknown capability id is
`package.invalid`, capabilities are core's security vocabulary, and the concept is generic. The
provider contract §0.4's test — could an unrelated future provider ignore this without carrying
Kubernetes baggage — is failed by a Kubernetes-named capability and passed by this one.
