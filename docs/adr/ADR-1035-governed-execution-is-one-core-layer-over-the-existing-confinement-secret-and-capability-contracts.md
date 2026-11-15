# ADR-1035: Governed execution is one Core layer over the existing confinement, secret and capability contracts

- Status: accepted
- Date: 2026-11-15
- Spec refs: Security Specification 01 (`docs/architecture/security-spec-01-governed-execution.md`)
  §0, §2.1, §6, §7, §8, §9, §10, §12–§16, §20.3, §21, §22.1; v0.10 §0.3, §10.6, §11.1, §14.4,
  §23.6, §24.5, §28.3, Gates AS–AW; v0.2 §31.10, §31.15–§31.21, §31.37, §31.49; v0.4 remote scope
  and link identity; v0.4.1 §15.1–§15.3, §16, §17.1–§17.3, §19.1, §52.1; v0.5 evidence and
  provenance; v0.6 `ChangePlan`, protection, verification and recovery; v0.8/v0.9 Deck and duration;
  K11P §0.3, §7, §27.2; `docs/contracts/hardening/kuang_confinement_controls.yaml`,
  `docs/contracts/hardening/terminology.yaml`, `docs/contracts/kuang/protocol.v1.yaml`
  (`secrets.*`, `network.request`); ADR-0283, ADR-0442, ADR-0443, ADR-0447, ADR-0570, ADR-0573,
  ADR-0600, ADR-1016, ADR-1034; issues #203, #266, #268, #273, #274, #277, #279
- Decided by: the user (Security Specification 01, 2026-11-15); recorded by agent

## Context

On 2026-11-15 the user added Security Specification 01, *Governed Execution, Sandboxes &
Credential Mediation*, and revised v0.10 to depend on it. The specification defines three generic
Core mechanisms — an execution sandbox, a Credential Broker and an operation policy with trusted
executors — usable by ordinary shell processes, subshells, plugins, automation and AI agents. v0.10
is its first consumer: every assistant tool call executes under the effective Security Scope, and
the native assistant's protected tool use blocks the v0.10 release (Security Specification 01
§22.1; v0.10 §23.6, Gates AS–AW).

The specification is cross-release and carries no version number of its own, so two questions are
open: where it lives and how authoritative it is, and how it meets the security contracts the shell
already has. Several of those contracts already speak about the same ground:

- **Confinement (v0.4.1 §15–§17, ADR-0283, ADR-0442, ADR-0443, ADR-0570).** Native packages and
  `process.exec` run *confined* — resource ceilings, `no_new_privs`, own session, scrubbed
  environment, private working directory — and the control table
  `kuang_confinement_controls.yaml` is the single registry the gate compares. Landlock and seccomp
  are rows marked `not_provided`, and the tier `native-isolated` is named with `available: false`.
- **Terminology (v0.4.1 §17.3, §19.1, ADR-0447).** "Sandboxed" and "isolated" may only be used for
  a boundary the kernel enforces; `terminology.yaml` makes that checkable on README, `help` and the
  generated reference.
- **Secrets (ADR-1016, ADR-0573, #203, #279).** `secrets.request` issues no handle, because the
  shell has no store and no host operation consumes a handle; ADR-1016 says a store arrives "as one
  change with all three parts" — source, consumer, expiry — in a milestone that plans it.
- **Permissions (K11P, ADR-0600).** "A user grants intentions; KUANG/11 grants capabilities", with
  `secret.use` as the capability behind a credential.
- **Remote scope (v0.4), evidence (v0.5), change (v0.6), Deck and duration (v0.8/v0.9).** Each owns
  its concept; Security Specification 01 §2.1 lists them as authoritative and adds enforcement only.

## Decision

**Security Specification 01 is a normative architecture specification, and its mechanisms extend
the contracts above; none of them is duplicated.**

1. **Place and authority.** The specification lives at
   `docs/architecture/security-spec-01-governed-execution.md`, beside
   `external-system-provider.md`, because it is cross-cutting and tied to no numbered release
   (AGENTS.md §2, §5). An architecture specification never overrides a narrative specification on
   its own; v0.10 incorporates this one by reference for assistant tool execution, so for that
   path its requirements carry v0.10's authority. Where it meets an earlier narrative
   specification, the earlier owner keeps its concept (Security Specification 01 §0, §2.1) and an
   ADR records any conflict.
2. **Immutable like the narrative specifications.** Its checksum is recorded in
   `docs/specs/spec.sha256`, so `spec-check` notices an edit. Only the user changes it; an agent
   records ambiguity or a deviation in an ADR with a `Spec deviation` heading (AGENTS.md §5.1, §8).
   A security requirement therefore cannot be weakened by an edit nobody reviewed.
3. **One confinement registry.** The sandbox (Security Specification 01 §7, Phase S2) is the
   implementation of v0.4.1's reserved stronger tier: its kernel controls (Landlock, namespaces,
   seccomp, IPC and FD restrictions) become rows of `kuang_confinement_controls.yaml`, its
   enforcement report extends the existing confinement report, and `native-isolated` becomes
   `available: true` only where those rows are verified on the running host. The same registry
   serves subshells, tool subprocesses and external processes; there is no second control table.
