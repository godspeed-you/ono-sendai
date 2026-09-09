//! What a plan contributes to the v0.5 ledger (spec v0.6 §22.1, §22.2, §22.4, §55.11 cases 48 and
//! 50).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::sync::{Arc, Mutex};

use common::{PlanSpec, Script, empty_registry, instant, no_drift, observing, store, stored};
use ono_change_core::{
    ActionStatus, PlanState, Verdict, VerificationClass, VerificationContract, VerificationResult,
    VerificationStatus,
};
use ono_change_executor::events::{
    ACTION_COMPLETED, ACTION_FAILED, ACTION_STARTED, ASSET_CREATED, ASSET_REMOVED, PLAN_CREATED,
    PLAN_DEGRADED, PLAN_FAILED, PLAN_PROTECTED, PLAN_SEALED, PLAN_VERIFIED, RECOVERY_COMPLETED,
    RECOVERY_FAILED, RECOVERY_PLANNED, RECOVERY_STARTED, RECOVERY_VERIFIED,
    VERIFICATION_OBSERVED, checkpoint_before_mutation,
};
use ono_change_executor::execute::{ApplyRequest, apply};
use ono_change_executor::events::PlanLifecycle;
use ono_spatial_core::{BootIdentity, SpatialScope};
use ono_temporal_core::{
    ActionEvent, Appended, CausalLink, Checkpoint, EvidenceClaim, EvidenceStrength, EventKind,
    LedgerWrite, TemporalCoverage, TemporalEvent,
};
use ono_temporal_ledger::Ledger;
use ono_value::ErrorValue;

fn scope() -> SpatialScope {
    SpatialScope::host("test-host", BootIdentity::new("test-host", "boot-1"))
}

/// A ledger that refuses everything, for the rule that recording never fails a command.
#[derive(Debug, Default)]
struct RefusingLedger {
    attempts: Mutex<usize>,
}

impl RefusingLedger {
    fn attempts(&self) -> usize {
        self.attempts.lock().map(|count| *count).unwrap_or(0)
    }

    fn refusal() -> ErrorValue {
        ono_change_core::error::store_unavailable("the ledger is not writable here")
    }
}

impl LedgerWrite for RefusingLedger {
    fn append(
        &self,
        _events: &[TemporalEvent],
        _evidence: &[ono_temporal_core::Evidence],
    ) -> Result<Appended, ErrorValue> {
        if let Ok(mut count) = self.attempts.lock() {
            *count += 1;
        }
        Err(Self::refusal())
    }

    fn append_links(&self, _links: &[CausalLink]) -> Result<usize, ErrorValue> {
        Err(Self::refusal())
    }

    fn record_coverage(&self, _intervals: &[TemporalCoverage]) -> Result<(), ErrorValue> {
        Err(Self::refusal())
    }

    fn record_action(&self, _action: &ActionEvent) -> Result<(), ErrorValue> {
        Err(Self::refusal())
    }

    fn write_checkpoint(&self, _checkpoint: &Checkpoint) -> Result<(), ErrorValue> {
        Err(Self::refusal())
    }

    fn flush(&self) -> Result<(), ErrorValue> {
        Err(Self::refusal())
    }
}

/// A full plan lifecycle, staged in the order §4.1 walks it.
fn full_lifecycle(now: jiff::Timestamp) -> (ono_change_core::ChangePlan, PlanLifecycle) {
    let plan = PlanSpec::default().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.sealed(plan.digest(), now);
    let asset = common::protection(&plan, common::PROVIDER, "tank/data", now)
        .proposed_asset()
        .clone();
    lifecycle.asset_created(&asset, now);
    lifecycle.protected(1, now);
    for action in plan.actions() {
        lifecycle.action_started(action, now);
        lifecycle.action_settled(action, ActionStatus::Succeeded, now);
    }
    for contract in plan.verification().contracts() {
        let result = VerificationResult::new(
            plan.id().clone(),
            contract,
            VerificationStatus::Passed,
            now,
        );
        lifecycle.verification_observed(&result, now);
    }
    lifecycle.verified(Verdict::Verified, now);
    lifecycle.asset_removed(asset.id(), now);
    (plan, lifecycle)
}

fn subtypes(lifecycle: &PlanLifecycle) -> Vec<String> {
    lifecycle
        .events()
        .iter()
        .filter_map(|event| event.subtype.as_ref().map(|name| name.to_string()))
        .collect()
}

// ---- §22.1: the thirteen events -----------------------------------------------------------------

#[test]
fn should_contribute_every_event_section_twenty_two_names() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let seen = subtypes(&lifecycle);

    for expected in [
        PLAN_CREATED,
        PLAN_SEALED,
        PLAN_PROTECTED,
        ACTION_STARTED,
        ACTION_COMPLETED,
        VERIFICATION_OBSERVED,
        PLAN_VERIFIED,
        ASSET_CREATED,
        ASSET_REMOVED,
    ] {
        assert!(
            seen.iter().any(|subtype| subtype == expected),
            "§22.1 lists {expected} among what a plan should record"
        );
    }
}

