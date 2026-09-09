#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! The persistent plan store, through its public API (spec v0.6 §36, §41.2, §42.3, §42.4, §52.5).
//!
//! Three questions run through the file. Does a sealed plan survive shell exit as the same plan
//! (§36.1)? Can plan state be reconstructed from persisted action records after a crash (§41.2)?
//! And does the store stop two sessions from applying one sealed plan (§42.4)? Everything else —
//! prefix references (§36.4), the cleanup preview's dependency query (§37.3), secret handles
//! (§36.3) — is a property one of those three depends on.

use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, ActionStatus, ChangePlan, ConsistencyClass, DomainCoverage, DomainProtection,
    EffectConfidence, EffectDomain, EffectKind, Execution, FrozenTarget, Idempotency, ImpactClass,
    ImpactGraph, ImpactNode, Intent, PlanAction, PlanFragment, PlanId, PlanKind, PlanState,
    Precondition, PreconditionKind, ProposedEffect, ProtectionSummary, RecoveryAsset,
    RecoveryAssetType, RecoveryObjective, RecoveryScope, VerificationClass, VerificationContract,
};
use ono_change_plan::builder::PlanBuilder;
use ono_change_plan::freeze::ServiceTarget;
use ono_change_plan::secrets::SecretRedaction;
use ono_change_plan::store::{PlanFilter, PlanStore, StoreOptions};
use ono_change_plan::{PlanGranularity, rebase};
use ono_core::ErrorCode;
use ono_value::Value;
use tempfile::TempDir;

// ---------------------------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------------------------

fn at(second: i64) -> Timestamp {
    Timestamp::from_second(second).expect("a valid instant")
}

fn store() -> (TempDir, PlanStore) {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let store = PlanStore::open(&directory.path().join("plans.sqlite3")).expect("a store opens");
    (directory, store)
}

fn service(unit: &str) -> FrozenTarget {
    ServiceTarget::new("systemd", unit)
        .resolved_from("get service | where state == failed")
        .freeze()
        .expect("a namespaced unit freezes")
}

fn build(session: &str, intent: &str, units: &[&str]) -> PlanBuilder {
    contributed(session, intent)
        .resolve(units.iter().map(|unit| service(unit)).collect())
        .expect("the targets resolve")
}

/// A plan with a PREPARE, a MUTATE and a VERIFY action, which is what §41.2's resume reasons over.
fn contributed(session: &str, intent: &str) -> PlanBuilder {
    let builder = PlanBuilder::for_intent(
        Intent::new(intent.to_owned(), "plan restart service nginx"),
        session.to_owned(),
        at(0),
    );
    let id = builder.plan_id().clone();
    let prepare = PlanAction::new(
        &id,
        1,
        ActionRole::Prepare,
        "snapshot rpool/etc before the change",
        Execution::RecoveryOperation {
            provider: Arc::from("ono.recovery.zfs"),
            capability: Arc::from("recovery.prepare"),
            arguments: vec![(Arc::from("dataset"), Value::string("rpool/etc"))],
        },
    )
    .with_idempotency(Idempotency::Idempotent);
    let mutate = PlanAction::new(
        &id,
        2,
        ActionRole::Mutate,
        "restart the failed services",
        Execution::ProviderAction {
            provider: Arc::from("ono.service.systemd"),
            operation: Arc::from("ono.service.restart"),
            arguments: vec![(Arc::from("mode"), Value::string("replace"))],
        },
    )
    .after(prepare.id().clone())
    .with_idempotency(Idempotency::NonIdempotent)
    .requiring(
        Precondition::new(
            PreconditionKind::Generation,
            "systemd:nginx.service",
            "generation",
            Value::string("inv-1"),
        )
        .explained("the unit must still be the one the plan froze"),
    );
    let mutate_id = mutate.id().clone();
    let mutate = mutate.effecting(
        ProposedEffect::new(
            mutate_id,
            EffectDomain::ProcessRuntime,
            EffectKind::Replace,
            EffectConfidence::Guaranteed,
            "the worker processes are replaced",
        )
        .on("systemd:nginx.service"),
    );
    let verify = PlanAction::new(
        &id,
        3,
        ActionRole::Verify,
        "observe that the services are running",
        Execution::Program {
            program: Arc::from("/usr/bin/systemctl"),
            argv: vec![Arc::from("is-active"), Arc::from("nginx.service")],
        },
    );
    let fragment = PlanFragment::empty()
        .at_version("255.7")
        .acting(prepare)
        .acting(mutate)
        .acting(verify)
        .verifying(
            VerificationContract::new(
                &id,
                VerificationClass::Required,
                "systemd:nginx.service",
                "state == running",
            )
            .expecting(Value::string("running"))
            .within(Duration::from_secs(45)),
        );
    builder
        .contributing(&fragment)
        .expect("a fragment is accepted")
}

fn sealed(session: &str, intent: &str, units: &[&str]) -> ChangePlan {
    build(session, intent, units)
        .seal(at(60))
        .expect("a plan seals")
}

