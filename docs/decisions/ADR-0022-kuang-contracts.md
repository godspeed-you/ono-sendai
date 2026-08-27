# ADR-0022: The KUANG/11 machine-readable contracts

- Status: accepted
- Date: 2026-08-26
- Spec refs: §31 (all of it; §31.5, §31.7, §31.8, §31.9, §31.10, §31.12, §31.13, §31.14, §31.15,
  §31.16, §31.17, §31.18, §31.19, §31.20, §31.21, §31.22, §31.23, §31.24, §31.25, §31.26,
  §31.27, §31.28, §31.31, §31.32, §31.33, §31.34, §31.35, §31.36, §31.37, §31.39, §31.40,
  §31.41, §31.42, §31.43, §31.44, §31.45, §31.46, §31.47, §31.48, §31.49, §31.50, §31.51,
  §31.52, §31.57, §31.58, §31.61, §31.62, §31.63, §31.64, §31.65, §31.66, §31.68, §31.70,
  §31.71, §31.72, §31.74, §31.75, §31.78, §31.79, §31.80, §31.81), §10.5, §11.5, §16.5, §22.1,
  §22.2, §27, §35.3, §37, §43, §50; ADR-0006, ADR-0009, ADR-0011, ADR-0012, ADR-0014, ADR-0015,
  ADR-0016, ADR-0020, ADR-0021
- Decided by: agent (autonomous)

## Context

Spec §31.78 asks for a machine-readable source tree under `spec/kuang/` — `docs/spec/kuang/` in
this repository (AGENTS.md §2) — and lists seventeen proposed file names with no schema for any
of them. Spec §31 itself is 2,450 lines of design prose, YAML fragments and terminal mock-ups. A
Phase I implementation has to be built against something checkable, and `cargo xtask spec-check`
has to be able to verify it, so the design has to become data.

Turning it into data forced a set of decisions the specification does not fix, and turned up two
places where the specification contradicts itself and one where it contradicts a registry
ADR-0012 already wrote. ADR-0012 is the model for the conventions; this ADR is the KUANG/11
equivalent, and it exists for the same reason: a later agent reading any one of these files needs
one place that explains why it looks the way it does.

Nothing in `docs/spec/kuang/` describes an implementation. It describes what Phase I builds.

## Decision

### 1. Eight contract files plus a schema directory, not §31.78's seventeen

`docs/spec/kuang/` holds `manifest.v1.yaml`, `capabilities.v1.yaml`, `protocol.v1.yaml`,
`lifecycle.v1.yaml`, `contributions.v1.yaml`, `assistants.v1.yaml`, `errors.v1.yaml`, and
`schemas/*.v1.yaml`. Spec §31.78's list is labelled "Proposed repository contracts" and splits
along section boundaries rather than along the boundaries a reader or a checker uses. The
consolidation is:

| §31.78 file | Here |
|---|---|
| `package.schema.yaml`, `manifest.schema.yaml` | `manifest.v1.yaml` |
| `capabilities.yaml` | `capabilities.v1.yaml` (scope shapes and lifecycle; the family list stays in `docs/spec/capabilities.yaml`) |
| `host-api.yaml`, `value-protocol.yaml` | `protocol.v1.yaml` |
| `plugin-states.yaml`, `trust.yaml` | `lifecycle.v1.yaml` |
| `registries.yaml`, `view-protocol.yaml` | `contributions.v1.yaml` |
| `model-broker.yaml`, `assistant.schema.yaml`, `context-policy.yaml`, `autonomy.yaml` | `assistants.v1.yaml` |
| `errors.yaml` | `errors.v1.yaml` |
| `finding.schema.yaml`, `recommendation.schema.yaml`, `audit.schema.yaml` | `schemas/*.v1.yaml` |
| `examples/packet-eye/`, `pg-surgeon/`, `ops-assist/` | not written; see §16 below |

