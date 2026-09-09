# ADR-0676: The service rule explains a process event with the service event of the same direction

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §6.3, §7.4, §15.2, §15.8, §22.2; `docs/contracts/temporal/causality.yaml`;
  ADR-0627
- Decided by: agent (autonomous)

## Context

`docs/contracts/temporal/causality.yaml` declares `ono.service-controls-process` with
`input_event_kinds: [object.appeared, object.disappeared]`, required strengths of `derived` for
the appearance and `authoritative` for the disappearance, and an identity constraint that joins
through v0.4's `service.controls_process` edge. Its documentation says what it is for: *"Why a
process appeared or went away when nobody typed anything: the service that owns its cgroup was
doing something."*

A `CausalLink` joins two events. The row names the kinds both ends may take and the membership
that joins them, and leaves open which event stands at the cause end and whether an appearance may
explain a disappearance.

## Decision

The effect is the **process** event and the cause is the **service** event, and the two are of the
same kind: a service that appeared explains a process that appeared in its cgroup, and a service
that disappeared explains a process that went with it. The pairing across directions is not
emitted, because no evidence supports the sentence "the service went away, therefore a process
appeared".

The rule additionally requires:

- an `EvidenceClaim::RelationHeld` for `service.controls_process` from the service to the process,
  from `linux.systemd-dbus` or `linux.procfs`, whose `over` interval contains the process event's
  instant. The registry says this is *"a fact the coverage record has to support rather than an
  assumption"*, and an interval on the claim is how that fact is carried;
- the strength the row states for each end's own kind, which means the disappearance side is
  `authoritative`. §6.3 already forbids inventing a disappearance, and §7.4 makes absence a claim
  needing coverage capable of proving it; a causal explanation of a disappearance must not be the
  thing that launders a weaker observation into a stronger one;
- the process event at or after the service event, where the two share a clock domain.

## Consequences

- The rule is conservative enough to be silent in the common case where the service's own
  appearance was never recorded as an event, and `why process 2741` then falls to
  `ono.action-launched-process` or to `cause: unknown`. That is the intended failure direction.
- A restart, where a worker disappears and another appears, produces two links from two different
  service events rather than one link between the two processes. An explanation therefore says
  what the service did, which is the sentence the evidence supports.
- The direction pairing is an interpretation of a row that does not state one. If the lead reads
  the row differently, this ADR is the place the reading is written down and the place to change.

## Alternatives considered

**Pair any process event with any service event inside the membership interval.** Rejected: it
emits "the service disappeared, therefore a process appeared", which no evidence supports and
which a conservative engine should not be able to say.

**Take the cause from an `object.changed` on the service — its state transition — instead.**
Rejected as a change to the contract: `object.changed` is not among the row's declared input
kinds, and ADR-0627 makes the row the contract rather than a sketch. It is a plausible widening
for the lead to consider, and it would make the rule fire far more often against a real systemd
source; it is recorded here as a question rather than taken unilaterally.