fn plan() -> ChangePlan {
    sealed(
        "session-1",
        "restart the failed services",
        &["nginx.service"],
    )
}

fn asset(created_at: Timestamp) -> RecoveryAsset {
    RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        "rpool/etc@ono-a82f",
        RecoveryScope::new("zfs-dataset", "rpool/etc", "host-1").covering("/etc/nginx/nginx.conf"),
        created_at,
    )
}

// ---------------------------------------------------------------------------------------------
// §36.1 — persistence
// ---------------------------------------------------------------------------------------------

#[test]
fn should_return_the_same_plan_it_was_given_when_a_sealed_plan_round_trips() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert_eq!(
        read.digest(),
        plan.digest(),
        "§36.1: a sealed plan survives storage as the plan it was"
    );
    assert!(
        read.digest_holds(),
        "§4.4: the seal still describes the plan after a round trip"
    );
    assert_eq!(read.targets(), plan.targets());
    assert_eq!(read.actions().len(), plan.actions().len());
    assert_eq!(read.state(), PlanState::Sealed);
}

#[test]
fn should_keep_the_plan_after_the_shell_exits_and_starts_again() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("plans.sqlite3");
    let plan = plan();
    {
        let store = PlanStore::open(&path).expect("a store opens");
        store.put(&plan).expect("a sealed plan is persisted");
    }
    let store = PlanStore::open(&path).expect("the store reopens");
    let read = store.get(plan.id()).expect("the plan survived the exit");
    assert_eq!(
        read.intent().text(),
        plan.intent().text(),
        "§36.1: sealed plans MUST be persisted so they survive shell exit"
    );
}

#[test]
fn should_keep_the_frozen_target_identities_across_storage() {
    let (_directory, store) = store();
    let plan = sealed(
        "session-1",
        "restart the failed services",
        &["a.service", "b.service", "c.service"],
    );
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    let identities: Vec<&str> = read.targets().iter().map(FrozenTarget::identity).collect();
    assert_eq!(
        identities,
        vec![
            "systemd:a.service",
            "systemd:b.service",
            "systemd:c.service"
        ],
        "§28.2: membership froze at resolution and storage did not renegotiate it"
    );
}

#[test]
fn should_keep_the_selector_provenance_across_storage() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert_eq!(
        read.targets()[0].selector(),
        Some("get service | where state == failed"),
        "§4.3: the selector survives as provenance for `explain`"
    );
}

#[test]
fn should_keep_the_preconditions_a_provider_declared() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    let preconditions: Vec<&str> = read
        .actions()
        .iter()
        .flat_map(PlanAction::preconditions)
        .map(Precondition::field)
        .collect();
    assert_eq!(
        preconditions,
        vec!["generation"],
        "§7.2: the facts apply revalidates against are part of the persisted plan"
    );
}

#[test]
fn should_keep_the_impact_graph_beside_the_plan() {
    let (_directory, store) = store();
    let mut graph = ImpactGraph::empty();
    graph.add(ImpactNode::new(
        "systemd:postgres.service",
        "postgres",
        "ono.service/1",
        ImpactClass::Dependent,
        1,
    ));
    let plan = build(
        "session-1",
        "restart the failed services",
        &["nginx.service"],
    )
    .with_impact(graph)
    .seal(at(60))
    .expect("a plan seals");
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert_eq!(
        read.impact().nodes().len(),
        1,
        "§9: the graph the operator was shown is the one the store keeps"
    );
}

#[test]
fn should_refuse_a_plan_the_store_never_held() {
    let (_directory, store) = store();
    let refusal = store
        .get(&PlanId::derive(&["absent"]))
        .expect_err("§36.4 refuses a plan nobody stored");
    assert_eq!(refusal.code(), ErrorCode::ChangePlanNotFound);
}

// ---------------------------------------------------------------------------------------------
// §7.5 — revisions
// ---------------------------------------------------------------------------------------------

#[test]
fn should_answer_with_the_latest_revision_after_a_rebase() {
    let (_directory, store) = store();
    let original = sealed(
        "session-1",
        "restart the failed services",
        &["a.service", "b.service"],
    );
    store.put(&original).expect("a sealed plan is persisted");
    let next = rebase(&original, vec![service("a.service")], at(120)).expect("§7.5 rebases");
    store.put(&next).expect("the revision is persisted");
    let read = store.get(original.id()).expect("the plan comes back");
    assert_eq!(read.revision(), 2, "§7.5: the newest revision is the plan");
}

#[test]
fn should_still_hold_the_sealed_original_after_a_rebase() {
    let (_directory, store) = store();
    let original = sealed(
        "session-1",
        "restart the failed services",
        &["a.service", "b.service"],
    );
    store.put(&original).expect("a sealed plan is persisted");
    let next = rebase(&original, vec![service("a.service")], at(120)).expect("§7.5 rebases");
    store.put(&next).expect("the revision is persisted");
    let first = store
        .get_revision(original.id(), 1)
        .expect("revision 1 is still there");
    assert_eq!(
        first.digest(),
        original.digest(),
        "§7.5: rebase MUST NOT mutate the sealed original, and the store keeps it"
    );
    assert_eq!(first.targets().len(), 2);
}

