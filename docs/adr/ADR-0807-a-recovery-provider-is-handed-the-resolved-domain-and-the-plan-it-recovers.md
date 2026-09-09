# ADR-0807: A recovery provider is handed the resolved domain, and the plan it is recovering from

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.6 §11.2, §12.1, §50.1, §56.3, Appendix A.3, Appendix B, Appendix C.2
- Decided by: agent (autonomous)

## Context

§12.1 sketches the `RecoveryProvider` trait and says "The exact Rust signature may differ. The
semantics are normative." Two of its six methods take arguments that make the semantics harder to
satisfy than they need to be:

```rust
fn discover(&self, target: &ObjectRef) -> Result<Vec<RecoveryCandidate>>;
fn plan_recovery(&self, asset: &RecoveryAsset, current: &SystemState)
    -> Result<RecoveryPlanFragment>;
```

`discover(&ObjectRef)` means each provider maps the path to its own persistence object. §11.2 says
a path MUST be mapped to its containing persistence domain before protection is claimed, and
Appendix B gives a seven-step normative pipeline for doing it. Three providers each walking that
pipeline is three chances to walk it differently, and the differences would be invisible until one
of them claimed coverage another would have refused.

`plan_recovery(&self, asset, current: &SystemState)` asks the provider to decide the method from
the asset and the world. Appendix C.2 says the goal is usually not "return the whole persistence
domain to the snapshot timestamp" but "restore the changed configuration object and the service to
a healthy semantic state" — which is a fact about **what the original plan changed**, and is not
recoverable from the asset and the current world alone.

## Decision

Two departures from §12.1's sketch, both toward giving the provider the facts it needs:

```rust
fn resolve_domain(&self, path: &str) -> Result<Option<PersistenceDomain>, ErrorValue>;
fn discover(&self, domain: &PersistenceDomain, objective: RecoveryObjective)
    -> Result<Vec<RecoveryCandidate>, ErrorValue>;
fn plan_recovery(&self, asset: &RecoveryAsset, source: Option<&ChangePlan>, goal: RecoveryGoal)
    -> Result<RecoveryPlanFragment, ErrorValue>;
```

- **`discover` takes the resolved `PersistenceDomain`.** Appendix B's pipeline runs once, in
  `ono-change-protection`, and every provider sees the same answer. A provider still owns the last
  step — which dataset, which subvolume id, which boundaries — and `resolve_domain` is where it
  contributes that, so §13.4's and §14.3's boundary work stays with the provider that understands
  it. What moves out is the part Appendix B makes normative for everyone: mount resolution, bind
  mounts, overlays, network filesystems, pseudo and volatile filesystems.
- **`discover` takes the objective.** Appendix A.3 asks providers for candidates "for each required
  domain", and the objective is what makes a candidate answerable: a provider that can only offer
  `RESTORE_SEMANTIC` should not have to guess whether `PRESERVE_EXACT` was wanted.
- **`plan_recovery` takes the source plan rather than a `SystemState`.** Appendix C.2's goal is a
  property of what the original plan changed. `Option<&ChangePlan>` is `None` for §5.8's second
  form, recovering toward an asset directly, and the goal parameter carries what the operator asked
  for either way. The current world reaches the provider through the observations the recovery
  engine hands it, which keeps §50.1's rule that a provider does not own plan orchestration.

The other four methods keep §12.1's shape. `create`, `validate`, `restore` and `cleanup` are the
provider's own domain and need nothing added.

## Spec deviation

- Section: v0.6 §12.1
- Text: "`fn discover(&self, target: &ObjectRef) -> Result<Vec<RecoveryCandidate>>;`" and
  "`fn plan_recovery(&self, asset: &RecoveryAsset, current: &SystemState) -> Result<RecoveryPlanFragment>;`"
- Instead: the signatures above.
- Why: §12.1 itself says the exact signature may differ and the semantics are normative. Passing
  the resolved domain is what makes §11.2's "MUST be mapped before protection is claimed" true
  once rather than three times; passing the source plan is what makes Appendix C.2's goal
  answerable at all.

## Consequences

§56.3's fail-closed rule gets a natural home: `resolve_domain` returning an error rather than
`Ok(None)` distinguishes "not my business" from "I could not establish this", and only the second
blocks.

A provider that ships outside this repository must accept a `PersistenceDomain` it did not build.
That is the intended trade: the pipeline is normative, and a provider that wanted to disagree with
it would be disagreeing with Appendix B.

`ProviderCapabilities::missing_required()` enforces §12.2's five required capabilities at
registration, so a provider that implements `discover` and leaves `restore` unimplemented is
refused rather than discovered later. §62.1 names the alternative: a candidate nobody can use.
