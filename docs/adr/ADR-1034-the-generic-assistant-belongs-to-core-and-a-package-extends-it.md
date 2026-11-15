# ADR-1034: The generic assistant belongs to Core, and a package extends it

- Status: accepted
- Date: 2026-11-15
- Spec refs: v0.2 §7 (`ask` verb table), §31.41, §31.42, §31.43, §31.87; v0.10 §0.3, §4.1,
  §4.4, §4.5, §5.1, §24, Gates AM–AR; Security Specification 01 §2.1, §12.1, §12.4, §24;
  `docs/contracts/kuang/assistants.v1.yaml`, `docs/contracts/kuang/contributions.v1.yaml`
  (`AssistantRegistry`, `origin`), `docs/contracts/commands/kuang.yaml` (`ono.assistant.get`,
  `ono.assistant.ask`), `docs/contracts/targets.yaml` (`ono.target.assistant`); ADR-0022,
  ADR-0111, ADR-0566; issues #266, #267, #272, #276
- Decided by: the user (v0.10 revision of 2026-11-15); recorded by agent

## Context

The user revised v0.10 on 2026-11-15. The revision makes the generic assistant a native Core
capability: Core owns the assistant runtime, the model and context brokers, egress policy, the
typed tool bridge and the ChangePlan bridge; KUANG/11 model-provider packages supply transport and
authentication only; other KUANG/11 packages extend the assistant through the typed objects and
operations they already register (v0.10 §0.3, §24.1). v0.10 §0.3 and §5.1 require an ADR wherever
this collides with the exact v0.2 contract, before the machine-readable representation changes.

It collides in four places, each of which the shell implements today (spec §37 Phase I built "the
package side" of the assistant contract, `assistants.v1.yaml` header):

1. **v0.2 §31.41** draws the flow from an `assistant plugin`, and **§31.42** gives the object a
   mandatory owner: `Assistant { id, plugin: PluginId, ... }`. `contributions.v1.yaml` has an
   `AssistantRegistry` fed only by the `assistant` contribution kind, and `ono.target.assistant`
   is "a loaded assistant extension". A clean installation therefore has no assistant at all.
2. **v0.2 §7** defines `ask` as "send a request to an explicitly selected assistant" and lists
   `ask assistant <id> <request>`; `ono.assistant.ask` makes `id` mandatory ("Never implicit").
   v0.10 §4.1 and §4.4 make `ask assistant "<request>"` resolve to the built-in assistant.
3. **v0.2 §31.43** has a plugin ask for a model by requirements and calls that what "makes
   assistant packages portable". v0.10 §24.3 reduces package model preferences to non-binding
   metadata; model selection is `ModelPolicy`, owned by the operator (§8).
4. **v0.2 §31.42** examples (`inspect assistant ops`, `enter assistant ops`) present named
   assistants as separate things; v0.10 §4.4, §24.1 and Gate AR allow exactly one runtime, with
   named contexts/profiles as configuration.

AGENTS.md §5.2 settles overlaps between enhancement specifications: the later version wins, and
the ADR implementing that part cites both. v0.10 is the later version. Nothing here is a spec
deviation, so this ADR carries no `Spec deviation` heading; it records how the v0.2 contract is
read from now on, so the contract change in #266/#267 has a rule to implement.

## Decision

**One generic assistant runtime exists, it is Core's, and it is present in every installation.**

1. **Ownership.** The built-in assistant is a registry entry with origin `core` — the one origin
   `contributions.v1.yaml` allows to hold an `ono.*` id. Its runtime is Core code. No KUANG/11
   package is needed for `get assistant`, `inspect assistant`, `help ask assistant` or
   `ask assistant` to resolve; without a usable model connection, inference fails with a structured
   setup error (v0.10 Gate AM).
2. **`Assistant.plugin`.** v0.2 §31.42's `plugin: PluginId` becomes optional contributor metadata.
   The built-in assistant carries origin `core` and no plugin id; no package id is fabricated for
   it. The exact field shape (optional field or origin record) is chosen in the `assistants.v1`
   contract change of #266/#267, which keeps existing serialized references readable where
   feasible (v0.10 §5.1).
3. **`ask assistant` without an id.** `ask assistant "<request>"` addresses the built-in assistant.
   The explicit verb-target form is the explicit selection v0.2 §7 asks for: the operator names
   the assistant domain, and unknown shell input still never reaches a model (v0.2 §31.42, v0.10
   §4.5). `ask assistant <name> "<request>"` stays valid where a named context/profile exists and
   resolves to the same runtime; ambiguity follows the interactive-picker/non-interactive-error rule
   (v0.10 §4.4).
4. **The `assistant` contribution kind.** It remains in `contributions.v1.yaml` and is read as a
   declarative context/profile plus optional assistant-facing metadata (descriptions, argument
   hints, context-selection suggestions, lower-trust knowledge — v0.10 §24.3), executed by the Core
   runtime. A package never needs to contribute one for its typed commands and objects to become
   eligible tools (v0.10 §24.2, Gate AO). A contribution that would need its own execution engine,
   its own policy enforcement or a privileged prompt is refused at registration (v0.10 §24.3,
   Gate AR).
5. **Model requirements.** A package or profile may state model requirements; the requirements
   narrow candidates inside the operator's `ModelPolicy` and never select a provider on their own
   (v0.10 §8.2, §24.3).
6. **Governance.** Every assistant operation, whatever the package that contributed the tool,
   runs through Core's capability broker, effective Security Scope and ChangePlan bridge
   (v0.10 §23.6; Security Specification 01 §12.1, §12.4; ADR-1035).

## Consequences

- `assistants.v1.yaml`, `contributions.v1.yaml` (`AssistantRegistry` doc), `commands/kuang.yaml`
  (`ono.assistant.get` summary, `ono.assistant.ask` `id` selector and its example
  `ask assistant ops-assist ...`) and `targets.yaml` (`ono.target.assistant`) change in #266/#267,
  contract first (AGENTS.md §9 rule 3). Until then the shipped behaviour of v0.6.x stands:
  acceptance case 045's `get assistant` table and `ask assistant nobody` refusal stay as they are.
- #276 ships a reference *Core* assistant proof and an optional capability-extension fixture; it
  ships no reference assistant package.
- A model-provider package that also contributes typed operations does so through the ordinary
  KUANG/11 contracts, independently authorized (v0.10 §24.4).
- Revisit if a later specification reintroduces package-owned assistant runtimes; v0.10 §31 and
  §38 reject them for v0.10.

## Alternatives considered

- **Ship the built-in assistant as a bundled, pre-installed package.** Rejected: v0.10 §0.3 rule 1
  and Gate AM require the assistant to exist "even with no plugin installed", and a bundled
  package would make its removal or a failed package load remove the assistant.
- **A reserved pseudo-package id such as `ono.core` in `Assistant.plugin`.** Rejected: it
  fabricates an installed package (v0.10 §5.1) and makes `inspect plugin` answer for something no
  package lifecycle governs.
- **Keep `ask assistant` id mandatory and add a default alias.** Rejected: v0.10 §4.7 keeps aliases
  out of the canonical interface, and §4.1 makes `ask assistant "..."` canonical.
