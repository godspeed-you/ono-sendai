//! Recovery verification (v0.6 §25.1, §25.2, §25.3, §41.3, Appendix F).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    ChangePlan, EffectDomain, EquivalenceDomain, EquivalenceState, Intent, RecoveryAssetId,
    RecoveryGoal, RecoveryPlan, RestoreMethod, UnrecoverableEffect, VerificationClass,
    VerificationContract, VerificationSet, VerificationStatus,
};
use ono_change_recovery::verify::{recovery_failed, refusal_for, results, verify};
use support::{EPOCH, at};

const SNAPSHOT: &str = "rpool/ROOT/debian@ono-a82f";

struct Fixture {
    contracts: Vec<VerificationContract>,
    unrecoverable: Vec<UnrecoverableEffect>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            contracts: Vec::new(),
            unrecoverable: Vec::new(),
        }
    }

    fn checking(
        mut self,
        subject: &str,
        class: VerificationClass,
        domain: EquivalenceDomain,
    ) -> Self {
        let plan = draft();
        self.contracts.push(
            VerificationContract::new(plan.id(), class, subject, "equivalent == true")
                .about(domain),
        );
        self
    }

    fn checking_without_a_domain(mut self, subject: &str) -> Self {
        let plan = draft();
        self.contracts.push(VerificationContract::new(
            plan.id(),
            VerificationClass::Required,
            subject,
            "equivalent == true",
        ));
        self
    }

    fn leaving(mut self, subject: &str, domain: EffectDomain) -> Self {
        self.unrecoverable.push(UnrecoverableEffect::new(
            subject,
            domain,
            "the fixture declares it beyond reach",
        ));
        self
    }

    fn build(self) -> RecoveryPlan {
        let plan = draft()
            .with_verification(VerificationSet::of(self.contracts))
            .seal(EPOCH)
            .expect("a plan with no mutating action seals without a contract");
        let mut recovery = RecoveryPlan::new(
            plan,
            RecoveryGoal::RestoreChangedObjects,
            RestoreMethod::SelectiveFileRestore,
            SNAPSHOT,
        )
        .using(RecoveryAssetId::of(
            "ono.recovery.zfs",
            None,
            "rpool/ROOT/debian",
            "0",
        ));
        for effect in self.unrecoverable {
            recovery = recovery.leaving(effect);
        }
        recovery
    }
}

fn draft() -> ChangePlan {
    ChangePlan::draft(
        Intent::new("restore nginx configuration", "recover plan/a82f"),
        "session-verify",
        EPOCH,
    )
}

fn answering(
    answers: &'static [(&'static str, VerificationStatus)],
) -> impl Fn(&VerificationContract) -> VerificationStatus {
    move |contract| {
        answers
            .iter()
            .find(|(subject, _)| *subject == contract.subject())
            .map_or(VerificationStatus::Unknown, |(_, status)| *status)
    }
}

/// §25.2's worked shape: the config and the package version came back, the service is running
/// again with new worker PIDs, and the TCP sessions and the webhook are gone for good.
fn worked_example() -> RecoveryPlan {
    Fixture::new()
        .checking(
            "nginx.conf",
            VerificationClass::Required,
            EquivalenceDomain::PersistentState,
        )
        .checking(
            "package version",
            VerificationClass::Required,
            EquivalenceDomain::PersistentState,
        )
        .checking(
            "service state",
            VerificationClass::Required,
            EquivalenceDomain::RuntimeState,
        )
        .checking(
            "worker PIDs",
            VerificationClass::Observational,
            EquivalenceDomain::RuntimeState,
        )
        .leaving("TCP connections", EffectDomain::NetworkRuntime)
        .leaving("1 webhook request", EffectDomain::ExternalSideEffect)
        .build()
}

const WORKED_ANSWERS: &[(&str, VerificationStatus)] = &[
    ("nginx.conf", VerificationStatus::Passed),
    ("package version", VerificationStatus::Passed),
    ("service state", VerificationStatus::Passed),
    ("worker PIDs", VerificationStatus::Failed),
];

#[test]
fn should_verify_persistent_state_when_the_config_and_the_package_version_came_back() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    assert!(
        outcome.persistent_state_verified(),
        "§25.2: PERSISTENT STATE VERIFIED is a statement about persistent state"
    );
}

#[test]
fn should_report_new_worker_pids_as_an_expected_difference() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    let workers = outcome
        .runtime()
        .iter()
        .find(|(subject, _)| subject.as_ref() == "worker PIDs")
        .expect("§25.2 reports the worker PIDs");
    assert_eq!(
        workers.1,
        EquivalenceState::DifferentAsExpected,
        "§25.2: worker PIDs DIFFERENT / EXPECTED"
    );
}