4. **Terminology follows enforcement.** Until a verified backend exists, v0.4.1 §17.3 and
   `terminology.yaml` stand unchanged. When S2 lands, the qualifier for "sandboxed" names the
   verified profile and backend; a bare claim stays forbidden.
5. **One secret model.** The Credential Broker resolves the existing opaque `secret.use`
   references (Security Specification 01 §2.1, §8.1, §15.3). It is the store ADR-1016 deferred,
   delivered with all three parts: an operator-controlled source (#279's store), consumers (trusted
   executors and trusted transports, Security Specification 01 §8.4–§8.5, §9.4) and handle expiry
   bound to session and operation. ADR-1016 stays in force until that change lands; the change that
   lands it supersedes ADR-1016 by a new ADR, as AGENTS.md §8 requires.
6. **One permission system.** `SecurityProfile` and `CredentialBinding` are data that the policy
   compiler intersects with the existing grants (Security Specification 01 §6.3: operator, system,
   parent, session, KUANG/11 capabilities, target, credential). They compile into the capability
   broker's decisions; they add no second grant store, no second consent dialog family and no
   second plugin lifecycle. K11P's intention-level prompts remain the user-facing form
   (Security Specification 01 §16.3).
7. **Existing owners stay owners.** Remote restrictions are enforced at the v0.4 remote boundary
   (Security Specification 01 §7.8, §13.4); approvals for mutation bind to the sealed v0.6
   `ChangePlan` digest and never replace plan approval (§10.4); audit and correlation reuse the
   existing history and evidence records (§14.1); long-running governed work uses v0.8/v0.9 jobs and
   views. Security Specification 01's example commands (`enter sandbox`, `get credential-binding`)
   are provisional: verbs and targets are registered through the existing registries, by ADR, when
   they are implemented (§6.1, §16.1).
8. **What blocks v0.10.** Phases S0–S4 and the gates Security Specification 01 §22.1 names for the
   native assistant's protected tool use block v0.10. Phase S5 — ordinary subshells under a profile
   and external agents at Levels A–C — may ship independently and later; it blocks v0.10 only if
   v0.10 advertises protected external-agent execution (v0.10 §38). Phase S6 hardening is part of
   v0.10's release proof for whatever v0.10 advertises.
9. **Implementation ADRs.** The six decisions Security Specification 01 §20.3 calls ADR-S1 … ADR-S6
   (scope schema, Linux backend, broker privilege boundary, credential binding, approvals, v0.10
   integration) are taken in Phase S0 under #266 and numbered normally then.

## Consequences

- `spec-check` covers the new file; AGENTS.md §2 and §5.2, `docs/STATE.md` and the README's
  documentation list name it.
- Open question for S0/#279: how a model-provider transport authenticates. ADR-0573 has a package
  speak its own protocol over `network.connect`, so the package holds whatever authenticates the
  request. v0.10 §7.4 now requires broker-mediated credential use and allows a provider transport
  to hold material only under Security Specification 01's trusted-transport rules. Either the host
  takes over authenticated requests (reversing ADR-0573's decision 1, as ADR-1016 anticipated), or
  a provider transport tier is defined whose isolation is verified. The choice is S0's; until then,
  no connection type may be advertised as protected credential use.
- The existing confinement tests, the terminology gate and acceptance cases 178 and 213 stay green
  and unchanged by this decision; it adds requirements and changes no shipped behaviour.
- Same-UID attacks on the broker (Security Specification 01 §2.2, §15.2) are not covered by
  today's threat model (ADR-0015, ADR-0245). ADR-S3 must extend it before any broker ships.

## Alternatives considered

- **Place it under `docs/specs/` as a numbered enhancement.** Rejected: it declares itself "not a
  new feature-release number", and AGENTS.md §2 reserves `docs/specs/` for the release sequence
  and the KUANG/11 UX specifications.
- **Leave it editable like `external-system-provider.md`.** Rejected: an agent could then weaken a
  security requirement in the same commit that fails to meet it. The checksum makes every change a
  visible, user-owned act.
- **An AI-specific sandbox and secret mechanism inside the assistant.** Rejected by both
  specifications (v0.10 §23.6, Security Specification 01 §0): the mechanisms are generic Core
  services and the assistant is one consumer.
- **A new security milestone for S0–S6.** Rejected for now: the milestones are releases, and the
  specification is not one. Work that blocks v0.10 is tracked in `v0.10.0`; S5 work stays without a
  milestone until the user triages it.
