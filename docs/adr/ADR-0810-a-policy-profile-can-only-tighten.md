# ADR-0810: A policy profile can only tighten, and the strictest requirement in force applies

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §17.2, §17.3, §17.4, §26, §40.3, §53, Appendix H, Appendix H.5
- Decided by: agent (autonomous)

## Context

Three places in v0.6 say the same thing from different directions, and none of them says it as a
single rule:

- Appendix H.5: "A plan may impose stricter requirements than a profile. A profile MUST NOT weaken
  provider-declared safety constraints."
- §53, closing line: "Configuration MUST NOT silently weaken explicit plan requirements."
- §17.3: a per-plan `--protection require` "MAY override configuration".

Read separately they are three special cases. Read together they are one rule with three sources —
configuration, profile, and the plan itself — where the answer is always the strictest.

The failure mode they guard against is specific and easy to reach: an operator sets the `cautious`
profile, a plan says `--protection off` for a reason that made sense at the time, and the plan wins
because it is more specific. That is exactly backwards from what a cautious profile is for.

## Decision

**One rule: the strictest requirement in force applies, whichever source states it.**

`ProtectionPolicy::tighten_with` is strictly monotone, and its ordering is:

- protection mode: `off < prefer < maximize < require`;
- retention windows and free-space floors: the maximum;
- cost caps (size, count, quiesce duration): the minimum;
- an absent cap loses to a stated one;
- an explicit hold survives everything.

`effective_mode` takes the stricter of configuration and plan **in both directions**, so §53's
closing line holds and a configuration that requires protection is not weakened by a plan that says
`off`. §17.3's "MAY override configuration" is honoured in the direction that tightens, which is the
direction an override is for.

Appendix H's four profiles are presets over the same settings, and they **expand to inspectable
values** rather than being a mode of their own — H's opening line requires it, and the registry
lists each profile's expansion.

Two supporting decisions, both from §17.2 and easy to get wrong:

- **`off` still reports what was available.** §17.2's own words: "Do not create automatic recovery
  assets. Still show the protection opportunities." `off` means "I will not take a snapshot", not
  "I will not tell you one was possible", and the analysis carries the rejected candidates with the
  reason each lost.
- **`maximize` is bounded by the plan's scope.** §17.2 and §62.7 both add the caveat, and it is the
  operative half: `maximize` MUST NOT mean "snapshot everything on the host". Protection follows the
  planned mutation scope, inside the §38.3 cost limits.

§26's auto-recovery sits under the same authority rule, and is the case where the strictest reading
is "off": §26.1 makes it off by default, and §26.3's six conditions must **all** hold. A declaration
that does not meet them is rejected at seal (`change.auto_recovery_rejected`), which is the moment
where rejecting it costs nothing, rather than at the moment it would have run.

## Consequences

`scripted` (Appendix H.4) is a profile rather than a branch in the code. §17.4 and §40.3 both
forbid a non-interactive run stopping for a question, and a policy that cannot be satisfied fails
with a structured error naming the flag that would satisfy it.

An operator cannot loosen a policy by being more specific, which will occasionally be inconvenient
and is what a policy is. The way to loosen one is to change it, which is a visible act.

`xtask/src/change.rs::check_policies` checks that every profile's declared settings are a tightening
of the defaults, so a profile that relaxed something would fail the gate rather than take effect.

## Alternatives considered

**Most-specific-wins, the usual configuration convention.** Rejected: it inverts Appendix H.5 and
§53 in the one case that matters.

**A separate `--force` that permits loosening.** Rejected: §19.4 already has the acknowledgement
mechanism for "I know and I want to proceed", and it is per-risk rather than per-policy, which is
the finer and more honest grain.