// ---------------------------------------------------------------------------------------------
// §36.4 — references
// ---------------------------------------------------------------------------------------------

#[test]
fn should_resolve_a_short_reference_to_the_plan_it_names() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let resolved = store
        .resolve(plan.id().short())
        .expect("§36.4: `get plan a82f` resolves");
    assert_eq!(resolved, *plan.id());
}

#[test]
fn should_resolve_a_reference_written_with_its_marker() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let resolved = store
        .resolve(&format!("plan/{}", plan.id().short()))
        .expect("§36.4: `apply plan/a82f` resolves");
    assert_eq!(resolved, *plan.id());
}

#[test]
fn should_refuse_a_reference_that_matches_no_plan() {
    let (_directory, store) = store();
    store.put(&plan()).expect("a sealed plan is persisted");
    let refusal = store
        .resolve("ffffffffffffffff")
        .expect_err("§36.4 refuses an identity nobody stored");
    assert_eq!(refusal.code(), ErrorCode::ChangePlanNotFound);
}

#[test]
fn should_refuse_a_reference_that_is_not_an_identity_rather_than_search_for_it() {
    let (_directory, store) = store();
    let refusal = store
        .resolve("../etc/passwd")
        .expect_err("§36.4 refuses text that is not an identity");
    assert_eq!(refusal.code(), ErrorCode::ChangePlanNotFound);
}

#[test]
fn should_refuse_to_resolve_a_recovery_reference_against_the_plan_table() {
    let (_directory, store) = store();
    store.put(&plan()).expect("a sealed plan is persisted");
    let refusal = store
        .resolve("recovery/a82f")
        .expect_err("§37.5's marker names an asset, not a plan");
    assert_eq!(refusal.code(), ErrorCode::ChangePlanNotFound);
}

/// Two intents whose plan identities share `width` leading characters, found deterministically.
fn colliding_intents(width: usize) -> (String, String) {
    let mut seen: Vec<(String, String)> = Vec::new();
    for index in 0..20_000_u32 {
        let intent = format!("restart the failed services {index}");
        let id = PlanId::of("session-1", &at(0).to_string(), &intent);
        let prefix = id.as_str()[..width].to_owned();
        if let Some((_, earlier)) = seen.iter().find(|(seen, _)| *seen == prefix) {
            return (earlier.clone(), intent);
        }
        seen.push((prefix, intent));
    }
    panic!("no two of twenty thousand identities share {width} characters");
}

#[test]
fn should_widen_a_printed_reference_until_it_names_one_plan() {
    let (_directory, store) = store();
    let (first, second) = colliding_intents(4);
    let one = sealed("session-1", &first, &["a.service"]);
    let two = sealed("session-1", &second, &["b.service"]);
    store.put(&one).expect("a sealed plan is persisted");
    store.put(&two).expect("a second sealed plan is persisted");
    let width = store.reference_width().expect("the store answers");
    assert!(
        width > 4,
        "§36.4: two plans sharing four characters must render more, got {width}"
    );
    let rendered = store.render_reference(one.id()).expect("the store answers");
    assert_eq!(
        store
            .resolve(&rendered)
            .expect("a printed reference resolves"),
        *one.id(),
        "§36.4: a printed reference must stay unambiguous when it is typed back"
    );
}

#[test]
fn should_refuse_an_ambiguous_reference_and_name_the_candidates() {
    let (_directory, store) = store();
    let (first, second) = colliding_intents(4);
    let one = sealed("session-1", &first, &["a.service"]);
    let two = sealed("session-1", &second, &["b.service"]);
    store.put(&one).expect("a sealed plan is persisted");
    store.put(&two).expect("a second sealed plan is persisted");
    let refusal = store
        .resolve(one.id().short())
        .expect_err("§36.4 refuses a reference that names two plans");
    assert_eq!(refusal.code(), ErrorCode::ChangePlanReferenceAmbiguous);
    let candidates = refusal
        .metadata()
        .get("candidates")
        .cloned()
        .expect("§36.4's refusal names what it matched");
    let Value::List(items) = candidates else {
        panic!("the candidate list is a list");
    };
    assert_eq!(items.len(), 2, "§36.4: write enough to tell them apart");
}

// ---------------------------------------------------------------------------------------------
// §5.5 — `get plan`
// ---------------------------------------------------------------------------------------------

#[test]
fn should_list_the_plans_the_store_holds_newest_first() {
    let (_directory, store) = store();
    let older = sealed("session-1", "restart a", &["a.service"]);
    let newer = build("session-1", "restart b", &["b.service"])
        .expiring_at(at(9_000))
        .seal(at(60))
        .expect("a plan seals");
    store.put(&older).expect("a sealed plan is persisted");
    store
        .put(&newer)
        .expect("a second sealed plan is persisted");
    let rows = store.list(&PlanFilter::all()).expect("the store answers");
    assert_eq!(rows.len(), 2, "§5.5: `get plan` lists what the store holds");
    assert!(rows.iter().all(|row| row.state == PlanState::Sealed));
}