#[test]
fn should_contribute_the_recovery_half_of_section_twenty_two() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let recovery = PlanSpec::default().as_recovery().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.recovery_planned(recovery.id(), now);
    lifecycle.recovery_started(now);
    lifecycle.recovery_settled(PlanState::Recovered, now);
    lifecycle.recovery_verified(&["persistent-state".to_owned()], now);
    let seen = subtypes(&lifecycle);

    for expected in [
        RECOVERY_PLANNED,
        RECOVERY_STARTED,
        RECOVERY_COMPLETED,
        RECOVERY_VERIFIED,
    ] {
        assert!(
            seen.iter().any(|subtype| subtype == expected),
            "§22.1 lists {expected}"
        );
    }
}

#[test]
fn should_use_only_kinds_the_closed_list_of_seventeen_already_has() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);

    for event in lifecycle.events() {
        assert!(
            EventKind::ALL.contains(&event.kind),
            "v0.5 §6.1's list is closed, and {} is on it only if it was already",
            event.kind
        );
        assert!(
            event.subtype.is_some(),
            "§6.1: the refinement travels as a namespaced subtype"
        );
    }
}

#[test]
fn should_namespace_every_subtype_under_ono() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);

    for subtype in subtypes(&lifecycle) {
        assert!(
            subtype.starts_with("ono."),
            "v0.5 §31.5: only the Ono project may claim the ono.* namespace, and this is Ono"
        );
    }
}

#[test]
fn should_record_an_action_that_started_as_the_real_executed_kind() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let started = lifecycle
        .events()
        .iter()
        .find(|event| event.subtype.as_deref() == Some(ACTION_STARTED))
        .expect("the action started");

    assert_eq!(
        started.kind,
        EventKind::ActionExecuted,
        "§17.2's own kind for a mutation that was carried out"
    );
}

#[test]
fn should_record_an_action_that_completed_as_the_real_completed_kind() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let completed = lifecycle
        .events()
        .iter()
        .find(|event| event.subtype.as_deref() == Some(ACTION_COMPLETED))
        .expect("the action completed");

    assert_eq!(completed.kind, EventKind::ActionCompleted);
}

#[test]
fn should_refuse_to_call_a_failed_action_a_completion() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.action_settled(&plan.actions()[0], ActionStatus::Failed, now);
    let last = lifecycle.events().last().expect("an event was staged");

    assert_eq!(
        last.kind,
        EventKind::ActionFailed,
        "§2.14: a command that did not succeed has not completed"
    );
    assert_eq!(last.subtype.as_deref(), Some(ACTION_FAILED));
}

#[test]
fn should_record_an_unresolved_action_as_a_failure_kind_carrying_its_real_status() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.action_settled(&plan.actions()[0], ActionStatus::Unknown, now);
    let last = lifecycle.events().last().expect("an event was staged");
    let payload = last
        .payload
        .as_ref()
        .and_then(|value| value.as_map().ok().cloned())
        .expect("the body is a map");

    assert_eq!(
        payload.get("status").cloned(),
        Some(ono_value::Value::string("unknown")),
        "Appendix F.2: the uncertainty survives into the ledger rather than becoming a failure"
    );
}

#[test]
fn should_record_a_degraded_plan_under_its_own_subtype() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.verified(Verdict::Degraded, now);
    let last = lifecycle.events().last().expect("an event was staged");

    assert_eq!(last.subtype.as_deref(), Some(PLAN_DEGRADED));
    assert_eq!(
        last.kind,
        EventKind::ActionCompleted,
        "§4.8: DEGRADED means the intended primary state exists"
    );
}

#[test]
fn should_record_a_failed_plan_as_a_failure() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.verified(Verdict::Failed, now);
    let last = lifecycle.events().last().expect("an event was staged");

    assert_eq!(last.subtype.as_deref(), Some(PLAN_FAILED));
    assert_eq!(last.kind, EventKind::ActionFailed);
}

#[test]
fn should_record_a_failed_recovery_as_a_failure() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.recovery_settled(PlanState::RecoveryFailed, now);
    let last = lifecycle.events().last().expect("an event was staged");

    assert_eq!(last.subtype.as_deref(), Some(RECOVERY_FAILED));
    assert_eq!(
        last.kind,
        EventKind::ActionFailed,
        "Appendix F: nothing may claim the state was recovered"
    );
}

