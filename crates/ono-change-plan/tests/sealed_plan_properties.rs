//! Property tests for the seal (v0.6 §54.2, §2.6, §4.4, §7.5, §36.1).
//!
//! §54.2 asks two things of every sealed plan, not only of the plans the example suites build:
//! that it is immutable, and that no target can appear in it without its revision changing. Both
//! are held here against generated plans — generated target sets, with and without a mutating
//! action, a coverage matrix and a provider binding — in memory and across the plan store, which
//! is where §36.1 keeps a sealed plan after the shell that sealed it has gone. Each case is fixed
//! by its seed, and a failure names the seed that reproduces it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::value::{frozen_target_map, plan_from_record, plan_record};
use ono_change_core::{
    ActionRole, ChangePlan, DomainCoverage, DomainProtection, EffectDomain, Execution,
    FrozenTarget, Intent, PlanAction, PlanState, ProtectionSummary, ProviderBinding,
    RecoveryObjective, VerificationClass, VerificationContract, VerificationSet,
};
use ono_testkit::Rng;
use ono_value::{RecordValue, Value};
use support::store;

mod support;

/// How many generated plans each property is held against.
const CASES: u64 = 256;

const SCHEMAS: &[&str] = &["ono.service/1", "ono.file/1", "ono.package/1", "ono.user/1"];
const HOSTS: &[&str] = &["localhost", "db-1", "edge-7"];

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("a valid instant")
}

/// One frozen target. `ordinal` keeps identities unique within a plan and across the extra
/// target a property tries to add.
fn target(rng: &mut Rng, seed: u64, ordinal: usize) -> FrozenTarget {
    let schema = *rng.pick(SCHEMAS).expect("four schemas");
    let mut target = FrozenTarget::new(
        schema,
        format!("{schema}:object-{seed}-{ordinal}"),
        format!("object {ordinal} of case {seed}"),
    );
    if rng.chance(2) {
        target = target.on_host(*rng.pick(HOSTS).expect("three hosts"));
    }
    if rng.chance(3) {
        target = target.resolved_from(format!("get {schema} | where ordinal == {ordinal}"));
    }
    if rng.chance(3) {
        target = target.in_domain("zfs:rpool/ROOT/debian");
    }
    target
}

fn summary(rng: &mut Rng) -> ProtectionSummary {
    let rows = (0..1 + rng.below(3))
        .map(|index| {
            DomainCoverage::new(
                *rng.pick(EffectDomain::ALL).expect("a closed vocabulary"),
                *rng.pick(RecoveryObjective::ALL)
                    .expect("a closed vocabulary"),
                *rng.pick(DomainProtection::ALL)
                    .expect("a closed vocabulary"),
                format!("generated row {index}"),
            )
        })
        .collect();
    ProtectionSummary::of(rows)
}

/// A sealed plan over one to five generated targets.
fn sealed(rng: &mut Rng, seed: u64) -> ChangePlan {
    let created = at(i64::try_from(seed).expect("a small seed") * 60);
    let targets = (0..1 + rng.below(5))
        .map(|ordinal| target(rng, seed, ordinal))
        .collect();
    let mut plan = ChangePlan::draft(
        Intent::new(
            format!("generated change {seed}"),
            format!("plan {{ generated change {seed} }}"),
        ),
        format!("session-{seed}"),
        created,
    )
    .resolve(targets)
    .expect("a draft resolves");
    if rng.chance(2) {
        // §23.1: a plan that mutates seals only with a verification contract.
        let action = PlanAction::new(
            plan.id(),
            1,
            ActionRole::Mutate,
            "restart the generated service",
            Execution::ProviderAction {
                provider: Arc::from("ono.service.systemd"),
                operation: Arc::from("ono.service.restart"),
                arguments: Vec::new(),
            },
        );
        let contract = VerificationContract::new(
            plan.id(),
            VerificationClass::Required,
            "generated.service",
            "state == running",
        );
        plan = plan
            .with_action(action)
            .expect("a resolved plan accepts an action")
            .with_verification(VerificationSet::empty().with(contract));
    }
    if rng.chance(2) {
        plan = plan.with_protection(summary(rng));
    }
    if rng.chance(2) {
        plan = plan.binding(ProviderBinding::new("ono.service.systemd", "255.7"));
    }
    plan.seal(at(i64::try_from(seed).expect("a small seed") * 60 + 30))
        .unwrap_or_else(|error| panic!("seed {seed}: a generated plan seals: {}", error.message()))
}

/// A sealed plan and a target it does not hold.
fn case(seed: u64) -> (ChangePlan, FrozenTarget) {
    let mut rng = Rng::seeded(seed);
    let plan = sealed(&mut rng, seed);
    let extra = target(&mut rng, seed, 99);
    (plan, extra)
}

fn widened(plan: &ChangePlan, extra: &FrozenTarget) -> Vec<FrozenTarget> {
    let mut targets = plan.targets().to_vec();
    targets.push(extra.clone());
    targets
}

/// The same record with one field rewritten, as a store somebody edited would hold it.
fn rewritten(record: &RecordValue, field: &str, value: Value) -> RecordValue {
    let mut builder =
        RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for declared in record.schema().fields() {
        let name = declared.name();
        let held = if name == field {
            value.clone()
        } else {
            record.get(name).cloned().unwrap_or(Value::Null)
        };
        builder = builder
            .set(name, held)
            .expect("the schema declares its own fields");
    }
    builder.build()
}

