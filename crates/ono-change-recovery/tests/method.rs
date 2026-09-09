//! Choosing the restore method (v0.6 Appendix C.1, C.2, C.5, C.7, §56.3, §59.6).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    MetadataCoverage, RecoveryAssetId, RecoveryGoal, RecoveryPlanFragment, RestoreMethod,
};
use ono_change_recovery::method::{
    MethodOffer, MethodOffers, MethodRejection, MethodRequest, offers, select,
};
use support::{EPOCH, TestProvider, content_mode_owner, full_metadata, ready_asset, registry};

fn asset_id(provider: &str) -> RecoveryAssetId {
    RecoveryAssetId::of(provider, None, "rpool/ROOT/debian", "0")
}

fn offer(provider: &str, method: RestoreMethod, metadata: MetadataCoverage) -> MethodOffer {
    MethodOffer::new(
        provider,
        asset_id(provider),
        format!("{provider}:asset"),
        RecoveryPlanFragment::new(provider, method).restoring_metadata(metadata),
    )
}

#[test]
fn should_prefer_the_least_destructive_method_that_meets_the_goal() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "zfs",
            RestoreMethod::DatasetRollback,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("Appendix C.1 offers two methods that reach the goal");
    assert_eq!(
        selection.chosen().method(),
        RestoreMethod::SelectiveFileRestore,
        "Appendix C.1 and §59.6: prefer selective restore over a full rollback"
    );
}

#[test]
fn should_say_which_method_the_chosen_one_beat() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "zfs",
            RestoreMethod::DatasetRollback,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("two methods reach the goal");
    assert_eq!(selection.rejected().len(), 1, "§20.1: the loser stays visible");
    assert_eq!(
        selection.rejected()[0].reason(),
        MethodRejection::Dominated,
        "Appendix C.1: something less destructive reaches the same goal"
    );
}

#[test]
fn should_fall_back_to_a_bigger_method_when_nothing_smaller_is_offered() {
    let candidates = MethodOffers::empty().with(offer(
        "zfs",
        RestoreMethod::DatasetRollback,
        MetadataCoverage::content_only(),
    ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("the only offer reaches the goal");
    assert_eq!(selection.chosen().method(), RestoreMethod::DatasetRollback);
}

#[test]
fn should_refuse_when_no_offered_method_can_meet_the_goal() {
    let candidates = MethodOffers::empty().with(offer(
        "compensating",
        RestoreMethod::Compensation,
        MetadataCoverage::content_only(),
    ));
    let error = select(&MethodRequest::new(
        RecoveryGoal::RestoreDomain,
        &candidates,
    ))
    .expect_err("§27.4: compensation is not a restore of a whole persistence domain");
    assert_eq!(
        error.code().name(),
        "recovery.plan_incomplete",
        "§56.3: block rather than reach for the biggest available hammer"
    );
}

#[test]
fn should_refuse_when_nothing_at_all_was_offered() {
    let candidates = MethodOffers::empty();
    let error = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect_err("§56.3: no method is not the smallest method");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_record_that_a_method_lost_because_it_cannot_reach_the_goal() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "compensating",
            RestoreMethod::Compensation,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("one offer reaches the goal");
    assert_eq!(
        selection.rejected()[0].reason(),
        MethodRejection::GoalUnsatisfied,
        "Appendix C.2: the goal decides, not the mechanism"
    );
}

#[test]
fn should_not_choose_a_content_only_restore_when_the_goal_needs_the_mode_and_owner_back() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "clone",
            RestoreMethod::CloneAndCopy,
            content_mode_owner(),
        ));
    let selection = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .requiring_metadata(content_mode_owner()),
    )
    .expect("Appendix C.1 permits dropping to a lower item");
    assert_eq!(
        selection.chosen().method(),
        RestoreMethod::CloneAndCopy,
        "Appendix C.1: a lower item MAY be chosen when upper items cannot preserve required \
         metadata"
    );
}

#[test]
fn should_name_the_metadata_the_skipped_method_could_not_restore() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "clone",
            RestoreMethod::CloneAndCopy,
            content_mode_owner(),
        ));
    let selection = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .requiring_metadata(content_mode_owner()),
    )
    .expect("a lower item can do it");
    let skipped = selection
        .rejected()
        .iter()
        .find(|rejection| rejection.method() == RestoreMethod::SelectiveFileRestore)
        .expect("the skipped method stays visible");
    assert_eq!(skipped.reason(), MethodRejection::MetadataShortfall);
    let unmet: Vec<&str> = skipped.unmet().iter().map(std::convert::AsRef::as_ref).collect();
    assert!(
        unmet.contains(&"mode") && unmet.contains(&"owner/group"),
        "Appendix C.7: missing metadata support MUST be visible, and it named {unmet:?}"
    );
}