#[test]
fn should_record_a_verification_as_an_observation_rather_than_a_change() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let observed = lifecycle
        .events()
        .iter()
        .find(|event| event.subtype.as_deref() == Some(VERIFICATION_OBSERVED))
        .expect("a check was observed");

    assert_eq!(
        observed.kind,
        EventKind::ObjectObserved,
        "§23 observes the world without claiming it changed"
    );
}

#[test]
fn should_record_an_asset_as_an_object_that_appeared() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let created = lifecycle
        .events()
        .iter()
        .find(|event| event.subtype.as_deref() == Some(ASSET_CREATED))
        .expect("an asset was created");
    let removed = lifecycle
        .events()
        .iter()
        .find(|event| event.subtype.as_deref() == Some(ASSET_REMOVED))
        .expect("an asset was removed");

    assert_eq!(created.kind, EventKind::ObjectAppeared);
    assert_eq!(removed.kind, EventKind::ObjectDisappeared);
}

// ---- §22.4: the causal anchor -------------------------------------------------------------------

#[test]
fn should_anchor_every_event_on_the_plan_identity() {
    let now = instant(1_000);
    let (plan, lifecycle) = full_lifecycle(now);

    assert_eq!(
        lifecycle.evidence().len(),
        lifecycle.events().len(),
        "every event carries the claim `why` and `timeline --plan` join on"
    );
    for record in lifecycle.evidence() {
        match &record.claim {
            EvidenceClaim::Transaction { token, .. } => assert_eq!(
                token.as_ref(),
                plan.id().as_str(),
                "§22.4: the plan ID is a causal anchor"
            ),
            other => panic!("§22.4's mechanism is a transaction claim, not {other:?}"),
        }
        assert_eq!(
            record.strength,
            EvidenceStrength::Authoritative,
            "the shell owns the fact that it ran this plan"
        );
    }
}

#[test]
fn should_attribute_every_record_to_the_session_source() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);

    for record in lifecycle.evidence() {
        assert_eq!(
            record.source.as_str(),
            "ono.session",
            "v0.5 §7.1's list is closed, and a plan is the shell's own observation"
        );
    }
}

#[test]
fn should_carry_the_plan_and_its_revision_in_every_payload() {
    let now = instant(1_000);
    let (plan, lifecycle) = full_lifecycle(now);

    for event in lifecycle.events() {
        let payload = event
            .payload
            .as_ref()
            .and_then(|value| value.as_map().ok().cloned())
            .expect("every event carries a plan-shaped body");
        assert_eq!(
            payload.get("plan").cloned(),
            Some(ono_value::Value::string(plan.id().as_str()))
        );
        assert!(payload.get("revision").is_some());
    }
}

#[test]
fn should_give_two_different_events_two_different_identities() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let mut ids: Vec<String> = lifecycle
        .events()
        .iter()
        .map(|event| event.event_id.as_str().to_owned())
        .collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();

    assert_eq!(
        ids.len(),
        total,
        "v0.5 §3.3: an event has a stable identity independent of its rendered row"
    );
}

// ---- recording never fails a command -------------------------------------------------------------

#[test]
fn should_return_the_ledgers_refusal_rather_than_hiding_it() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let ledger = RefusingLedger::default();

    let outcome = lifecycle.record(&ledger);

    assert!(outcome.is_err(), "the caller is told, and then decides");
    assert_eq!(ledger.attempts(), 1);
}

#[test]
fn should_not_change_the_apply_outcome_when_the_ledger_refuses_every_append() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let plan = stored(&PlanSpec::default(), &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.verified(Verdict::Verified, now);
    let ledger = RefusingLedger::default();
    // The call-site rule: v0.5 §16.5 does not make a refused append a reason to lose the result.
    let _ = lifecycle.record(&ledger);

    assert_eq!(
        outcome.state(),
        PlanState::Verified,
        "a ledger that refused an append is not a reason to lose the mutation's own result"
    );
    assert!(outcome.is_success());
}

#[test]
fn should_write_the_lifecycle_to_a_session_ledger_that_accepts_it() {
    let now = instant(1_000);
    let (_plan, lifecycle) = full_lifecycle(now);
    let ledger = Ledger::session();

    lifecycle
        .record(&ledger)
        .expect("the session ledger accepts the append");

    let events = ono_temporal_core::LedgerRead::events(
        &ledger,
        &ono_temporal_core::EventQuery::in_range(ono_temporal_core::TimeRange::all()),
    )
    .expect("the ledger answers");
    assert_eq!(
        events.len(),
        lifecycle.events().len(),
        "§55.11 case 48: plan lifecycle events appear in the v0.5 timeline"
    );
}

// ---- §22.2: the pre-plan checkpoint ---------------------------------------------------------------

#[test]
fn should_take_no_checkpoint_when_the_ledger_would_not_survive_it() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let ledger = Ledger::session();

    let projection = checkpoint_before_mutation(&plan, &scope(), &[], &[], &ledger, now);

    assert!(
        projection.is_none(),
        "§22.2 exists so a later session can compare, and a session ledger does not outlive it"
    );
}