#[test]
fn should_list_only_the_latest_revision_of_each_plan_by_default() {
    let (_directory, store) = store();
    let original = sealed("session-1", "restart a", &["a.service", "b.service"]);
    let next = rebase(&original, vec![service("a.service")], at(120)).expect("§7.5 rebases");
    store.put(&original).expect("a sealed plan is persisted");
    store.put(&next).expect("the revision is persisted");
    let rows = store.list(&PlanFilter::all()).expect("the store answers");
    assert_eq!(rows.len(), 1, "§7.5: one plan, whatever its revision count");
    assert_eq!(rows[0].revision, 2);
}

#[test]
fn should_list_every_revision_when_the_superseded_ones_are_asked_for() {
    let (_directory, store) = store();
    let original = sealed("session-1", "restart a", &["a.service", "b.service"]);
    let next = rebase(&original, vec![service("a.service")], at(120)).expect("§7.5 rebases");
    store.put(&original).expect("a sealed plan is persisted");
    store.put(&next).expect("the revision is persisted");
    let rows = store
        .list(&PlanFilter::all().including_superseded())
        .expect("the store answers");
    assert_eq!(rows.len(), 2, "§7.5: the earlier revision is still a fact");
}

#[test]
fn should_list_only_the_plans_of_one_session() {
    let (_directory, store) = store();
    store
        .put(&sealed("session-1", "restart a", &["a.service"]))
        .expect("a sealed plan is persisted");
    store
        .put(&sealed("session-2", "restart b", &["b.service"]))
        .expect("a second sealed plan is persisted");
    let rows = store
        .list(&PlanFilter::all().in_session("session-2"))
        .expect("the store answers");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].session.as_ref(), "session-2");
}

#[test]
fn should_list_only_the_plans_in_one_lifecycle_state() {
    let (_directory, store) = store();
    store
        .put(&sealed("session-1", "restart a", &["a.service"]))
        .expect("a sealed plan is persisted");
    let rows = store
        .list(&PlanFilter::all().in_state(PlanState::Applying))
        .expect("the store answers");
    assert!(
        rows.is_empty(),
        "§4.1: a sealed plan is not an applying one, and the filter says so"
    );
}

#[test]
fn should_list_only_the_plans_of_one_kind() {
    let (_directory, store) = store();
    store
        .put(&sealed("session-1", "restart a", &["a.service"]))
        .expect("a sealed plan is persisted");
    let recovery = store
        .list(&PlanFilter::all().of_kind(PlanKind::Recovery))
        .expect("the store answers");
    assert!(
        recovery.is_empty(),
        "§24.1: a change plan and a recovery plan are told apart on the row"
    );
    let change = store
        .list(&PlanFilter::all().of_kind(PlanKind::Change))
        .expect("the store answers");
    assert_eq!(change.len(), 1);
}

#[test]
fn should_limit_the_number_of_rows_it_returns() {
    let (_directory, store) = store();
    for index in 0..5 {
        store
            .put(&sealed(
                "session-1",
                &format!("restart {index}"),
                &["a.service"],
            ))
            .expect("a sealed plan is persisted");
    }
    let rows = store
        .list(&PlanFilter::all().limited_to(2))
        .expect("the store answers");
    assert_eq!(rows.len(), 2);
}

#[test]
fn should_put_a_reference_on_every_row_that_resolves_back_to_its_plan() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let rows = store.list(&PlanFilter::all()).expect("the store answers");
    let reference = rows[0].reference.clone();
    assert!(reference.starts_with("plan/"), "got {reference}");
    assert_eq!(
        store.resolve(&reference).expect("the reference resolves"),
        *plan.id(),
        "§36.4: what `get plan` printed is what `apply` accepts"
    );
}

#[test]
fn should_carry_the_seal_digest_onto_the_listed_row() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let rows = store.list(&PlanFilter::all()).expect("the store answers");
    assert_eq!(
        rows[0].digest.as_deref(),
        plan.digest(),
        "§4.4: the row shows the seal the plan carries"
    );
}

#[test]
fn should_forget_a_plan_that_was_removed() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    store.remove(plan.id()).expect("the plan is removed");
    assert!(
        store
            .list(&PlanFilter::all())
            .expect("the store answers")
            .is_empty(),
        "a removed plan is gone from `get plan` too"
    );
}

// ---------------------------------------------------------------------------------------------
// §41.2 — reconstruction from persisted action records
// ---------------------------------------------------------------------------------------------