// --- Sealed plans are immutable (§54.2, §4.4) -------------------------------------------------

#[test]
fn should_refuse_to_freeze_a_new_target_set_into_any_sealed_plan() {
    for seed in 0..CASES {
        let (plan, extra) = case(seed);
        assert!(
            plan.clone().resolve(widened(&plan, &extra)).is_err(),
            "seed {seed}: §2.6 forbids the target set of a sealed plan moving, and resolution \
             was accepted"
        );
    }
}

#[test]
fn should_refuse_to_add_an_action_to_any_sealed_plan() {
    for seed in 0..CASES {
        let (plan, _) = case(seed);
        let action = PlanAction::new(
            plan.id(),
            plan.actions().len() + 1,
            ActionRole::Mutate,
            "an action added after the seal",
            Execution::Program {
                program: Arc::from("/usr/bin/true"),
                argv: Vec::new(),
            },
        );
        assert!(
            plan.clone().with_action(action).is_err(),
            "seed {seed}: §4.4 makes a sealed plan immutable, and it accepted a new action"
        );
    }
}

#[test]
fn should_refuse_to_seal_any_sealed_plan_a_second_time() {
    for seed in 0..CASES {
        let (plan, _) = case(seed);
        assert!(
            plan.clone().seal(at(1_000_000)).is_err(),
            "seed {seed}: a sealed plan was sealed again, which would re-date and re-digest it"
        );
    }
}

#[test]
fn should_read_back_every_sealed_plan_with_the_seal_it_was_written_with() {
    let (_directory, store) = store();
    for seed in 0..CASES {
        let (plan, _) = case(seed);
        store.put(&plan).expect("the store accepts a sealed plan");
        let read = store.get(plan.id()).expect("the plan reads back");
        assert_eq!(
            (read.digest(), read.revision(), read.state()),
            (plan.digest(), plan.revision(), PlanState::Sealed),
            "seed {seed}: §36.1: a sealed plan survives the store as the same seal"
        );
        assert!(
            read.digest_holds(),
            "seed {seed}: §63.2: the digest read back no longer describes the plan read back"
        );
        assert_eq!(
            read.targets(),
            plan.targets(),
            "seed {seed}: the store changed the frozen target set of a sealed plan"
        );
    }
}

// --- No target appears after seal without a revision change (§54.2, §7.5) ---------------------

#[test]
fn should_carry_a_new_target_only_in_a_new_revision_with_a_new_digest() {
    for seed in 0..CASES {
        let (plan, extra) = case(seed);
        let revised = plan.revise();
        assert_eq!(
            (revised.revision(), revised.state(), revised.digest()),
            (plan.revision() + 1, PlanState::Draft, None),
            "seed {seed}: §7.5: a revision is a new, unsealed draft one revision on"
        );
        let resealed = revised
            .resolve(widened(&plan, &extra))
            .expect("a revision accepts a new target set")
            .seal(at(2_000_000))
            .expect("the revision seals");
        assert!(
            resealed.targets().contains(&extra) && !plan.targets().contains(&extra),
            "seed {seed}: the new target is in the revision and only there"
        );
        assert_ne!(
            resealed.digest(),
            plan.digest(),
            "seed {seed}: §4.4: a different target set sealed under the same digest"
        );
        assert!(
            plan.digest_holds(),
            "seed {seed}: revising a sealed plan disturbed the original's seal"
        );
    }
}

#[test]
fn should_keep_the_sealed_original_in_the_store_when_a_revision_adds_a_target() {
    let (_directory, store) = store();
    for seed in 0..CASES {
        let (plan, extra) = case(seed);
        store.put(&plan).expect("the store accepts the original");
        let resealed = plan
            .revise()
            .resolve(widened(&plan, &extra))
            .expect("a revision accepts a new target set")
            .seal(at(2_000_000))
            .expect("the revision seals");
        store
            .put(&resealed)
            .expect("the store accepts the revision");

        let original = store
            .get_revision(plan.id(), plan.revision())
            .expect("the original revision reads back");
        assert_eq!(
            (original.digest(), original.targets()),
            (plan.digest(), plan.targets()),
            "seed {seed}: §7.5: storing a revision rewrote the sealed original"
        );
        let latest = store
            .get(plan.id())
            .expect("the latest revision reads back");
        assert_eq!(
            (latest.revision(), latest.targets().contains(&extra)),
            (plan.revision() + 1, true),
            "seed {seed}: the new target is read back under the new revision"
        );
    }
}

#[test]
fn should_break_the_seal_of_a_stored_plan_whose_targets_were_widened_under_the_same_revision() {
    // The store is a file on disk (§36.1). A target written into a sealed record without a new
    // revision is exactly what §54.2 forbids, and the seal is what must give it away.
    for seed in 0..CASES {
        let (plan, extra) = case(seed);
        let record = plan_record(&plan).expect("a sealed plan encodes");
        let widened_targets = Value::list(
            plan.targets()
                .iter()
                .chain(std::iter::once(&extra))
                .map(frozen_target_map),
        );
        let tampered = rewritten(&record, "targets", widened_targets);
        if let Ok(read) = plan_from_record(&tampered) {
            assert!(
                read.targets().contains(&extra),
                "seed {seed}: the rewritten record must carry the smuggled target"
            );
            assert!(
                !read.digest_holds(),
                "seed {seed}: §54.2: a target appeared in sealed revision {} and the seal still \
                 holds",
                read.revision()
            );
        }
    }
}
