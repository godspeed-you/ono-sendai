# ADR-0111: Capabilities, the audit trail, and the assistant, model and finding tables

- Status: accepted
- Date: 2026-08-27
- Spec refs: §31.16–§31.19, §31.24, §31.37, §31.41–§31.43, §31.80; ADR-0022 §13, ADR-0040, ADR-0068, ADR-0107
- Decided by: agent (autonomous)

## Context

The broker inside the supervisor already evaluates a policy on every call and records every
decision in a per-instance trail (ADR-0040), but the policy was fixed at load and nothing in the
shell could show the trail, list grants, or change them. `get capability`, `grant capability`,
`revoke capability` and `get audit` are declared in `kuang.yaml`; so are `get assistant`,
`get model`, `ask assistant` and `get finding`, whose subsystems (the model broker, assistant
packages, analyses) are later increments of Phase I.

## Decision

### 1. Grants are host state; the broker's policy is derived from them

The host (ADR-0107) keeps every grant this session made: identity, package, capability, the
scope the manifest asked for, the declaration class, the source (`session` for `--grant` at
load, `prompt` for `grant capability`), and its revocation time. A revoked grant is retained
rather than deleted, as `ono.capability-grant/1` requires. The broker policy of a package is
the fold of its standing grants over `Policy::deny_all()`; `LoadedPlugin::update_policy`
(new in the supervisor) replaces the actor's policy, effective at the package's next call —
a running invocation is never interrupted, a later one is denied.

- `grant capability <capability> --plugin <id>` runs in the evaluator (it answers the grant
  record, not an action row): an unknown capability is `resolve.target_not_found`, so is an
  unknown package; the grant takes the manifest's scope for that capability, never wider.
- `revoke capability <capability | grant id> --plugin <id>` binds through the mutation road:
  the `selector` resolves the standing grants by capability or id, `--plugin` narrows to one
  package, and `act` marks the grant revoked and updates the instance.
- `get capability`: without `--plugin`, the definitions the broker knows — one row per family
  of `kuang_capabilities`, `decision: deny`, `source: default`, package fields null, because a
  definition is not a grant and pretending otherwise would be worse than a null — followed by
  every installed package's rows; with `--plugin`, that package's declared requests merged with
  its grants (one row per capability, `decision: allow` where a grant stands) and any grant
  of a capability it did not declare (`class: runtime-requested`). `enforcement` is the
  negotiated contract's where an instance runs, `broker` otherwise: the broker checks every
  capability call.

### 2. The audit trail is the instances' trails plus the host's own events

`get audit` reads every running instance's trail and what the host retained: the trail of an
instance that was unloaded, replaced or removed, and the host's own events — a load
(`lifecycle.load`), a grant, a revocation — under the invocation `host`. `--plugin`,
`--capability` and `--since` filter. A denial is a record like a success (spec §31.37);
the K11 error inside it is an `ono.error/1` with its own code (ADR-0108).

### 3. Assistants, models and findings are typed, empty tables

`get assistant`, `get model` and `get finding` answer from `ono.shell` with their contract's
schema and no rows: no assistant package is loaded, no model provider is configured, no
analysis has run. Empty is the honest answer, and typed means `where severity == "high"` and
`where sevrity` (E0202, with the suggestion) behave exactly as they will once rows exist.
`ask assistant <id> …` runs in the evaluator and answers `resolve.target_not_found` naming the
assistant, since spec §7.1 makes the assistant explicit and none can be resolved. The model
broker and the assistant role stay Phase I remainders (ADR-0040).

## Consequences

- `crates/ono-cli/tests/plugins_missing.rs`: `should_list_capability_definitions`,
  `should_show_a_grant_made_at_load_for_the_package`,
  `should_grant_and_revoke_a_capability_at_runtime`,
  `should_refuse_to_grant_an_unknown_capability`,
  `should_record_a_capability_use_in_the_audit_trail`,
  `should_record_a_denied_capability_use_in_the_audit_trail`,
  `should_filter_the_audit_trail_by_package`,
  `should_reject_an_unknown_field_on_the_audit_stream`,
  `should_report_no_assistants_when_none_is_loaded`,
  `should_report_no_model_providers_when_none_is_configured`,
  `should_report_a_structured_not_found_when_asking_an_unknown_assistant`,
  `should_report_no_findings_when_nothing_was_analysed`,
  `should_reject_an_unknown_field_on_the_finding_stream`.
- Grants live for the session (`duration: session`); `always` grants, leases with expiry and
  use counts, and `--scope`/`--duration` on `grant capability` are accepted by the contract
  and not yet honoured — a later increment with the on-disk policy store of spec §31.19.
- `inspect plugin`'s `capability_grants` stays empty until the inspection reads the host's
  grants; `get capability --plugin` is the inspectable form spec §31.18 asks for.

## Alternatives considered

- **Definitions as a separate target.** `kuang.yaml` and `targets.yaml` put definitions,
  requests, grants and leases on one target; a second target would be undocumented surface.
  Rejected.
- **Reload the instance on grant/revoke.** Would cancel whatever the package holds and re-run
  negotiation for a change the broker can apply at the next call. Rejected.
- **Refuse `get assistant` and friends as unimplemented.** A stream that will exist should
  compose today; an E0402 would make every script written against it wrong twice. Rejected.