#[test]
fn should_show_the_metadata_the_chosen_method_still_does_not_restore() {
    let candidates = MethodOffers::empty().with(offer(
        "clone",
        RestoreMethod::CloneAndCopy,
        content_mode_owner(),
    ));
    let selection = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .requiring_metadata(content_mode_owner()),
    )
    .expect("the offer meets the requirement");
    assert!(
        selection.metadata_gaps().contains(&"SELinux labels"),
        "Appendix C.7: missing metadata support reduces recovery coverage and MUST be visible"
    );
}

#[test]
fn should_leave_no_metadata_gap_when_the_chosen_method_restores_everything() {
    let candidates = MethodOffers::empty().with(offer(
        "zfs",
        RestoreMethod::SelectiveFileRestore,
        full_metadata(),
    ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("the offer meets the requirement");
    assert!(selection.metadata_gaps().is_empty());
}

#[test]
fn should_refuse_when_no_offered_method_restores_the_required_metadata() {
    let candidates = MethodOffers::empty().with(offer(
        "archive",
        RestoreMethod::SelectiveFileRestore,
        MetadataCoverage::content_only(),
    ));
    let error = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .requiring_metadata(full_metadata()),
    )
    .expect_err("§56.3: nothing can meet the goal");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_not_choose_a_method_that_cannot_preserve_a_required_application_semantic() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ))
        .with(
            offer(
                "postgres",
                RestoreMethod::CloneAndCopy,
                MetadataCoverage::content_only(),
            )
            .preserving("transaction consistency of the open database"),
        );
    let selection = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .requiring_semantic("transaction consistency of the open database"),
    )
    .expect("Appendix C.1 permits dropping to a lower item");
    assert_eq!(
        selection.chosen().method(),
        RestoreMethod::CloneAndCopy,
        "Appendix C.1: a lower item MAY be chosen when upper items cannot preserve required \
         application semantics"
    );
    assert_eq!(
        selection.rejected()[0].reason(),
        MethodRejection::SemanticShortfall
    );
}

#[test]
fn should_refuse_a_method_this_engine_does_not_know() {
    let candidates = MethodOffers::empty().with(offer(
        "archive",
        RestoreMethod::SelectiveFileRestore,
        MetadataCoverage::content_only(),
    ));
    let error = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates).forcing("merge"),
    )
    .expect_err("Appendix C.5: automatic semantic merging is an explicit non-goal");
    assert_eq!(error.code().name(), "change.action_not_plannable");
}

#[test]
fn should_explain_that_merge_belongs_to_a_provider_with_its_own_verification() {
    let candidates = MethodOffers::empty().with(offer(
        "archive",
        RestoreMethod::SelectiveFileRestore,
        MetadataCoverage::content_only(),
    ));
    let error = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .forcing("configuration-merge"),
    )
    .expect_err("Appendix C.5");
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("distinct RecoveryPlan action with its own verification")),
        "Appendix C.5: a KUANG/11 provider MAY offer merge support, presented as its own action"
    );
}

#[test]
fn should_offer_no_method_that_is_a_merge() {
    let names: Vec<&str> = RestoreMethod::ALL
        .iter()
        .map(|method| method.as_str())
        .collect();
    assert!(
        !names.iter().any(|name| name.contains("merge")),
        "Appendix C.5: no method the core recovery engine can choose is a merge, and it offers \
         {names:?}"
    );
}

#[test]
fn should_choose_the_method_the_operator_named_when_it_meets_the_goal() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "zfs",
            RestoreMethod::DatasetRollback,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ));
    let selection = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .forcing("dataset-rollback"),
    )
    .expect("the named method is on offer and reaches the goal");
    assert_eq!(selection.chosen().method(), RestoreMethod::DatasetRollback);
}

#[test]
fn should_refuse_a_named_method_nobody_offered() {
    let candidates = MethodOffers::empty().with(offer(
        "archive",
        RestoreMethod::SelectiveFileRestore,
        MetadataCoverage::content_only(),
    ));
    let error = select(
        &MethodRequest::new(RecoveryGoal::RestoreChangedObjects, &candidates)
            .forcing("offline-root-recovery"),
    )
    .expect_err("§56.3: a method no provider offers is not a plan");
    assert_eq!(error.code().name(), "recovery.plan_incomplete");
}

