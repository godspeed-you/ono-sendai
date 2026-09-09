# ADR-0815: `require` outranks `maximize`, and the narrowing is reported

- Status: accepted
- Date: 2026-09-10
- Spec refs: v0.6 §17.2, §17.3, §53, §2.4, §56.3
- Decided by: agent (autonomous)

## Context

v0.6 §17.2 lists four protection modes — `off`, `prefer`, `require`, `maximize` — and defines each
one on its own terms:

> **require** — Refuse to apply if required mutation domains cannot reach the plan's required
> protection class.
>
> **maximize** — Attempt all non-conflicting available protection mechanisms that improve recovery
> coverage within configured cost limits.

These are two different properties, not two points on one scale. `require` is about what happens
when coverage falls short; `maximize` is about how much coverage is attempted. Neither implies the
other: a `maximize` plan whose coverage still falls short proceeds, and a `require` plan takes only
the protection `prefer` would have taken.

§17.3 lets a plan override the configured mode, and §53 closes with the rule that decides what
happens when the two disagree:

> Configuration MUST NOT silently weaken explicit plan requirements.

Applying that rule needs an order over the modes, and `require` and `maximize` do not have one.

## Decision

**The four modes are totalised as `off` < `prefer` < `maximize` < `require`, and
`ono_change_protection::policy::mode_rank` is the single place that order is written.**
`effective_mode` returns the higher-ranked of the configuration's mode and the plan's, so a plan
that asked for more than the configuration allows keeps its request.

`require` sits at the top because refusing is the failure-closed answer, and failing closed is what
§2.4 and §56.3 choose everywhere else in v0.6: an unestablished recovery property is not coverage,
and a fact that could not be established blocks rather than being guessed at.

**A plan whose requested mode is not the mode it runs under is told so.**
`ProtectionPolicy::narrowed` returns the mode the plan asked for whenever it differs from the
effective one, and `narrowing_note` is the sentence an operator sees. The only case that can
produce one is `maximize` requested under a `require` configuration — the one pair the order
totalises rather than compares — and it is exactly the case that would otherwise be silent.

## Consequences

- Everything that ranks modes reads `mode_rank`. `xtask/src/change.rs::check_authority` drives
  `effective_mode` over all sixteen mode pairs and fails the gate if any of them returns something
  the order calls weaker than what the plan requested, so a second ordering cannot appear
  unnoticed — which is how this ADR came to be written.
- A plan sealed `--protection maximize` under a `require` configuration attempts the protection
  `require` attempts, not every mechanism that fits. It refuses on a shortfall, which the plan did
  not ask for and which is stricter, and it says both things.
- `ProtectionMode::refuses_shortfall` stays true only for `require`. Making `maximize` refuse would
  be a rule §17.2 does not state, and inventing one is not open to this ADR.
- If a later release needs both properties at once, the shape is a second axis on
  `ProtectionPolicy` rather than a fifth mode: `required_level` is already stored separately, and
  the refusal could move onto it without touching §17.2's vocabulary.

## Alternatives considered

- **`maximize` strictest.** It attempts the most, so it looks like the maximum. It also proceeds
  when coverage falls short, which would let a `maximize` configuration cancel a plan's explicit
  `require` — the precise thing §53 forbids.
- **Refuse when the two are incomparable.** Honest, and it makes `--protection maximize` fail on
  any `require` host for a reason that has nothing to do with the change. §17.3 calls the override
  a MAY, not a conflict.
- **Give `maximize` the refusal too.** A total order for free, and a behaviour §17.2 does not
  describe. Rejected under §5.1: the deviation would be invisible to a reader of the spec.