#[test]
fn should_reconstruct_the_per_action_statuses_after_the_shell_restarts() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("plans.sqlite3");
    let plan = plan();
    {
        let store = PlanStore::open(&path).expect("a store opens");
        store.put(&plan).expect("a sealed plan is persisted");
        // A crash mid-apply: the snapshot was taken, the restart's outcome was never established,
        // the verification never ran.
        store
            .record_action_status(
                plan.id(),
                1,
                plan.actions()[0].id(),
                1,
                ActionStatus::Succeeded,
                at(70),
                Some("rpool/etc@ono-a82f exists"),
            )
            .expect("evidence is recorded");
        store
            .record_action_status(
                plan.id(),
                1,
                plan.actions()[1].id(),
                2,
                ActionStatus::Unknown,
                at(75),
                Some("the link dropped while the restart was in flight"),
            )
            .expect("evidence is recorded");
    }
    let store = PlanStore::open(&path).expect("the store reopens");
    let read = store.get(plan.id()).expect("the plan comes back");
    let statuses: Vec<ActionStatus> = read.actions().iter().map(PlanAction::status).collect();
    assert_eq!(
        statuses,
        vec![
            ActionStatus::Succeeded,
            ActionStatus::Unknown,
            ActionStatus::Pending
        ],
        "§41.2: plan state MUST be reconstructable from persisted action records"
    );
}

#[test]
fn should_bring_an_unestablished_outcome_back_as_unknown_and_not_as_failure() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    store
        .record_action_status(
            plan.id(),
            1,
            plan.actions()[1].id(),
            2,
            ActionStatus::Unknown,
            at(75),
            None,
        )
        .expect("evidence is recorded");
    let read = store.get(plan.id()).expect("the plan comes back");
    let recovered = read.actions()[1].status();
    assert_eq!(
        recovered,
        ActionStatus::Unknown,
        "Appendix F.2: an unestablished outcome is not a failure and not a success"
    );
    assert!(
        recovered.may_have_mutated(),
        "Appendix F.2: unknown is an uncertainty boundary, so it may have changed the system"
    );
}

#[test]
fn should_not_let_resume_rerun_a_non_idempotent_action_whose_outcome_is_unknown() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    store
        .record_action_status(
            plan.id(),
            1,
            plan.actions()[1].id(),
            2,
            ActionStatus::Unknown,
            at(75),
            None,
        )
        .expect("evidence is recorded");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert!(
        !read.actions()[1].may_resume(),
        "§41.2: Ono MUST NOT blindly rerun unknown or non-idempotent actions"
    );
}

#[test]
fn should_keep_the_action_evidence_when_the_same_revision_is_written_again() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    store
        .record_action_status(
            plan.id(),
            1,
            plan.actions()[0].id(),
            1,
            ActionStatus::Succeeded,
            at(70),
            None,
        )
        .expect("evidence is recorded");
    store.put(&plan).expect("the plan is written again");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert_eq!(
        read.actions()[0].status(),
        ActionStatus::Succeeded,
        "§41.2: re-persisting a plan must not erase the evidence resume depends on"
    );
}

#[test]
fn should_update_an_action_status_as_the_action_settles() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    for status in [ActionStatus::Running, ActionStatus::Succeeded] {
        store
            .record_action_status(
                plan.id(),
                1,
                plan.actions()[0].id(),
                1,
                status,
                at(70),
                None,
            )
            .expect("evidence is recorded");
    }
    let statuses = store
        .action_statuses(plan.id(), 1)
        .expect("the store answers");
    assert_eq!(
        statuses.get(plan.actions()[0].id().as_str()),
        Some(&ActionStatus::Succeeded),
        "§41.2: the record is what the executor last established, not the first thing it saw"
    );
}

#[test]
fn should_keep_the_seal_verifiable_after_action_evidence_is_laid_over_it() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    store
        .record_action_status(
            plan.id(),
            1,
            plan.actions()[0].id(),
            1,
            ActionStatus::Succeeded,
            at(70),
            None,
        )
        .expect("evidence is recorded");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert!(
        read.digest_holds(),
        "§4.4: what an action did is not part of what the plan is, so the seal still holds"
    );
}

// ---------------------------------------------------------------------------------------------
// §42.3, §42.4 — the apply claim
// ---------------------------------------------------------------------------------------------

#[test]
fn should_let_one_session_claim_a_sealed_plan_for_apply() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let claim = store
        .claim(plan.id(), "session-1", at(100))
        .expect("§42.4: the first session takes the claim");
    assert_eq!(claim.session(), "session-1");
    assert_eq!(claim.plan(), plan.id());
    assert!(claim.expires_at() > at(100), "§42.3: the lock is bounded");
}

#[test]
fn should_refuse_a_second_session_applying_the_same_sealed_plan() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let _held = store
        .claim(plan.id(), "session-1", at(100))
        .expect("the first session takes the claim");
    let refusal = store
        .claim(plan.id(), "session-2", at(120))
        .expect_err("§42.4 refuses the second session");
    assert_eq!(
        refusal.code(),
        ErrorCode::ChangePlanAlreadyApplying,
        "§42.4: the plan store MUST prevent two sessions applying one sealed plan"
    );
    assert_eq!(
        refusal.metadata().get("holder"),
        Some(&Value::string("session-1")),
        "§45: the refusal names who holds it, so the operator knows who to ask"
    );
}

