# ADR-0816: Nine judgements on the KUANG/11 change surface

- Status: accepted — corrected by ADR-0833
- Date: 2026-09-10
- Spec refs: v0.6 §12.2, §16, §18.4, §19.2, §27, §37.5, §39.2, §39.3, §43.4, §48; K11P §6.2, §7
- Decided by: agent (autonomous)

## Context

v0.6 §16 and §48 open the change and recovery model to KUANG/11 packages: a plugin may contribute
plan actions, prospective effects, impact edges and risk findings, and may register a recovery
provider of its own. The specification names the capabilities and states the rules the surface must
enforce; it does not say where in the protocol each rule is enforced, what a wire record's fields
are, or which consent class a grant belongs to. Nine of those gaps had to be closed to ship the
surface, and each was decided rather than derived.

None of them is large enough for an ADR of its own. Together they are the shape of the boundary,
and a later reader changing one of them needs to know it was a choice.

## Decision

**1. An `application-consistent` claim requires `recovery.quiesce`.**
A recovery-provider contribution declaring `consistency: application-consistent` without
`recovery.quiesce` in its `capabilities` is `package.invalid` at load. §39.2 forbids Ono labelling
state application-consistent unless an application-aware provider asserts the guarantee, and a
package that cannot pause the application has no mechanism by which the claim could be true. It may
claim `crash-consistent`, which it can own.

**2. `recovery.validate` is carried by `recovery.discover`, not by a capability of its own.**
§11.4's validation reads an asset and reports what it found. It mutates nothing, and giving it a
separate grant would make an operator answer a question about a read that the discovery grant
already covers. §12.2 names seven recovery capabilities and validation is not among them.

**3. `recovery.resume` is carried by `recovery.quiesce`.**
§18.4 requires the application to be resumed when snapshot creation fails, and makes a failure to
resume its own critical error. A package able to pause must always be able to let go; a separate
grant would make "still paused, and not permitted to release it" a reachable state.

**4. A contributed risk finding names the rule that produced it.**
`RiskFindingContribution` carries a dimension, a class, a rule and a reason, and the rule must be
one the package declared in its own `risk_rules` contribution. Carrying a class is not deciding one:
the host folds every finding with `RiskAssessment::classify`, which is a maximum. §19.2 defines no
minimum, so there is no path from a contributed finding to a lower plan class however low the
finding is.

**5. Every contribution record is `deny_unknown_fields`.**
`PlanContributeParams` and each wire record beneath it refuse a field they do not know. A plugin
built against a later protocol version that sends more than this host understands is told so, rather
than having the surplus silently dropped — which for a contribution is the difference between an
effect the host weighed and one it never saw.

**6. Four `HostServices` methods carry the whole surface.**
`change_plan_read`, `change_plan_contribute`, `recovery_report` and `verification_observe`. Each
defaults to a refusal in the trait, so a host that has not wired the change engine denies rather
than half-answers. Everything else a plugin needs — objects, relations, history — it already had.

**7. `change.plan.contribute` is consent class B with kind `local-contribution`.**
Not `host-change`: what it adds is bounded to the package's own declared rules and effect
vocabulary, and §19.2's maximum means a contributed finding can raise a plan's risk class and never
lower it. Contributing changes nothing outside Ono's own plan object, which is what
`local-contribution` describes (K11P §7.2).

**8. `RecoveryCostWire.estimated` defaults to `true` when absent.**
§37.5 requires cost figures to be labelled estimated wherever filesystem accounting is not exact. A
package that omits the field has not claimed exactness, and reading the omission as exact is the one
direction that could mislead.

**9. The surface refuses in two layers, at load and at the call.**
Load-time refusals are properties of the declaration and are checked before any code runs: a
provider offering a restore method without a `destructive`-risk capability (§48.4), an
`application-consistent` claim without `recovery.quiesce` (§39.2), a `transaction` naming a resource
outside the provider's own domain kinds (§27.3), a `transaction` without `recovery.transaction`
(§12.2). Call-time refusals are properties of the values: a capability the session does not hold, a
scope the broker can check and the call violates, a vocabulary word outside a closed list. A rule
that can be decided from the manifest is decided from the manifest, because a package that cannot
be trusted should not have been loaded.

## Consequences

- `recovery.quiesce` becomes the load-bearing grant of §16's application-aware providers: it
  authorises pausing, resuming, and the only consistency claim above `filesystem-consistent`. An
  operator who denies it gets a provider that still protects and still validates, and that says
  `crash-consistent` where it would have said more (ADR-0812).
- No first-party provider declares `recovery.quiesce`, and
  `xtask/src/change.rs::check_quiesce` is what keeps that true — §39.3's protocol is contract for a
  surface that has not shipped a user yet, and a first-party provider taking the capability without
  the protocol would be exactly the overstatement §39.1 forbids.
- `deny_unknown_fields` makes the contribution records a compatibility boundary: adding a field is a
  protocol version bump, and `docs/contracts/kuang/protocol.v1.yaml`'s `since:` is where it is
  recorded.
- A package cannot contribute a risk finding for a rule it did not declare, so §19.2's rule registry
  stays the complete list of what could raise a plan's class.

## Alternatives considered

- **A `recovery.validate` capability of its own.** More granular, and it splits a read from the read
  that motivates it. §12.2's list is seven, and lengthening it makes a permission prompt longer
  without making a denial more meaningful.
- **`recovery.resume` as a separate grant.** Symmetrical, and it creates the state §18.4 exists to
  prevent.
- **Allowing a contributed finding to lower a plan's class.** A plugin that knows a change is safe
  could say so. It could also be wrong, and §19.2's maximum is the shape that survives being wrong.
- **Ignoring unknown wire fields.** Forgiving across versions, and for a *contribution* it converts a
  protocol mismatch into a silently smaller plan.
