# ADR-0821: One ledger per process, and the subtype is visible

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §22.1, §22.4, §29.2; v0.5 §6.8, §10.2, §10.7, §17.2, §33
- Decided by: agent (autonomous)

## Context

v0.6 §22.1 records a plan's lifecycle as events on the v0.5 ledger — no second audit subsystem,
the seventeen canonical kinds of v0.5 §17.2 unchanged, and an `ono.plan.*` subtype saying which
lifecycle event each one is. Driving the shell showed three ways that was not true.

**The events were written to a ledger nobody read.** `writable_ledger()` answers with a published
handle; a session created its own and published it only when a recorder replaced it, so before any
temporal command ran, `plan` wrote into a `Ledger::default()` that was then thrown away. Worse,
configuration is lazy: the first temporal command applies `temporal.session.max_events` by
*swapping* the ledger, and the swap discarded whatever the previous one held. `plan … | apply` then
`find event` in one shell showed the apply's events and none of the plan's, and `plan` alone showed
nothing at all.

**`apply` recorded a second creation for one creation.** `PlanLifecycle::created` stages
`PlanCreated`, and `apply` used it as a constructor. §6.8 makes an event's identity a content
digest, so two `PlanCreated` events differing only by their instant are both kept.

**`PlanProtected` was never recorded by `apply`.** §4.5 creates the recovery assets during apply,
and only `protect` — the separate command an operator rarely needs — wrote the event.

And in the view: `timeline` printed five rows reading `action completed`, `action requested`,
`action authorized`. The subtype, which is the whole of what distinguishes them, was rendered for
`provider.event` and for nothing else.

## Decision

**There is one ledger per process, and everything shares it.** `TemporalState::new` adopts the
published handle rather than creating a second, and `set_shared_ledger` carries the events of the
ledger it replaces into the new one. §6.8's content-digest identity makes the carry-over
idempotent: an event already there is a duplicate, not a second event. A carry-over that fails is
not reported — it happens while a session is being configured, and a history shorter than it might
have been is not a reason to refuse the configuration.

**`PlanLifecycle::continuing` is the constructor for a run that did not create the plan.** `apply`
and `protect` use it; `plan` keeps `created`. One creation, one event.

**`apply` records `PlanProtected` for the assets it created**, which is where §4.5 creates them.

**`timeline` prints the subtype where an event carries one.** §17.2 closes the kinds and extends
them by `subtype`, so an event carrying one is saying something its kind alone does not. The kind
stays in the record and in `find event`; the subtype is the line a person reads.

## Consequences

- `plan … | apply` followed by `find event` in one shell answers with the seven events §22.1
  names: created, sealed, protected, action started, action completed, verification observed,
  verified — each once, each carrying the plan identity §22.4 anchors on.
- `crates/ono-cli/tests/change_timeline.rs` is the suite. It asserts the chain, that no kind
  outside v0.5's seventeen appears, that every plan event carries the identity, that one creation
  produces one event, that a shell which planned nothing has no plan history, and that the events
  reach `timeline` and not only the store.
- The ledger is still in-memory unless a recorder is running (v0.5 §10.2). A second process sees
  nothing, which is v0.5's design and not v0.6's to change: `start recorder` is what makes a
  plan's history outlive the shell that made it.
- Making the subtype the rendered word changes `timeline` for v0.5 events too — a provider event
  reads as its subtype, which is what its own case already did. Nothing else in the tree carries a
  subtype today, so this is a rule that will matter most for what comes next.
- `set_shared_ledger` now reads the old ledger in full. It is bounded by
  `temporal.session.max_events` (§10.7), so the cost is bounded by that setting rather than by how
  long the shell has been running.

## Alternatives considered

- **Configure the temporal session eagerly at startup.** It removes the swap, and §32.1 budgets
  startup with recording disabled at no filesystem cost — building the session before anything
  asks for it spends that budget on every `ono -c` that never touches time.
- **Have the change layer write through the session rather than the published handle.** The
  session is behind an async mutex and `plan` is claimed by the evaluator before the registry
  path (ADR-0814); threading it through would put a lock acquisition in the middle of sealing.
- **Render `kind` and `subtype` together.** Twice the width for one fact, and the kind is already
  in `find event` and in the record for anything that needs to group by it.
