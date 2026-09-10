//! The guards in front of mutation: one apply per plan (§42.4), protection that counts only when
//! it validated (§4.6, §17.2), and the privilege a session actually holds (§43.2, §43.3).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::Arc;

use common::{
    FakeRecoveryProvider, PlanSpec, ProviderScript, Script, empty_registry, instant, no_drift,
    observing, protection, registry, stored,
};
use ono_change_core::{
    AssetState, ChangePlan, PlanState, ProtectionAction, ProtectionMode, VerificationStatus,
};
use ono_change_executor::execute::{
    ApplyOutcome, ApplyRequest, Authority, PrepareRequest, apply, prepare,
};
use ono_change_plan::PlanStore;
use ono_change_protection::ProviderRegistry;

const SECOND: &str = "ono.recovery.second";

fn apply_with(
    plan: &ChangePlan,
    store: &PlanStore,
    session: &str,
    protection: &[ProtectionAction],
    providers: &ProviderRegistry,
    script: &Script,
) -> ApplyOutcome {
    let drift = no_drift();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        plan,
        store,
        session,
        instant(1_000),
        protection,
        providers,
        &drift,
        &execute,
        &observe,
    );
    apply(&mut request)
}

fn code_of(outcome: &ApplyOutcome) -> String {
    outcome
        .error()
        .map(|error| error.code().name().to_owned())
        .unwrap_or_default()
}

/// A healthy provider and a second one whose assets validate the wrong scope.
fn healthy_and_invalid() -> ProviderRegistry {
    let mut providers = ProviderRegistry::new();
    providers
        .register(Arc::new(FakeRecoveryProvider::healthy()))
        .expect("the first provider registers");
    providers
        .register(Arc::new(
            FakeRecoveryProvider::with_script(ProviderScript::ValidatesWrongScope).named(SECOND),
        ))
        .expect("the second provider registers");
    providers
}

// ---- X4, §42.4: one apply per plan ----------------------------------------------------------

#[test]
fn should_refuse_a_stale_copy_of_a_plan_another_session_already_applied() {
    let now = instant(1_000);
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("plans.sqlite3");
    let first = PlanStore::open(&path).expect("the first session opens the store");
    let second = PlanStore::open(&path).expect("the second session opens the same store");
    let plan = stored(&PlanSpec::default(), &first, now);
    // The second session read the plan while it was still sealed.
    let stale = second
        .get(plan.id())
        .expect("the second session reads the plan");
    let providers = empty_registry();

    let applied = apply_with(
        &plan,
        &first,
        "session-a",
        &[],
        &providers,
        &Script::healthy(),
    );
    assert_eq!(
        applied.state(),
        PlanState::Verified,
        "the first session applies"
    );
    let again = Script::healthy();
    let outcome = apply_with(&stale, &second, "session-b", &[], &providers, &again);

    assert!(
        again.calls().is_empty(),
        "§2.7 and §42.4: a sealed plan applies once, whatever copy a later session holds"
    );
    assert_eq!(
        code_of(&outcome),
        "change.plan_not_sealed",
        "the refusal says what the store knows: the plan is past sealed"
    );
    assert!(!outcome.has_mutated(), "the second session changed nothing");
    assert_eq!(
        second.get(plan.id()).expect("the plan reads back").state(),
        PlanState::Verified,
        "the refusal does not write its stale state over the first session's verdict"
    );
}

#[test]
fn should_answer_already_applying_when_another_writer_holds_the_store() {
    let now = instant(1_000);
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("plans.sqlite3");
    let store = PlanStore::open(&path).expect("the store opens");
    let plan = stored(&PlanSpec::default(), &store, now);
    let other = rusqlite::Connection::open(&path).expect("another process opens the file");
    other
        .execute_batch("BEGIN IMMEDIATE")
        .expect("another process holds the write lock");
    let script = Script::healthy();

    let outcome = apply_with(&plan, &store, "session-b", &[], &empty_registry(), &script);

    assert_eq!(
        code_of(&outcome),
        "change.plan_already_applying",
        "§42.4: a claim lost to a concurrent writer is contention, not an unavailable store"
    );
    assert!(script.calls().is_empty());
    other
        .execute_batch("ROLLBACK")
        .expect("the lock is released");
}

// ---- X5, §4.6 and §17.2: an optional asset counts only when it validated -----------------