#[test]
fn should_not_make_new_worker_pids_a_recovery_failure() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    assert!(
        outcome.persistent_state_verified(),
        "§25.1: a service restart may restore configuration while naturally creating new PIDs"
    );
    assert!(
        refusal_for(&results(&plan, &answering(WORKED_ANSWERS), at(16, 30))).is_none(),
        "§23.2: an observational result never changes the plan's state"
    );
}

#[test]
fn should_report_tcp_connections_as_not_recoverable() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    assert!(
        outcome
            .runtime()
            .iter()
            .any(|(subject, state)| subject.as_ref() == "TCP connections"
                && *state == EquivalenceState::NotRecoverable),
        "§25.2: TCP connections NOT RECOVERABLE"
    );
}

#[test]
fn should_report_a_delivered_webhook_as_not_recoverable() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    assert!(
        outcome
            .external()
            .iter()
            .any(|(subject, state)| subject.as_ref() == "1 webhook request"
                && *state == EquivalenceState::NotRecoverable),
        "§25.2: 1 webhook request NOT RECOVERABLE"
    );
}

#[test]
fn should_report_that_something_is_unrecoverable_beside_the_verified_scope() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    assert!(
        outcome.has_unrecoverable(),
        "§25.2: FULL WORLD EQUIVALENCE NOT CLAIMED"
    );
}

#[test]
fn should_report_the_restored_service_state_in_the_runtime_domain() {
    let plan = worked_example();
    let outcome = verify(&plan, &answering(WORKED_ANSWERS), at(16, 30));
    assert!(
        outcome
            .runtime()
            .iter()
            .any(|(subject, state)| subject.as_ref() == "service state"
                && *state == EquivalenceState::Restored),
        "§25.1: runtime-state equivalence is reported separately"
    );
}

#[test]
fn should_refuse_when_a_required_recovery_check_failed() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[
        ("nginx.conf", VerificationStatus::Failed),
        ("package version", VerificationStatus::Passed),
        ("service state", VerificationStatus::Passed),
        ("worker PIDs", VerificationStatus::Failed),
    ];
    let plan = worked_example();
    let error = refusal_for(&results(&plan, &answering(ANSWERS), at(16, 30)))
        .expect("Appendix F: recovery verification fails and does not claim recovered");
    assert_eq!(error.code().name(), "recovery.verification_failed");
}

#[test]
fn should_name_the_domain_a_failed_recovery_check_belongs_to() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[
        ("nginx.conf", VerificationStatus::Failed),
        ("package version", VerificationStatus::Passed),
        ("service state", VerificationStatus::Passed),
        ("worker PIDs", VerificationStatus::Failed),
    ];
    let plan = worked_example();
    let error = refusal_for(&results(&plan, &answering(ANSWERS), at(16, 30)))
        .expect("the required check failed");
    let domains = error
        .metadata()
        .get("domains")
        .expect("§25.3: the metadata reports per equivalence domain")
        .as_list()
        .expect("a list")
        .to_vec();
    assert_eq!(
        domains,
        vec![ono_value::Value::string("persistent-state")],
        "§25.3: user-visible language MUST describe the verified scope"
    );
}

#[test]
fn should_not_claim_persistent_state_when_a_required_check_failed() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[
        ("nginx.conf", VerificationStatus::Failed),
        ("package version", VerificationStatus::Passed),
        ("service state", VerificationStatus::Passed),
        ("worker PIDs", VerificationStatus::Failed),
    ];
    let plan = worked_example();
    let outcome = verify(&plan, &answering(ANSWERS), at(16, 30));
    assert!(
        !outcome.persistent_state_verified(),
        "Appendix F: nothing claims the state was recovered"
    );
}

#[test]
fn should_refuse_when_a_required_recovery_check_could_not_be_answered() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[
        ("nginx.conf", VerificationStatus::Unknown),
        ("package version", VerificationStatus::Passed),
        ("service state", VerificationStatus::Passed),
        ("worker PIDs", VerificationStatus::Failed),
    ];
    let plan = worked_example();
    assert!(
        refusal_for(&results(&plan, &answering(ANSWERS), at(16, 30))).is_some(),
        "§23.5 forbids treating an unanswered check as success"
    );
}

#[test]
fn should_report_a_skipped_check_as_unknown() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[("nginx.conf", VerificationStatus::Skipped)];
    let plan = Fixture::new()
        .checking(
            "nginx.conf",
            VerificationClass::Required,
            EquivalenceDomain::PersistentState,
        )
        .build();
    let outcome = verify(&plan, &answering(ANSWERS), at(16, 30));
    assert_eq!(outcome.persistent()[0].1, EquivalenceState::Unknown);
}