#[test]
fn should_release_the_claim_when_it_is_dropped() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    {
        let _held = store
            .claim(plan.id(), "session-1", at(100))
            .expect("the first session takes the claim");
    }
    let taken = store
        .claim(plan.id(), "session-2", at(120))
        .expect("§42.3: a released lock is available again");
    assert_eq!(taken.session(), "session-2");
}

#[test]
fn should_release_the_claim_when_the_apply_returned_early() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let attempt = || -> Result<(), &'static str> {
        let _held = store
            .claim(plan.id(), "session-1", at(100))
            .map_err(|_| "the claim was refused")?;
        Err("preparation failed")
    };
    assert!(attempt().is_err());
    assert!(
        store
            .claim_holder(plan.id(), at(120))
            .expect("the store answers")
            .is_none(),
        "§42.3: locks MUST be bounded and released on failure"
    );
}

#[test]
fn should_let_a_later_session_take_over_a_claim_whose_lease_expired() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let held = store
        .claim_for(plan.id(), "session-1", at(100), Duration::from_secs(10))
        .expect("the first session takes the claim");
    std::mem::forget(held);
    let taken = store
        .claim(plan.id(), "session-2", at(200))
        .expect("§42.3: a crashed session's claim must not block the plan forever");
    assert_eq!(taken.session(), "session-2");
}

#[test]
fn should_refuse_a_take_over_while_the_lease_is_still_live() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let held = store
        .claim_for(plan.id(), "session-1", at(100), Duration::from_secs(600))
        .expect("the first session takes the claim");
    std::mem::forget(held);
    let refusal = store
        .claim(plan.id(), "session-2", at(200))
        .expect_err("§42.4 refuses while the lease is live");
    assert_eq!(refusal.code(), ErrorCode::ChangePlanAlreadyApplying);
}

#[test]
fn should_report_no_holder_once_the_lease_has_passed() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let held = store
        .claim_for(plan.id(), "session-1", at(100), Duration::from_secs(10))
        .expect("the first session takes the claim");
    std::mem::forget(held);
    assert_eq!(
        store
            .claim_holder(plan.id(), at(105))
            .expect("the store answers")
            .as_deref(),
        Some("session-1"),
        "§42.4: a live claim has a holder"
    );
    assert!(
        store
            .claim_holder(plan.id(), at(200))
            .expect("the store answers")
            .is_none(),
        "§42.3: an expired lease holds nothing"
    );
}

#[test]
fn should_extend_the_lease_while_an_apply_is_still_making_progress() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let mut claim = store
        .claim_for(plan.id(), "session-1", at(100), Duration::from_secs(10))
        .expect("the first session takes the claim");
    let first = claim.expires_at();
    claim
        .renew(at(105))
        .expect("§42.3: a lease may be extended");
    assert!(
        claim.expires_at() > first,
        "§42.3: an apply still making progress keeps its lock"
    );
}

#[test]
fn should_let_the_holding_session_take_its_own_claim_again() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    let held = store
        .claim(plan.id(), "session-1", at(100))
        .expect("the first session takes the claim");
    std::mem::forget(held);
    let again = store
        .claim(plan.id(), "session-1", at(120))
        .expect("§42.4 is about two sessions, and this is one");
    assert_eq!(again.session(), "session-1");
}

#[test]
fn should_leave_a_claim_alone_that_another_session_took_over() {
    let (_directory, store) = store();
    let plan = plan();
    store.put(&plan).expect("a sealed plan is persisted");
    {
        let _first = store
            .claim_for(plan.id(), "session-1", at(100), Duration::from_secs(10))
            .expect("the first session takes the claim");
        let second = store
            .claim(plan.id(), "session-2", at(200))
            .expect("the lease expired and the second session took over");
        std::mem::forget(second);
        // `_first` drops here, and it must not release the claim it no longer holds.
    }
    assert_eq!(
        store
            .claim_holder(plan.id(), at(220))
            .expect("the store answers")
            .as_deref(),
        Some("session-2"),
        "§42.4: a stale holder releasing somebody else's lock would be worse than not releasing"
    );
}

// ---------------------------------------------------------------------------------------------
// §11, §37 — recovery assets
// ---------------------------------------------------------------------------------------------

#[test]
fn should_return_the_same_asset_it_was_given() {
    let (_directory, store) = store();
    let recovery = asset(at(30));
    store.put_asset(&recovery).expect("an asset is persisted");
    let read = store
        .get_asset(recovery.id())
        .expect("the asset comes back");
    assert_eq!(read.reference(), "rpool/etc@ono-a82f");
    assert_eq!(read.scope().domain(), "rpool/etc");
    assert!(
        read.scope().covers_object("/etc/nginx/nginx.conf"),
        "§11.2: what an asset covers is exact membership, and storage keeps it exact"
    );
}