The `.v1` suffix follows ADR-0012's schema-file convention, so a breaking change to a contract
takes a new file rather than being edited in place. Every file carries `version: 1` and a header
naming the spec sections it derives from, as ADR-0012 §1 requires.

### 2. The family list stays in `docs/spec/capabilities.yaml`; this directory adds the shapes

ADR-0012 §11 already put §31.16's twenty-nine capability families in `kuang_capabilities`, with
`risk`, `elevation` and the scope *keys*. Duplicating them would create two places to keep
honest. `capabilities.v1.yaml` therefore adds only what that registry deliberately omits — what
each family permits, the *shape* of each scope key, whether the host can enforce it, and the
grant lifecycle — and declares `docs/spec/capabilities.yaml` authoritative where the two could
ever disagree. `spec-check` compares them on id, risk, elevation and scope-key list.

### 3. A scope declares whether it is enforceable, and `model.infer`'s `data_class` is advisory

Spec §31.16: "A scope that cannot be enforced reliably MUST NOT be offered as if it were a
security boundary." A rule like that cannot be honoured by a file that lists scopes without
saying which ones are real. Every scope key therefore carries `enforcement: broker | advisory`:

- **broker** — the capability broker checks the scope on every host call, against the value the
  operation will actually use. Denial is `capability.scope_violation`. Only this level may be
  presented as a security boundary.
- **advisory** — recorded, audited and shown, and labelled advisory on every surface that shows
  it.

Exactly one scope key in the model is advisory: `model.infer`'s `data_class`. The transport
decision is enforced — a value classified `secret` is not sent to a provider that denies it —
but the *completeness* of the classification is not, and spec §31.44 says so itself: "The
classification system is a guardrail, not magical DLP." Three broker-enforced scopes carry a
`caveat` instead of a downgrade, because what they bound is genuinely narrower than a reader
might assume: `process.exec`'s `programs` and `container.exec`'s `containers` bound what starts,
not what the started thing then does, and `network.connect`'s `hosts` is checked against the
resolved address rather than only the name.

### 4. `history.write` has no scope

Spec §31.7's manifest example writes `history.write: {scope: plugin}`, and
`docs/spec/capabilities.yaml` gives `history.write` no scope keys. The two are reconciled rather
than one overruling the other: every history entry a package writes is attributed to its package
id by the host, and a package cannot write an entry attributed to the operator or to another
package. `scope: plugin` is therefore the only behaviour available, which makes it attribution
and not a grant scope. Recording it as a scope would offer a choice that does not exist.

### 5. The fourth lifecycle state is `active`, and there are six states, not four

Spec §31.8's heading says "Install, enable, load and run are different states". Its state machine
writes `ACTIVE`, and its Definitions list defines **Installed**, **Enabled**, **Loaded**,
**Active**, **Degraded** and **Quarantined**. It never defines "running". The canonical name is
therefore `active`, and the enum has six values, because §31.8's own `get plugin` example shows
`degraded` and `quarantined` in the same STATE column as `loaded` and `enabled` — they are
states, not modifiers on a state.

