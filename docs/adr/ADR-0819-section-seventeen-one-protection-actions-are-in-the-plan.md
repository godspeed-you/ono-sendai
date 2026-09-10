# ADR-0819: §17.1's protection actions are in the plan

- Status: accepted — corrected by ADR-0833
- Date: 2026-09-10
- Spec refs: v0.6 §2.1, §3.3, §4.5, §4.7, §5.2, §12.2, §12.3, §17.1, §46.1, Appendix E
- Decided by: agent (autonomous)

## Context

§17.1 says what the default policy means, as four things `prefer` does. The second is:

> include protection actions in the plan;

The implementation did the first, third, fourth and fifth — it discovered low-cost protection,
executed it during PREPARE, aborted before mutation when a required one failed, and fabricated
nothing. It did not do the second. The coverage matrix showed what *could* protect, the executor
worked from a parallel `Vec<ProtectionAction>` the analysis produced, and the plan's `actions`
list — the thing §5.2 and §46.1 make an operator's view of what will happen — held only the
mutating action.

The consequence was visible in the shell. Appendix E's progress display counts actions by role,
so a plan that created a recovery asset printed `PREPARE none` beside a recovery point that
demonstrably existed, and `get plan` showed one action for a run that would take two.

## Decision

**`plan` adds one `PlanAction` of role `PREPARE` per protection action the analysis chose**, and
the executor settles them.

Their execution is `Execution::RecoveryOperation` — §12.2's shape, naming the provider, the
capability and typed arguments. §12.3 forbids generating a shell command string from
user-controlled values, and there is no variant here that could hold one.

They are inspectable and refusable exactly like the mutating actions, which is the point: §2.1
keeps the asset unmade until `apply`, and an operator who does not want a snapshot taken can see
that one is planned and seal a different plan. An optional action says so in its own text, so a
reader can tell which of them §2.3 would abort for.

**The executor settles a PREPARE action against the asset it planned.** The match is the action's
target against the created asset's scope domain. §4.5 has already run them by the time `mutate`
starts, so leaving them `pending` would describe a run that did not happen; the status is written
to the store beside every other action status, which is what §4.7 asks for and what §41.2 reads.

`ChangePlan::settled` puts a run's statuses back onto the plan, so a renderer handed the plan and
the outcome shows the run rather than the seal. Appendix E's `PREPARE`, `APPLY` and `VERIFY` lines
now count what happened.

## Consequences

- A protected plan shows two actions, and applying it reports `PREPARE 1/1 APPLY 1/1 VERIFY 1/1`.
- The protection actions and `analysis.actions()` are two views of one decision. The analysis
  remains authoritative — it is what `prepare` executes — and the plan actions are its record.
  They are built from the same `ProtectionAction` in the same statement, so they cannot describe
  different assets, but a later change to either must change both.
- A plan sealed before this change has no PREPARE actions and still applies: the executor works
  from the analysis, and settling is skipped for actions that are not there.
- `PlanBuilder::acting` renumbers onto the end, so the protection actions carry higher ordinals
  than the mutation they protect while running before it. §4.5 fixes the order by role, and the
  ordinal is an identity input rather than a schedule.

## Alternatives considered

- **Leave them out and teach the renderer to count assets.** Smaller, and it leaves §17.1's
  sentence unimplemented while making the display agree with the implementation — the wrong one
  of the two to move.
- **Drive PREPARE from the plan's actions instead of from the analysis.** One representation
  rather than two, and the plan action cannot carry the `RecoveryCandidate` and the proposed asset
  that `prepare` needs without becoming a copy of them. That is a larger change to
  `ono-change-executor`'s contract than this defect justifies, and it is the shape to revisit if a
  third reader of either appears.
