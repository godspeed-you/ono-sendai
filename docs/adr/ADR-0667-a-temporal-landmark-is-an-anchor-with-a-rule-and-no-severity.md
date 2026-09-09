# ADR-0667: A temporal landmark is an anchor with a rule and no severity

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §27.1, §27.2, §27.3, §18.4, §43.4, §33; v0.4 §3.7, §26.2
- Decided by: agent (autonomous)

## Context

§27.1 lists eleven built-in temporal landmark candidates, §27.2 forbids the shell from claiming
"operational severity beyond the underlying rule", and §27.3 requires a landmark to keep its event
reference. v0.4 already has a `Landmark` with a `LandmarkReason` — fourteen spatial reasons about
objects being *interesting now*. The temporal ones are about *transitions*, and the two vocabularies
have no overlap worth merging.

Two of §27.1's lines are not one anchor each. "service failure/recovery" is two things a reader
navigates to separately; "container start/stop/failure" is one lifecycle transition seen from three
sides. Two of them also need a number the spec does not give: a restart *loop* is a restart repeated
often enough, and "often enough" has to be stated somewhere.

## Decision

**`TemporalLandmarkKind` is twelve kinds over §27.1's eleven lines, and `TemporalLandmark` carries
no severity field.**

1. `ServiceFailure` and `ServiceRecovery` are separate: they are separate destinations.
   `ContainerLifecycle` stays one kind, because start, stop and failure are one transition.
2. The type has `kind`, `event`, `at`, `scope`, `subject` and `detail`, and nothing that ranks it.
   §27.2 is enforced by there being no field to claim severity in; `detail` is the rule's own words
   about what it saw. A test asserts the rendered record has no `severity` and no `priority`.
3. `event()` is the `EventId`, so `why event`, `at event` and `map --at event` all reach it
   (§27.3). A landmark is a way of navigating rather than a notification.
4. Each event yields at most one anchor, by the first rule that matches in §27.1's order, so a
   failing container is a container lifecycle anchor and not also a service failure.
5. The restart loop needs thresholds, so `LandmarkRules` states them: three entries into a start-up
   state within five minutes, matching §33's checkpoint cadence. One anchor is emitted at the
   instant the threshold is crossed rather than one per restart — a unit that keeps restarting is
   one thing to navigate to (§43.4).
6. A coverage boundary whose source is `ono.recorder` is a `RecorderGap`; a remote scope's is a
   `RemoteLink`; anything else is a `CoverageBoundary`. "Nobody recorded this" and "systemd was not
   running" are different facts and §7.5 already keeps them apart.
7. The rules read events in presentation order and consult nothing else, so the same events give
   the same anchors on any machine.

## Consequences

- The temporal vocabulary lives in `ono-temporal-query` beside the rules that emit it, and v0.4's
  `LandmarkReason` is untouched. A renderer showing both shows two lists, which is what they are.
- §18.4's stepping puts `LandmarkChange` fourth, and that class is keyed on the `landmark.added`
  and `landmark.removed` event kinds rather than on this module, so a landmark recorded in the
  ledger and a landmark computed from events rank the same way.
- A rule that needs a threshold has to add it to `LandmarkRules`, which keeps the numbers in one
  place and out of the rule bodies.
- Encoded by `crates/ono-temporal-query/tests/timeline.rs`.

## Alternatives considered

- **Extending v0.4's `LandmarkReason`.** It is a `#[non_exhaustive]` enum about present-state
  interest, and adding transitions to it would make `look`'s landmark list and the timeline's
  anchor list the same list, which they are not.
- **Eleven kinds, with a direction in `detail`.** Faithful to the line count and it forces a
  renderer to parse prose to tell a failure from a recovery.
- **A severity or priority field.** Precisely what §27.2 forbids, and the thing that turns a
  navigation aid into an alerting system nobody asked for.