This contradicts a registry that already exists: `docs/spec/commands/kuang.yaml` documents
`get plugin --state` as "one of the states of spec §31.8: installed, enabled, loaded, running".
That option's documentation must be corrected to `installed, enabled, loaded, active, degraded,
quarantined`. It is outside this increment's file scope; it is recorded here and in the report so
it is fixed rather than discovered.

### 6. `ono.plugin/1` is identified by `[id, version]`

Spec §31.5 makes `package.id` immutable and unique, which reads like an identity of `[id]`. But
§31.35 wants side-by-side installed versions with one active per scope, and §31.81's removal
lists "package versions 2.4.1, 2.3.0" for one package. An identity of `[id]` cannot represent two
installed versions without making them the same object, which would make an upgrade's
side-by-side window unrepresentable exactly when an operator most wants to look at it.
`ono.plugin/1` therefore carries `active_version: bool`, and `get plugin` shows one row per
installed version.

### 7. Two host API domains beyond §31.12's list

Spec §31.12's sixteen domains are labelled "Proposed domains", and §31.61's interface sketch
names two more: `interface capabilities { check, request-once }` and
`interface output { emit, finding }`. Both are needed and neither fits an existing domain —
checking a grant is not an `objects` call, and emitting a finding is not a `streams` call.
`protocol.v1.yaml` therefore declares eighteen domains, with the two additions marked as coming
from §31.61.

### 8. Flow control is pull-based in both directions

Spec §31.15 requires bounded event queues and lists five overflow policies, but does not say how
values move. They move only when the consumer has asked for them: a plugin calls `streams.next`
with a credit, and the host calls `stream.demand` with a credit for the plugin's output streams.
The host never pushes unsolicited values and a plugin cannot emit beyond its credit — an attempt
is `runtime.protocol_violation`, not a queue.

This makes boundedness structural rather than a policy someone has to remember to apply, and it
is what makes `block-upstream` implementable at all: a plugin that stops asking has already
stopped the producer. It also gives the fuzzing requirement of ADR-0015 T7 something finite to
work against, since every frame is length-declared and every queue has a ceiling that is part of
the negotiated contract.

### 9. The host API version is `kuang-host/11.1`

Spec §31.5's manifest example writes `kuang_api: ">=11.1 <12"` and §31.63's negotiated contract
shows `kuang-host/11.3`. `11.1` is the lowest version the spec names, so it is the one these
contracts describe; the `11` is KUANG/11's major and does not move, and the minor is what
negotiation resolves. All seven version dimensions of §31.62 are declared independently in
`protocol.v1.yaml`, and the manifest carries them as separate optional fields so that a package
contributing no view has nothing to say about `ono-view/1`.

### 10. The manifest fails closed per section, not per field

Spec §31.7: "Unknown mandatory fields MUST fail closed. Unknown optional fields MAY be retained
and ignored with a diagnostic depending on compatibility rules." A per-field rule is
unimplementable for a field nobody has heard of — the manifest cannot know whether an unknown key
was meant to be mandatory. Each section therefore carries `closed: true | false`: an unknown key
in a closed section invalidates the manifest, and an unknown key in an open section is retained,
ignored and reported. Every section that carries authority — `package`, `compatibility`,
`runtime`, `capabilities`, `state`, `network`, `remote`, `assistant` — is closed.

`network` is additionally required even when the answer is `none`, so that the absence of network
access is something a package states rather than something a reader infers from a missing
section.

### 11. Assistant authority is a list of invariants with an enforcer and a test

Spec §31.41's "The model reasons; Ono observes and acts" and AUTONOMOUS_IMPLEMENTATION.md §15's
list of what Ono retains authority over are both prose, and prose is not a contract. They are
recorded in `assistants.v1.yaml` as `authority_invariants`: eleven entries, each with the rule,
the spec reference, the **host** component that enforces it, and the conformance case from
§31.74 that proves it. No invariant is enforced by the assistant package and none by the model,
because an invariant a plugin enforces on itself is a promise rather than a boundary.

Two consequences worth stating separately, because they are where an implementation would
otherwise drift:

- **The tool set is computed, not declared.** What is exposed to a model for a turn is the
  package's declared tools intersected with its current grants and the effective autonomy level.
  A tool the assistant cannot pay for is never offered, so the model cannot propose it and the
  operator never has to refuse it.
- **The effective autonomy level is `min(declared, policy)`**, and L4 additionally requires an
  active lease. Without one, L4 behaves as L3 — so a lease's expiry silently *reduces* authority
  instead of silently keeping it. There is no level above L4 and none may be added (spec §31.48).

### 12. Three schemas beyond the eleven `deferred.yaml` lists

`docs/spec/schemas/deferred.yaml` defers eleven KUANG/11 schemas. Writing them turned up three
types the spec names without defining and the deferred list does not carry:
`ono.evidence/1` (§31.24's `List<Evidence>`, §31.25, §31.26, §31.50), `ono.recommendation/1`
(§31.24's `List<Recommendation>`, and §31.78's own `recommendation.schema.yaml`), and
`ono.assistant-action/1` (§31.47's proposed action list). Each is written in full rather than
typed as a bare `record`: ADR-0012 §13 rejects that as making the registry parse cleanly while
discarding the contract, which is the failure mode `spec-check` exists to catch.

### 13. Error kinds for the K-family

Spec §31.79 gives twenty-seven codes with dotted names and no kinds. ADR-0006's kind set is
closed at twelve, so each code takes exactly one of them:

| Family | Kind | Why |
|---|---|---|
| `package.invalid` | parse | A manifest that fails validation is input that could not be read as a `kuang-package/1` document. |
| `package.incompatible`, `load.dependency_cycle` | conflict | The package's requirements conflict with what exists. |
| `package.integrity_failed`, `package.signature_invalid`, `publisher.untrusted`, `state.quota_exceeded`, `model.policy_denied`, `remote.policy_denied` | safety | A policy or an integrity requirement stopped it. Same reasoning that put `remote.host_key_changed` under `safety` in ADR-0006. |
| `load.capability_denied`, `capability.*`, `assistant.context_denied` | permission | Understood, not permitted. |
| `load.dependency_missing` | resolution | A name did not resolve. |
| `load.runtime_unavailable`, `runtime.protocol_violation`, `runtime.schema_violation`, `view.protocol_error`, `model.provider_unavailable`, `remote.extension_unavailable` | provider | A provider could not answer, or answered outside its advertised contract. A plugin is a provider. |
| `runtime.trap`, `runtime.memory_limit`, `state.migration_failed` | external | External code failed or was terminated. |
| `runtime.timeout` | timeout | — |
| `runtime.backpressure_failure` | stream | The operation is not valid for a stream with these properties. |
| `assistant.tool_invalid` | type | A call did not fit the shape its contract declares — the same reading ADR-0021 §4 gives `type.mismatch`. |

`runtime.memory_limit` is `external` and `state.quota_exceeded` is `safety` deliberately: the
first terminates the instance, the second refuses a write and leaves everything running.

### 14. Contributed relations use `ono.graph-edge/1`

See the `Spec deviation` heading below. The mapping is written out in `contributions.v1.yaml`
→ `relation.field_mapping`, so nothing §31.26 carries is lost.

### 15. `runtime.kind` names the tier by what it is, and T0 is not declarable

Spec §31.10's tiers are `T0 core-built-in`, `T1 trusted-native`, `T2 isolated-component`,
`T3 remote-service`. A manifest declares `wasm-component`, `native-process`, `remote-service` or
`declarative` — what the artifact *is*, which is checkable — and the host maps that onto the
tier, which is a judgement about containment. `T0` has no manifest spelling: spec §31.10 reserves
in-process dynamic libraries for core-shipped code, and a package that could ask for T0 would
make that reservation advisory.

### 16. No `examples/` directory

Spec §31.78 lists `examples/packet-eye/`, `pg-surgeon/` and `ops-assist/`. They are not written
here. An example package is only worth having if it is executed — spec §31.73's test host runs
fixtures, and §31.74's conformance suite runs cases — and neither exists before Phase I. Writing
three example manifests now would produce three files nothing validates, which is the kind of
speculative artifact AGENTS.md §4 rules out. They belong to the increment that lands the test
host, where they become the fixtures it runs.

## Spec deviation

### The rendered form of the KUANG/11 error codes

- Section: spec §31.79
- Text: "`ONO-K11001 package.invalid`" (and the twenty-six codes that follow it, all spelled
  `ONO-K11nnn`)
- Instead: the codes render as `Ono-Sendai-K11001` … `Ono-Sendai-K11702`. The dotted `name` of
  each — `package.invalid`, `capability.scope_violation`, and so on — is kept exactly as §31.79
  gives it.
- Why: spec §43 spells every other code in the product `Ono-Sendai-E0001`, and
  `docs/spec/errors.yaml` implements that spelling. §31.79 says these codes "should integrate
  with the global structured Error model"; two rendering conventions inside one error registry is
  not integration, it is a seam a user meets the first time an extension fails. Nothing
  scriptable changes: ADR-0006 makes the dotted `name` the thing `catch` and `where` match on,
  and that is unchanged.

### The record for a contributed relationship

- Section: spec §31.26
- Text: "Edge contract: `Relation { from: ObjectRef, type: RelationType, to: ObjectRef,
  direction: directed | undirected, evidence: List<EvidenceRef>, confidence: Float?, observed_at:
  Timestamp, expires_at: Timestamp?, source: ProviderOrPluginId }`"
- Instead: a contributed edge is `ono.graph-edge/1` — the schema of spec §22.1, written by
  ADR-0012 §9 — with §31.26's additional fields carried in its `metadata` record.
  `Relation.confidence`, a float, maps onto `GraphEdge.confidence`, the closed enum
  `exact | inferred`: `1.0` or absent on a kernel-derived edge becomes `exact`, anything else
  becomes `inferred`, and the numeric value is retained in `metadata.confidence` so nothing is
  discarded. `Relation.source` becomes `GraphEdge.provider` and is set by the host.
- Why: the specification describes one concept twice. §22.1 gives `Edge` and §31.26 gives
  `Relation`, and they disagree on the type of `confidence` — which is not a detail, because
  §22.2 requires that Ono "MUST distinguish exact relationships from inferred ones" and that the
  UI "must not visually imply certainty that the provider does not possess". A closed enum makes
  that distinction unavoidable; a nullable float makes it a rendering decision, and every
  renderer would have to pick a threshold nobody wrote down. Two edge records would also mean a
  contributed edge could not appear in the same `Graph` as a core one, which contradicts §31.26's
  own premise that a package may add edges without owning either endpoint schema.
  §31.26's own rule survives intact and is quoted in the contract: "Inferred relations MUST
  identify themselves as inferred."

## Consequences

Easy: Phase I has a target. `manifest.v1.yaml` is what the manifest parser is tested against,
`protocol.v1.yaml` enumerates every message the supervisor and the SDK must implement,
`lifecycle.v1.yaml` is the state machine a test asserts, `capabilities.v1.yaml` is what the
broker enforces, and the fourteen schemas are what the KUANG/11 commands in
`docs/spec/commands/kuang.yaml` already promise to emit. Spec §31.77's ambition — that a
code-generating agent can implement a plugin work package from the contracts without inventing
the public interface — has something to generate from. ADR-0015 T3 acquires a concrete surface:
its denial paths are the `enforcement` field, the precedence order and the conformance cases.

Hard: eighteen domains, roughly ninety messages and fourteen schemas are a large surface written
before any of it runs, and some of it will be wrong. The mitigation is the same one ADR-0012
relies on — `spec-check` validates the cross-references mechanically (every capability id against
`docs/spec/capabilities.yaml`, every schema reference against the schema directory, every error
code against `errors.v1.yaml`, every command id against `docs/spec/commands/`) — plus the rule
that a contract is corrected by editing it, in its own commit, with the ADR amended by a
successor rather than in place.

Must be revisited, and each is named in the report for the orchestrator:

1. Eleven of the fourteen schemas belong under `docs/spec/schemas/`, and their
   `docs/spec/schemas/deferred.yaml` entries must be removed when they move. Three
   (`ono.evidence/1`, `ono.recommendation/1`, `ono.assistant-action/1`) are new and have no
   deferred entry to remove.
2. `docs/spec/commands/kuang.yaml`'s `get plugin --state` documentation says `running`; it must
   say `active`, and should list `degraded` and `quarantined` (§5 above).
3. `docs/spec/commands/kuang.yaml` has no `ono.plugin.set`, although spec §31.3 writes
   `set plugin pg-surgeon --enabled false` and §31.38 writes `set plugin packet-eye --background
   true`. Two lifecycle transitions name it. The registry needs the entry.
4. Spec §31.3's requirement — that if `install` and `verify` are accepted as core verbs they
   "MUST be added to the global verb registry rather than implemented as a private KUANG/11
   grammar" — is **already satisfied**: `docs/spec/verbs.yaml` carries `install`, `load`,
   `unload`, `verify`, `grant`, `revoke` and `ask` under its KUANG/11 heading, and `set` among
   the mutation verbs. Nothing is needed here; it is recorded so a later reader does not
   re-open it.
5. The KUANG/11 error family belongs in `docs/spec/errors.yaml` alongside the E-codes, or that
   file must reference `docs/spec/kuang/errors.v1.yaml` explicitly. One error registry that a
   reader has to know is in two files is a registry with a seam.
6. `examples/` (§16 above) arrives with the test host.
7. `docs/spec/providers/*.yaml` conformance generation (ADR-0012 §1) will need a KUANG/11
   equivalent for contributed providers, driven by §31.74.

Encoded by: `docs/spec/kuang/**`, and the `spec-check` cross-reference checks Phase D adds and
Phase I extends.

## Alternatives considered

- **Writing §31.78's seventeen files verbatim** — rejected: several would hold three lines, the
  split does not match how the contracts are read or checked, and §31.78 labels the list
  "Proposed". The mapping table in §1 preserves the correspondence for anyone looking for a file
  by its §31.78 name.
- **Repeating the capability families in `docs/spec/kuang/capabilities.v1.yaml`** — rejected:
  ADR-0012 §11 already made the deliberate choice to keep provider capabilities and KUANG/11
  capabilities in one file with two lists, precisely so nobody can mistake one for the other. A
  third copy would be a third thing to keep honest, and the one most likely to drift is the copy
  a security check reads.
- **Omitting `enforcement` and listing scopes as spec §31.16 does** — rejected: §31.16's own
  sentence forbids offering an unenforceable scope as a boundary, and a file that lists scopes
  without saying which are enforceable makes that sentence unimplementable. It would also make
  spec §31.80's last threat — "capability scope is broader in implementation than UI suggests" —
  something no contract could catch.
- **A second `Relation` record beside `ono.graph-edge/1`** — rejected: see the `Spec deviation`
  heading. Two edge types would fork the graph model at exactly the point where composition
  between core providers and packages is the feature.
- **Keeping `ONO-K11001` verbatim** — rejected: see the `Spec deviation` heading. The
  alternative reading is defensible — the spec writes it that way — but it puts the seam in the
  user's face rather than in a document.
- **Push-based streaming with a bounded queue and a drop policy** — rejected: it makes
  `block-upstream` (spec §31.15) impossible to implement honestly, and it makes boundedness a
  property of the host's queue rather than of the protocol, so a bug in one queue is an
  unbounded allocation instead of a protocol violation.
- **Modelling `degraded` and `quarantined` as flags beside a four-value state** — rejected:
  spec §31.8's `get plugin` example puts them in the STATE column, and a flat enum is what a
  `where state == degraded` predicate needs. A state plus a condition would mean every consumer
  reconstructing the display value the spec already shows.
- **Deferring `assistants.v1.yaml` to a later increment, since Phase I's own sub-phases put
  assistants at K11-G** — rejected: the authority invariants of §11 constrain the object API,
  the capability broker and the planner, all of which land in K11-C and K11-D. Writing them after
  those exist means retrofitting the constraint into the components that were supposed to enforce
  it, which is how a model ends up in a privileged path by accident.