#[test]
fn should_not_refuse_when_only_an_advisory_check_failed() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[
        ("nginx.conf", VerificationStatus::Passed),
        ("cache warmed", VerificationStatus::Failed),
    ];
    let plan = Fixture::new()
        .checking(
            "nginx.conf",
            VerificationClass::Required,
            EquivalenceDomain::PersistentState,
        )
        .checking(
            "cache warmed",
            VerificationClass::Advisory,
            EquivalenceDomain::PersistentState,
        )
        .build();
    assert!(
        refusal_for(&results(&plan, &answering(ANSWERS), at(16, 30))).is_none(),
        "§23.2: a failing advisory expectation may make the plan DEGRADED, not FAILED"
    );
}

#[test]
fn should_not_verify_persistent_state_from_a_check_that_declares_no_domain() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[("something", VerificationStatus::Passed)];
    let plan = Fixture::new()
        .checking_without_a_domain("something")
        .build();
    let outcome = verify(&plan, &answering(ANSWERS), at(16, 30));
    assert!(
        !outcome.persistent_state_verified(),
        "§25.1 requires a recovery verification to say which equivalence it establishes"
    );
}

#[test]
fn should_block_the_persistent_claim_when_an_effect_domain_could_not_be_classified() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[("nginx.conf", VerificationStatus::Passed)];
    let plan = Fixture::new()
        .checking(
            "nginx.conf",
            VerificationClass::Required,
            EquivalenceDomain::PersistentState,
        )
        .leaving("an opaque action", EffectDomain::Unknown)
        .build();
    let outcome = verify(&plan, &answering(ANSWERS), at(16, 30));
    assert!(
        !outcome.persistent_state_verified(),
        "Appendix A.7 and §56.3: an unclassified domain does not support a claim about any domain"
    );
}

#[test]
fn should_record_when_each_recovery_check_was_observed() {
    const ANSWERS: &[(&str, VerificationStatus)] = &[("nginx.conf", VerificationStatus::Passed)];
    let plan = Fixture::new()
        .checking(
            "nginx.conf",
            VerificationClass::Required,
            EquivalenceDomain::PersistentState,
        )
        .build();
    let observed = results(&plan, &answering(ANSWERS), at(16, 30));
    assert_eq!(
        observed[0].at(),
        at(16, 30),
        "§23.3 records when the observation was made, and it comes from the caller"
    );
}

#[test]
fn should_report_a_recovery_failure_without_claiming_the_state_came_back() {
    let plan = worked_example();
    let error = recovery_failed(
        &plan,
        "restore /etc/nginx/nginx.conf",
        "the dataset is busy",
    );
    assert_eq!(error.code().name(), "recovery.apply_failed");
}

#[test]
fn should_name_the_assets_a_failed_recovery_still_has() {
    let plan = worked_example();
    let error = recovery_failed(
        &plan,
        "restore /etc/nginx/nginx.conf",
        "the dataset is busy",
    );
    let retained: Vec<String> = error
        .metadata()
        .get("retained_assets")
        .expect("Appendix F: preserve remaining assets")
        .as_list()
        .expect("a list")
        .iter()
        .map(|asset| asset.as_str().expect("an asset identity").to_owned())
        .collect();
    let rested_on: Vec<String> = plan
        .source_assets()
        .iter()
        .map(|asset| asset.as_str().to_owned())
        .collect();
    assert_eq!(
        rested_on.len(),
        1,
        "precondition: the recovery rests on one asset"
    );
    assert_eq!(
        retained, rested_on,
        "§41.3 and Appendix F: the remaining assets are named by the identity a retry can use"
    );
}

#[test]
fn should_name_the_method_a_failed_recovery_was_using() {
    let plan = worked_example();
    let error = recovery_failed(
        &plan,
        "restore /etc/nginx/nginx.conf",
        "the dataset is busy",
    );
    assert_eq!(
        error.metadata().get("method"),
        Some(&ono_value::Value::string("selective-file-restore")),
        "Appendix F: the exact partial state includes how the recovery was being carried out"
    );
}

#[test]
fn should_report_no_domain_at_all_when_a_recovery_carries_no_checks() {
    let plan = Fixture::new().build();
    let outcome = verify(&plan, &answering(&[]), at(16, 30));
    assert!(
        !outcome.persistent_state_verified(),
        "§25.2: a claim about persistent state needs evidence about persistent state"
    );
}
