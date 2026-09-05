# ADR-0566: The model broker hands over a checked provider and a classified request

- Status: accepted
- Date: 2026-09-03
- Spec refs: §31.12, §31.16, §31.17, §31.19, §31.37, §31.41, §31.43, §31.44, §31.46, §31.52, §31.58, §31.67, §31.74, §31.82, §31.87, §17.5; ADR-0022, ADR-0264, ADR-0265, ADR-0268
- Decided by: agent (autonomous)

## Context

Issue #5, C-6: `ono-model-broker` did not exist. `get model` answered an empty list,
`assistants.v1.yaml` named `model_broker_protocol: ono-model/1` and defined nothing behind the
name, `protocol.v1.yaml` declared `models.list` and `models.infer` and the supervisor refused
both as calls the host API does not carry. The contracts were otherwise complete: the
`ono.model-provider/1` schema, the `model.infer` capability with its `providers` scope
(broker-enforced) and `data_class` scope (advisory), the data classes and origin labels, and
`K11601` / `K11602`. The issue carried a design; this ADR records the decisions in it and the
ones made building it.

## Decision

**1. Approval composes; there is no second vocabulary.** Operator approval for inference *is* a
`model.infer` grant with `--scope providers=…` and `--duration`, exactly as ADR-0264 built it.
The privacy plan of §31.82 is a disclosure written to the audit trail before the first remote
inference of an instance, not a second prompt. "Who approved" is the grant; "what leaves" is the
plan; one vocabulary.

**2. The crate cannot decide.** `ono-model-broker` has a catalogue, a policy that says allow,
transform or deny for a class, a request and response type, and a transport. It has no function
that returns a grant or a decision and does not depend on the supervisor or its policy module.
`ModelBroker::infer` takes a provider the caller already chose under a grant the caller already
checked, and a request the policy has already been applied to. That is the structural form of
`no-model-in-privileged-path`: the component that talks to models cannot reach the check it
would need to subvert. The supervisor does the choosing, checking, classifying and disclosing,
in that order, and only then calls the broker.

**3. `kind` declares where inference happens; the transport is a command.** One JSON document in
on standard input, one out on standard output, over a program the operator configured — no HTTP
client, so nothing untested ships. A `kind: remote` provider is a local bridge process; what it
does with the request is its business and the operator's. The wire is
`docs/contracts/kuang/model-broker.v1.yaml`.

**4. The policy is applied by the host, and a package's labels are the host's to set.** A package
may author `PLUGIN_KNOWLEDGE` and `UNTRUSTED_TEXT` and nothing else; a segment arriving with the
host's or the operator's label is relabelled, because a segment's label says where the content
came from (§31.52) and a package cannot speak as the host. A denied class refuses the whole
request with `model.policy_denied` naming the classes; a transformed class replaces the segment
with `[redacted: <class>]`; an unknown class is denied. The operator's `data_class` scope stays
advisory and is labelled so everywhere it is shown (`capabilities.v1.yaml`); the enforcement is
the provider's policy, which the operator wrote with the provider.

**5. `changed` is not the question here; the audit is.** Every request is audited under
`model.infer` whether it succeeded, was denied by the broker, was refused by the policy, or
failed in the provider — `everything-is-audited`. The disclosure is an audit record too, so
`get audit --plugin <id>` is where a plan stays inspectable.

**6. `get model` answers from `<config>/kuang/models.yaml`,** sibling of ADR-0265's
`policy.yaml`, read once per session when the host is configured. A file that cannot be read is
reported beside the rows, not as an empty catalogue.

## Consequences

- The example package gained three commands — `models`, `infer` and `inject` — so the
  conformance suite proves the surface under the deterministic test host: the scope filter on
  `models.list`, an answer through a configured command, a provider outside the scope refused
  and audited, a denied class refused with the classes named, a transformed class sent redacted
  and the plan disclosed once, untrusted text asking for a capability changing no grant, and a
  package with no configured model told so on the turn.
- A model call blocks the instance's actor loop for the turn's budget (defaulted to 30 s,
  clamped to 300 s). The plugin's other calls wait; the shell does not. Streaming responses are
  not delivered yet: `models.infer` answers with the parts in one reply, and the stream handle
  `protocol.v1.yaml` declares is the shape a later increment fills when a provider that can
  stream exists.
- The manifest's `compatibility.model_broker: ono-model/1` is what a package requesting
  `model.infer` declares; the example manifests carry it.

## Alternatives considered

- **An HTTP client with vendor adapters.** Rejected for this increment: it would ship a client
  nothing in the gate can exercise against a real endpoint, and §31.43's point is that the
  vendor is the operator's to choose. A bridge program is where a vendor lives.
- **Trimming denied segments and sending the rest.** Rejected: `assistants.v1.yaml` says why —
  silently trimming makes the boundary invisible.
- **A second approval prompt before the first remote inference.** Rejected in favour of the
  disclosure record: the grant is the approval, and a prompt inside a plugin's call would block
  terminal input (§31.67).
- **Letting the package classify its own segments downward.** Rejected: the host sets labels
  and may only raise a class; a package's declared class is honoured where it is at least as
  strict as the label implies, and the transport never sees a class the policy denies.