#[test]
fn should_ask_only_the_planning_half_of_a_provider_when_gathering_offers() {
    let provider = TestProvider::new("ono.recovery.zfs");
    let calls = provider.calls();
    let registry = registry(vec![provider.shared()]);
    let assets = vec![ready_asset(
        "ono.recovery.zfs",
        "rpool/ROOT/debian@ono-a82f",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EPOCH,
    )];
    let answer = offers(
        &registry,
        &assets,
        None,
        RecoveryGoal::RestoreChangedObjects,
    );
    assert_eq!(answer.offers().len(), 1);
    assert_eq!(calls.planning(), 1, "§12.1: plan_recovery is the planning half");
    assert_eq!(
        calls.mutating(),
        0,
        "§24.1: `recover` produces a plan and does not modify state"
    );
}

#[test]
fn should_record_a_refusal_when_the_provider_that_holds_an_asset_cannot_run_here() {
    let registry = registry(vec![
        TestProvider::new("ono.recovery.zfs")
            .unavailable("the zfs command is not installed")
            .shared(),
    ]);
    let assets = vec![ready_asset(
        "ono.recovery.zfs",
        "rpool/ROOT/debian@ono-a82f",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EPOCH,
    )];
    let answer = offers(
        &registry,
        &assets,
        None,
        RecoveryGoal::RestoreChangedObjects,
    );
    assert!(answer.offers().is_empty());
    assert!(
        !answer.is_conclusive(),
        "§55.6 case 29: an empty list and 'the provider could not be asked' are different answers"
    );
    assert_eq!(
        answer.refusals()[0].error().code().name(),
        "recovery.provider_unavailable"
    );
}

#[test]
fn should_record_a_refusal_when_no_provider_with_the_assets_id_is_registered() {
    let registry = registry(vec![TestProvider::new("ono.recovery.btrfs").shared()]);
    let assets = vec![ready_asset(
        "ono.recovery.zfs",
        "rpool/ROOT/debian@ono-a82f",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EPOCH,
    )];
    let answer = offers(
        &registry,
        &assets,
        None,
        RecoveryGoal::RestoreChangedObjects,
    );
    assert!(answer.offers().is_empty());
    assert_eq!(answer.refusals().len(), 1, "§11.1: an asset's provenance is its provider");
}

#[test]
fn should_record_a_refusal_when_the_providers_own_recovery_planning_fails() {
    let registry = registry(vec![
        TestProvider::new("ono.recovery.zfs")
            .failing(ono_change_core::error::consistency_unknown(
                "rpool/ROOT/debian",
                "the pool did not answer",
            ))
            .shared(),
    ]);
    let assets = vec![ready_asset(
        "ono.recovery.zfs",
        "rpool/ROOT/debian@ono-a82f",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EPOCH,
    )];
    let answer = offers(
        &registry,
        &assets,
        None,
        RecoveryGoal::RestoreChangedObjects,
    );
    assert!(!answer.is_conclusive());
    assert_eq!(
        answer.refusals()[0].error().code().name(),
        "recovery.consistency_unknown"
    );
}

#[test]
fn should_name_the_provider_that_could_not_be_asked_in_the_refusal_to_choose() {
    let registry = registry(vec![
        TestProvider::new("ono.recovery.zfs")
            .unavailable("the zfs command is not installed")
            .shared(),
    ]);
    let assets = vec![ready_asset(
        "ono.recovery.zfs",
        "rpool/ROOT/debian@ono-a82f",
        "rpool/ROOT/debian",
        &["/etc/nginx/nginx.conf"],
        EPOCH,
    )];
    let answer = offers(
        &registry,
        &assets,
        None,
        RecoveryGoal::RestoreChangedObjects,
    );
    let error = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &answer,
    ))
    .expect_err("§56.3");
    assert!(
        error.help().is_some_and(|help| help.contains("zfs")),
        "§45: the refusal says which provider could not be asked"
    );
}

#[test]
fn should_reach_a_provider_native_restore_before_a_selective_one() {
    let candidates = MethodOffers::empty()
        .with(offer(
            "archive",
            RestoreMethod::SelectiveFileRestore,
            MetadataCoverage::content_only(),
        ))
        .with(offer(
            "apt",
            RestoreMethod::ProviderNativeRestore,
            MetadataCoverage::content_only(),
        ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("both reach the goal");
    assert_eq!(
        selection.chosen().method(),
        RestoreMethod::ProviderNativeRestore,
        "Appendix C.1's first item is the owning provider restoring its own object"
    );
}

#[test]
fn should_carry_the_asset_the_chosen_offer_would_restore_from() {
    let candidates = MethodOffers::empty().with(offer(
        "archive",
        RestoreMethod::SelectiveFileRestore,
        MetadataCoverage::content_only(),
    ));
    let selection = select(&MethodRequest::new(
        RecoveryGoal::RestoreChangedObjects,
        &candidates,
    ))
    .expect("the offer reaches the goal");
    assert_eq!(selection.chosen().reference(), "archive:asset");
    assert_eq!(selection.chosen().provider(), "archive");
}