#[test]
fn should_refuse_an_asset_the_store_never_held() {
    let (_directory, store) = store();
    let refusal = store
        .get_asset(&ono_change_core::RecoveryAssetId::derive(&["absent"]))
        .expect_err("§37.5 refuses an asset nobody stored");
    assert_eq!(refusal.code(), ErrorCode::RecoveryAssetNotFound);
}

#[test]
fn should_list_every_asset_the_store_holds() {
    let (_directory, store) = store();
    store
        .put_asset(&asset(at(30)))
        .expect("an asset is persisted");
    store
        .put_asset(&asset(at(40)))
        .expect("a second asset is persisted");
    assert_eq!(
        store.list_assets().expect("the store answers").len(),
        2,
        "§37.5: `get recovery` lists what the store holds"
    );
}

#[test]
fn should_resolve_a_short_asset_reference() {
    let (_directory, store) = store();
    let recovery = asset(at(30));
    store.put_asset(&recovery).expect("an asset is persisted");
    let resolved = store
        .resolve_asset(&format!("recovery/{}", recovery.id().short()))
        .expect("§37.5: a short asset reference resolves");
    assert_eq!(resolved, *recovery.id());
}

#[test]
fn should_refuse_an_asset_reference_that_matches_nothing() {
    let (_directory, store) = store();
    let refusal = store
        .resolve_asset("recovery/ffffffffffffffff")
        .expect_err("§37.5 refuses an asset nobody stored");
    assert_eq!(refusal.code(), ErrorCode::RecoveryAssetNotFound);
}

#[test]
fn should_name_the_assets_a_plans_protection_rests_on() {
    let (_directory, store) = store();
    let recovery = asset(at(30));
    let plan = build("session-1", "restart a", &["a.service"])
        .with_protection(ProtectionSummary::of(vec![
            DomainCoverage::new(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
                "the zfs snapshot holds the prior bytes",
            )
            .by_asset(recovery.id().clone())
            .at_consistency(ConsistencyClass::FilesystemConsistent),
        ]))
        .seal(at(60))
        .expect("a plan seals");
    store.put_asset(&recovery).expect("an asset is persisted");
    store.put(&plan).expect("a sealed plan is persisted");
    assert_eq!(
        store.assets_for(plan.id()).expect("the store answers"),
        vec![recovery.id().clone()],
        "§10.3: the coverage matrix names the assets the plan's protection rests on"
    );
}

#[test]
fn should_name_the_plans_that_become_unrecoverable_if_an_asset_is_removed() {
    let (_directory, store) = store();
    let recovery = asset(at(30));
    let plan = build("session-1", "restart a", &["a.service"])
        .with_protection(ProtectionSummary::of(vec![
            DomainCoverage::new(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
                "the zfs snapshot holds the prior bytes",
            )
            .by_asset(recovery.id().clone()),
        ]))
        .seal(at(60))
        .expect("a plan seals");
    store.put_asset(&recovery).expect("an asset is persisted");
    store.put(&plan).expect("a sealed plan is persisted");
    assert_eq!(
        store
            .plans_depending_on(recovery.id())
            .expect("the store answers"),
        vec![plan.id().clone()],
        "§37.3: the cleanup preview shows which plans become unrecoverable"
    );
}

#[test]
fn should_name_a_plan_that_created_an_asset_as_depending_on_it() {
    let (_directory, store) = store();
    let plan = plan();
    let recovery = asset(at(30)).for_plan(plan.id().clone());
    store.put_asset(&recovery).expect("an asset is persisted");
    store.put(&plan).expect("a sealed plan is persisted");
    assert_eq!(
        store
            .plans_depending_on(recovery.id())
            .expect("the store answers"),
        vec![plan.id().clone()],
        "§2.15: an asset a retained plan required MUST NOT be deleted silently"
    );
    assert_eq!(
        store.assets_for(plan.id()).expect("the store answers"),
        vec![recovery.id().clone()]
    );
}

#[test]
fn should_name_nobody_for_an_asset_no_plan_rests_on() {
    let (_directory, store) = store();
    let recovery = asset(at(30));
    store.put_asset(&recovery).expect("an asset is persisted");
    assert!(
        store
            .plans_depending_on(recovery.id())
            .expect("the store answers")
            .is_empty(),
        "§37.3: an asset nothing depends on can be removed without a preview to show"
    );
}

// ---------------------------------------------------------------------------------------------
// §36.3 — secrets
// ---------------------------------------------------------------------------------------------

fn plan_with_password(session: &str) -> ChangePlan {
    let builder = PlanBuilder::for_intent(
        Intent::new("set the database password", "plan set password postgres"),
        session.to_owned(),
        at(0),
    );
    let id = builder.plan_id().clone();
    let fragment = PlanFragment::empty()
        .at_version("16.2")
        .acting(
            PlanAction::new(
                &id,
                1,
                ActionRole::Mutate,
                "set the database password",
                Execution::ProviderAction {
                    provider: Arc::from("dev.example.postgres"),
                    operation: Arc::from("ono.database.set-password"),
                    arguments: vec![
                        (Arc::from("user"), Value::string("ono")),
                        (Arc::from("password"), Value::string("hunter2")),
                    ],
                },
            )
            .with_idempotency(Idempotency::Idempotent),
        )
        .verifying(
            VerificationContract::new(
                &id,
                VerificationClass::Required,
                "postgres",
                "authentication succeeds",
            )
            .expecting(Value::Bool(true)),
        );
    builder
        .contributing(&fragment)
        .expect("a fragment is accepted")
        .resolve(vec![service("postgres.service")])
        .expect("one target resolves")
        .seal(at(60))
        .expect("a plan seals")
}