#[test]
fn should_take_a_checkpoint_when_the_ledger_is_persistent() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let directory = tempfile::tempdir().expect("a temporary directory");
    let ledger = Ledger::persistent(
        &ono_temporal_ledger::StoreOptions::at(&directory.path().join("ledger.sqlite3")),
    )
    .expect("a persistent ledger opens");

    let projection = checkpoint_before_mutation(&plan, &scope(), &[], &[], &ledger, now)
        .expect("§22.2 takes a checkpoint immediately before mutation");

    assert_eq!(projection.checkpoint().captured_at, now);
    assert!(
        projection.checkpoint().objects.is_empty(),
        "a checkpoint holds what the caller could actually see, and nothing more"
    );
}

#[test]
fn should_keep_only_the_objects_the_plan_reaches() {
    let now = instant(1_000);
    let directory = tempfile::tempdir().expect("a temporary directory");
    let ledger = Ledger::persistent(
        &ono_temporal_ledger::StoreOptions::at(&directory.path().join("ledger.sqlite3")),
    )
    .expect("a persistent ledger opens");
    let inside = ono_spatial_core::SpatialId::new(
        ono_spatial_core::IdentityTier::Stable,
        ono_spatial_core::SpatialType::Service,
        &["nginx.service"],
    );
    let outside = ono_spatial_core::SpatialId::new(
        ono_spatial_core::IdentityTier::Stable,
        ono_spatial_core::SpatialType::Service,
        &["postgresql.service"],
    );
    let plan = {
        let base = PlanSpec::default().seal(now);
        let targets = vec![
            ono_change_core::FrozenTarget::new("ono.service/1", "nginx.service", "nginx")
                .at_place(inside.as_str()),
        ];
        base.revise()
            .resolve(targets)
            .expect("a draft resolves")
            .seal(now)
            .expect("re-seals")
    };
    let objects = vec![
        object_state(&inside, now),
        object_state(&outside, now),
    ];

    let projection = checkpoint_before_mutation(&plan, &scope(), &objects, &[], &ledger, now)
        .expect("a persistent ledger takes the checkpoint");

    assert_eq!(
        projection.checkpoint().objects.len(),
        1,
        "§22.2: the checkpoint covers plan targets and impact-relevant objects, not the world"
    );
    assert_eq!(projection.checkpoint().objects[0].id, inside);
}

fn object_state(
    id: &ono_spatial_core::SpatialId,
    now: jiff::Timestamp,
) -> ono_temporal_core::ObjectState {
    ono_temporal_core::ObjectState {
        id: id.clone(),
        object_type: ono_spatial_core::SpatialType::Service,
        label: Arc::from("a service"),
        record: ono_value::RecordValue::new(
            ono_value::SchemaId::new("ono.service", 1),
            ono_value::MapValue::new(),
            ono_value::Provenance::local("ono.session", ono_value::SchemaId::new("ono.service", 1)),
        ),
        observed_at: now,
        source: ono_temporal_core::EvidenceSource::session(),
    }
}

#[test]
fn should_stage_one_event_per_contract_a_plan_carries() {
    let now = instant(1_000);
    let plan = PlanSpec::default()
        .checking(VerificationClass::Advisory, "worker count")
        .seal(now);
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    for contract in plan.verification().contracts() {
        let result = VerificationResult::new(
            plan.id().clone(),
            contract,
            VerificationStatus::Passed,
            now,
        );
        lifecycle.verification_observed(&result, now);
    }
    let observed = subtypes(&lifecycle)
        .iter()
        .filter(|subtype| *subtype == VERIFICATION_OBSERVED)
        .count();

    assert_eq!(
        observed, 2,
        "§23.3: each check reports on its own, and §22.1 records each report"
    );
}

#[test]
fn should_name_the_check_and_its_class_in_the_observation_body() {
    let now = instant(1_000);
    let plan = PlanSpec::default().seal(now);
    let contract: &VerificationContract = &plan.verification().contracts()[0];
    let result = VerificationResult::new(
        plan.id().clone(),
        contract,
        VerificationStatus::Failed,
        now,
    );
    let mut lifecycle = PlanLifecycle::created(&plan, scope(), now);
    lifecycle.verification_observed(&result, now);
    let last = lifecycle.events().last().expect("an event was staged");
    let payload = last
        .payload
        .as_ref()
        .and_then(|value| value.as_map().ok().cloned())
        .expect("the body is a map");

    assert_eq!(
        payload.get("status").cloned(),
        Some(ono_value::Value::string("failed"))
    );
    assert_eq!(
        payload.get("class").cloned(),
        Some(ono_value::Value::string("required")),
        "§23.2: how much the failure says about the plan travels with it"
    );
}