#[test]
fn should_not_count_an_optional_asset_that_failed_validation_as_protection() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = healthy_and_invalid();
    let protection = vec![
        protection(&plan, common::PROVIDER, "tank/data", now),
        protection(&plan, SECOND, "tank/extra", now).optional(),
    ];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let prepared = prepare(&mut request).expect("§17.2: a maximize extra does not stop prepare");

    assert!(
        prepared.assets().iter().all(|asset| asset.is_usable()),
        "§4.6: the assets preparation vouches for are the ones that validated"
    );
    assert_eq!(prepared.assets().len(), 1);
    let shortfall = prepared.shortfall();
    assert_eq!(
        shortfall.len(),
        1,
        "the invalid extra is reported, never silent"
    );
    assert_eq!(
        shortfall[0].asset().map(|asset| asset.state()),
        Some(AssetState::Invalid),
        "Appendix F: the asset exists and is marked invalid"
    );
    let invalid = shortfall[0].asset().map(|asset| asset.id().clone());
    assert!(
        invalid.is_some_and(|id| request.retained().contains(&id)),
        "the invalid asset still occupies storage, so it is retained and reported"
    );
}

#[test]
fn should_report_an_optional_asset_that_could_not_be_created() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let providers = registry(FakeRecoveryProvider::healthy());
    let protection = vec![
        protection(&plan, common::PROVIDER, "tank/data", now),
        protection(&plan, "ono.recovery.absent", "tank/extra", now).optional(),
    ];
    let mut request = PrepareRequest::new(&plan, &protection, &providers, now);

    let prepared = prepare(&mut request).expect("§17.2: a maximize extra does not stop prepare");

    let shortfall = prepared.shortfall();
    assert_eq!(
        shortfall.len(),
        1,
        "§17.2: a failed extra degrades the coverage, and the degradation is reported"
    );
    assert!(shortfall[0].asset().is_none(), "nothing was created");
    assert_eq!(shortfall[0].provider(), "ono.recovery.absent");
    assert_eq!(
        shortfall[0].reason().code().name(),
        "recovery.provider_unavailable"
    );
}

#[test]
fn should_carry_the_protection_shortfall_into_the_apply_outcome() {
    let now = instant(1_000);
    let (_directory, store) = common::store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = healthy_and_invalid();
    let protection = vec![
        protection(&plan, common::PROVIDER, "tank/data", now),
        protection(&plan, SECOND, "tank/extra", now).optional(),
    ];

    let outcome = apply_with(
        &plan,
        &store,
        "session-a",
        &protection,
        &providers,
        &Script::healthy(),
    );

    assert_eq!(outcome.state(), PlanState::Verified);
    assert_eq!(
        outcome.protection_shortfall().len(),
        1,
        "§17.2: the operator is told the extra did not validate"
    );
    assert!(
        outcome
            .assets()
            .iter()
            .any(|asset| asset.state() == AssetState::Invalid),
        "the invalid asset is still reported among what this apply created"
    );
}

// ---- X6, §17.2 `require`: one predicate decides what is required ------------------------

#[test]
fn should_not_mutate_a_require_plan_whose_only_protection_failed() {
    let now = instant(1_000);
    let (_directory, store) = common::store();
    let plan = stored(
        &PlanSpec::default().under(ProtectionMode::Require),
        &store,
        now,
    );
    let providers = registry(FakeRecoveryProvider::with_script(
        ProviderScript::CreateFails,
    ));
    // The discovery check counted this as protection; preparation then called it optional and
    // dropped its failure. Under `require`, nothing planned is optional.
    let protection = vec![protection(&plan, common::PROVIDER, "tank/data", now).optional()];
    let script = Script::healthy();

    let outcome = apply_with(&plan, &store, "session-a", &protection, &providers, &script);

    assert!(
        script.calls().is_empty(),
        "§17.2: `require` refuses to apply when the protection could not be established"
    );
    assert_eq!(outcome.state(), PlanState::PrepareFailed);
    assert!(!outcome.has_mutated());
}

// ---- X7, §43.3: the session's real privilege ---------------------------------------------

/// `CAP_SYS_ADMIN`, bit 21 of the kernel's effective capability set.
const CAP_SYS_ADMIN: u64 = 1 << 21;

#[test]
fn should_hold_elevation_only_where_the_session_actually_has_it() {
    assert!(
        !Authority::for_session(1000, Some(0)).is_elevated(),
        "an ordinary user without capabilities is not elevated"
    );
    assert!(
        Authority::for_session(1000, Some(CAP_SYS_ADMIN)).is_elevated(),
        "a process granted CAP_SYS_ADMIN is"
    );
    assert!(
        !Authority::for_session(0, Some(0)).is_elevated(),
        "root with every capability dropped cannot do what elevation promises"
    );
    assert!(
        Authority::for_session(0, None).is_elevated(),
        "where the capability set could not be read, the effective uid decides"
    );
}

#[test]
fn should_refuse_a_privileged_plan_for_a_session_that_is_not_elevated() {
    let now = instant(1_000);
    let (_directory, store) = common::store();
    let plan = stored(&PlanSpec::default().privileged(), &store, now);
    let providers = empty_registry();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &[],
        &providers,
        &drift,
        &execute,
        &observe,
    )
    .with_authority(Authority::for_session(1000, Some(0)));

    let outcome = apply(&mut request);

    assert_eq!(code_of(&outcome), "change.privilege_required");
    assert!(script.calls().is_empty());
}
