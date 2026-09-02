# ADR-0494: A cost class is a weight a caller can read

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §33.3, §34.1, §34.2, §34.3, §52.1, §52.2, §53.1; v0.4 §32.1, §32.2, §35.3;
  ADR-0491, ADR-0493; issues #25, #86
- Decided by: agent (autonomous)

## Context

§34.2 lists four names and adds one requirement:

> Relationship/provider acquisition SHOULD classify cost as `cheap` / `moderate` / `expensive` /
> `external`. **The class MUST be machine-readable.**

The shell already had a cost classification — `ono_spatial_core::CostClass` — and it is five
variants with different names: `Cheap`, `Normal`, `Expensive`, `Privileged`, `Remote`. It is used,
correctly, to decide what an orientation query may follow eagerly (v0.4 §32.1, §32.2) and how a
relation declines (§35.3). It is also entirely internal: nothing serialised it, nothing declared
it, and `grep` found no caller outside the three crates that compute with it.

§34.1 then asks for the coarse estimate the class is an input to, and sets its bar:

> It need not be mathematically exact. It MUST be conservative enough to avoid obviously explosive
> work.

## Decision

**Two vocabularies, one map between them, and the class is a weight rather than a label.**

### 1. `AcquisitionCost` is what a caller reads; `CostClass` stays what the planner reasons in

The planner has to tell a broad local scan from a privilege it will not request from a hop across
a link, because it declines all three differently. A caller deciding whether to ask for something
needs one thing: how expensive it is, and whether it leaves this shell. So `AcquisitionCost`
carries exactly §34.2's four names, `CostClass::acquisition()` maps the five onto them, and
`Privileged` and `Remote` both report `external` — one needs an authority outside this shell, the
other a host outside this one, and to somebody deciding whether to pay that is the same answer.

Renaming `Normal` to `Moderate` across the planner was the obvious alternative and was rejected:
it would put §34.2's caller-facing vocabulary into the code that has to make a finer distinction,
and the finer distinction would then have nowhere to live.

### 2. The class is a weight, because §34.1 makes it an input

`AcquisitionCost::weight()` is 1, 4, 16, 64. §34.1 lists "relationship acquisition cost class"
among the estimate's inputs, so a class that were only a label would not be one. The numbers are
orders of magnitude, not measurements: an external acquisition is a round trip to another process
whose latency this shell does not control, which is why it outweighs a broad local scan.

`CostEstimate` uses four of §34.1's six inputs — candidate count, fan-out, class and depth. The
two it leaves out, **selector selectivity and cache state, can only make a query cheaper than this
says**, which is the direction "conservative" points in.

### 3. The budget is in candidate acquisitions, not in milliseconds

`INTERACTIVE_BUDGET` is 250 000 units. A thousand candidates at moderate cost with a fan-out of
four is 20 000 and is answered; two hundred thousand candidates is not. It is deliberately not a
wall clock: §32.4 puts absolute time on a named reference environment, and a refusal measured in
milliseconds would refuse different queries on different hosts — the same defect ADR-0491 and
ADR-0459 record in the test suite, arriving through the product instead.

### 4. The refusal names the estimate

`Ono-Sendai-E1401 spatial.cost_refused` carries the number of units and the number of candidates
it was estimated over. §34.1's exit test asks for a refusal "naming the estimate rather than
running", and §53.1 makes the stable code the thing a script catches. Issue #86's own exit test is
this sentence, and the message is written so that a reader can act on it: narrow the place, lower
the depth, or ask explicitly.

### 5. `--all` is the request path at the planning layer

§34.3 requires that anything described as "available on request" have a request path.
`CostEstimate::requested()` is that path where the planner is concerned: an estimate a caller has
accepted is not refused, because the refusal exists to stop work nobody asked for. `map --all`
sets it. The *user-facing* half — `follow owner` on a relation that declined — is issue #25 and the
next increment.

### 6. Where it is wired

`crates/ono-cli/src/spatial/map.rs::project_at` estimates the horizon before projecting it and
refuses over budget. That is the one place today where the candidate count is known before the
expansion happens. `look` and `near` are issue #87's, and the estimate is the input they will
bound with.

## Consequences

Easy: §34.2's class is a value with a stable spelling, declared in
`docs/spec/hardening/cost_classes.yaml` and checked against the implementation on every gate run
in both directions, including the weights and the budget. #87 has an estimate to bound with rather
than a number to invent, and #25 has a request path to hang a flag on.

Hard: the estimate is coarse enough that nothing on an ordinary host reaches the budget, so the
refusal is a guard rather than a behaviour anyone will see. That is what §34.1 asks for — a bar
for *obviously explosive* work — and it means the guard's value is in the queries it will stop
rather than in the ones it stops today.

Also hard: `xtask` now depends on `ono-spatial-query` for one constant. The alternative was a
second copy of 250 000 in the checker, which is exactly what §52.2 forbids.

Encoded by `crates/ono-spatial-query/tests/cost.rs::should_assign_a_declared_cost_class_to_every_canonical_query`,
`::should_refuse_with_the_estimated_cost_when_the_estimate_exceeds_the_interactive_budget`,
`::should_pay_for_an_expensive_relation_when_it_is_explicitly_requested`, and
`xtask::contracts::check_cost_classes`.

## Alternatives considered

**Rename `CostClass`'s variants to §34.2's four and drop `Privileged`/`Remote`.** One vocabulary
is simpler, and it would lose the two distinctions §35.3 needs to decline differently. The map is
cheaper than the loss.

**Estimate in milliseconds.** It is what a reader wants, and it is a property of the machine. §32.4
already says where absolute time is measured, and it is not in a planner running on an unknown
host.

**Put the per-relation class in the registry too.** `RelationSpec` already declares one per
relation and the test requires every relation and every enumerable space to carry one; a second
copy of thirty-odd assignments in YAML would be thirty-odd chances to disagree. The *vocabulary*
is what §34.2 requires to be machine-readable, and that is what the registry holds.

**Refuse in `look` and `near` as well, in this increment.** The estimate there has to be made
before the observation rather than after it, which is a different calculation and the subject of
§34.4. Mixing them would put two changes in one commit.
