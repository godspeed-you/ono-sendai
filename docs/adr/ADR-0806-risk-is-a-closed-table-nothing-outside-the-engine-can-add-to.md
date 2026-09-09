# ADR-0806: Risk is a closed table, and nothing outside the engine can add a rule

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §19.1, §19.2, §19.3, §19.4, §28.3, §34.2, §40.2, §62.11, §49
- Decided by: agent (autonomous)

## Context

§19.2 says of the five risk classes: "These classes are rule-based, not AI-generated." §62.11 says
it again from the other side — AI may explain or propose, and may not upgrade unknown to
guaranteed. §49.3 says it a third time for the specific case that matters: a model's opinion that
a change is reversible does not change recovery coverage.

Three statements of one rule usually mean the specification expects it to be violated, and the
violation is easy to picture: a `RiskAssessment::with_class(Critical)` somewhere, or a plugin
contributing a class directly, or a renderer deciding that a plan with many targets "looks" high
risk.

§40.2 adds the reason it matters in practice. A gate must "summarize the actual risk reason, not
display generic 'Are you sure?'". A class with no rule behind it has no reason to show, and a gate
with nothing to say is a gate that teaches the operator to type the flag without reading.

## Decision

**Risk is composed, never assigned.** `RiskAssessment::classify()` folds `RiskClass::max_of` over
the findings and there is no constructor taking a class. A finding carries the id of the rule that
made it, the §19.1 dimension it found, the class, and the sentence §40.2 prints.

**The rule registry is closed at the crate boundary.** `ono_change_impact::rules()` returns
`&'static [RiskRuleSpec]`, and the evaluation function is a private field, so nothing outside the
crate can construct a rule. Fourteen rules ship, and `docs/contracts/change/risk.yaml` lists them
with their dimension and the class each may emit — bidirectionally checked, so a rule added in Rust
without a row fails the gate and a documented rule nothing implements fails it too.

**A contributed rule is still a rule.** §48.2 lets a KUANG/11 package contribute a `RiskRule`, and
that arrives as a registered, inspectable entry with an id, a dimension and a declared class,
subject to the same composition. It can raise the plan's class and cannot lower it, because
`max_of` has no inverse.

**`UNKNOWN` ranks between `MODERATE` and `HIGH`.** This is the one ordering the specification
leaves open, and both directions matter: a risk nobody could classify is worse than one that was
classified as ordinary, and better than one a rule found serious. §2.4's rule that unknown is never
quietly promoted works in both directions here, and putting `UNKNOWN` at the top would make every
plan with an opaque boundary demand the same acknowledgement as one that restarts a whole role.

The class assignments the specification does not fix are recorded here so a later reader can argue
with them rather than guess at them:

| Rule | Class | Why |
|---|---|---|
| single-object mutate | MODERATE | So `LOW` means "changes nothing", which is what makes §19.3's first example — one config file, one restart — come out MODERATE rather than LOW. |
| irreversibility | HIGH | §19.3's `SIGKILL` example is HIGH, and irreversibility is the dimension that makes it so. |
| external side effect | HIGH | §35.1's effects have left the machine; the alternative reading is MODERATE, and this is the more arguable of the two below. |
| reboot requirement | HIGH | §30.5 distinguishes a requirement from a suggestion, and a requirement is an outage. Also arguable. |
| whole role, link loss | CRITICAL | §19.3's forty frontend nodes and §34.2's own word. |

## Consequences

`explain plan` can list every rule that fired with the sentence it fired for, and a gate shows the
leading findings rather than a number. §40.2's worked gate — "18/18 frontend nodes will restart. No
healthy serving member is excluded." — is a finding's reason, not a template.

Whole-role membership is derived from the v0.4 topology (`service.in_cgroup`,
`process.member_of_cgroup`, `container.contains_process`) and never from name similarity, so a plan
touching three of five members does not trip it. That was worth a test of its own.

A model can propose an intent and cannot contribute a finding. There is no path, and the KUANG/11
work asserts the absence rather than trusting it.

## Alternatives considered

**A configurable rule set in `docs/contracts/change/risk.yaml`, evaluated at run time.** Rejected:
a rule expressed as data needs a predicate language, and a predicate language over the plan is the
generic workflow engine §61 rules out.

**Letting a plugin contribute a class directly, without a rule.** Rejected: §40.2 needs a reason,
and a class with no rule has none.