#[test]
fn should_bring_a_plan_back_from_the_store_with_a_handle_instead_of_the_secret() {
    let (_directory, store) = store();
    let plan = plan_with_password("session-1");
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    let Execution::ProviderAction { arguments, .. } = read.actions()[0].execution() else {
        panic!("the action runs a provider operation");
    };
    let password = arguments
        .iter()
        .find(|(name, _)| name.as_ref() == "password")
        .map(|(_, value)| value.clone())
        .expect("§36.3 keeps the argument name");
    let Value::String(handle) = password else {
        panic!("a handle is text");
    };
    assert!(
        SecretRedaction::is_handle(&handle),
        "§36.3: an opaque secret handle stands in for the value, got `{handle}`"
    );
    assert!(!handle.contains("hunter2"));
}

#[test]
fn should_keep_the_seal_verifiable_over_the_handle_after_a_round_trip() {
    let (_directory, store) = store();
    let plan = plan_with_password("session-1");
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert!(
        read.digest_holds(),
        "§36.3 and §4.4: the digest is computed over the handle, so the seal survives storage"
    );
    assert_eq!(read.digest(), plan.digest());
}

#[test]
fn should_keep_the_other_arguments_readable_beside_a_redacted_one() {
    let (_directory, store) = store();
    let plan = plan_with_password("session-1");
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    let Execution::ProviderAction { arguments, .. } = read.actions()[0].execution() else {
        panic!("the action runs a provider operation");
    };
    assert!(
        arguments
            .iter()
            .any(|(name, value)| name.as_ref() == "user" && *value == Value::string("ono")),
        "§36.3 replaces the secret, and leaves the plan readable"
    );
}

#[test]
fn should_redact_a_plan_the_store_was_handed_with_a_raw_secret_in_it() {
    let directory = tempfile::tempdir().expect("a temporary directory");
    let path = directory.path().join("plans.sqlite3");
    // A caller that declared nothing produces a plan carrying the raw value; the store applies
    // §36.3 regardless, because the prohibition is about what is persisted.
    let permissive = PlanStore::open_with(
        &StoreOptions::at(&path).redacting(SecretRedaction::declaring_nothing()),
    )
    .expect("a store opens");
    let plan = plan_with_password("session-1");
    drop(permissive);
    let store = PlanStore::open(&path).expect("the store reopens with the default redaction");
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert!(
        !read.actions()[0]
            .execution()
            .digest_text()
            .contains("hunter2"),
        "§36.3: secrets MUST NOT be persisted in raw form"
    );
}

// ---------------------------------------------------------------------------------------------
// §52.5 — large plans
// ---------------------------------------------------------------------------------------------

#[test]
fn should_round_trip_a_plan_over_thousands_of_targets() {
    let (_directory, store) = store();
    let units: Vec<String> = (0..5_000)
        .map(|index| format!("s{index}.service"))
        .collect();
    let plan = contributed("session-1", "restart the failed services")
        .resolve_streaming(units.iter().map(|unit| Ok(service(unit))))
        .expect("five thousand targets resolve")
        .seal(at(60))
        .expect("a plan seals");
    store.put(&plan).expect("a sealed plan is persisted");
    let read = store.get(plan.id()).expect("the plan comes back");
    assert_eq!(
        read.targets().len(),
        5_000,
        "§52.5: final sealed target identity lists must be durable"
    );
    assert!(
        read.digest_holds(),
        "§4.4: a plan over thousands of targets still verifies against its own seal"
    );
}

// ---------------------------------------------------------------------------------------------
// §5.3 — the per-object plans a pipeline may be asked for
// ---------------------------------------------------------------------------------------------

#[test]
fn should_store_each_per_object_plan_under_its_own_identity() {
    let (_directory, store) = store();
    let plans = build(
        "session-1",
        "restart the failed services",
        &["a.service", "b.service"],
    )
    .seal_with(PlanGranularity::PlanPerObject, at(60))
    .expect("both plans seal");
    for plan in &plans {
        store.put(plan).expect("a sealed plan is persisted");
    }
    let rows = store.list(&PlanFilter::all()).expect("the store answers");
    assert_eq!(
        rows.len(),
        2,
        "§5.3: one plan per input object is two plans, each of which the store keeps"
    );
    for plan in &plans {
        let read = store.get(plan.id()).expect("the plan comes back");
        assert_eq!(read.targets().len(), 1);
        assert!(read.digest_holds());
    }
}
