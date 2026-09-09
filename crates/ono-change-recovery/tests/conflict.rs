//! The newer-state conflict model (v0.6 §24.2, Appendix C.3, C.4, Appendix I.5, §59.6).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{DirectoryRestorePolicy, NewerStateClass, NewerStateImpact, RestoreMethod};
use ono_change_recovery::conflict::{ConflictRequest, ObjectObservation, ObservedState, analyse};
use ono_value::ByteSize;
use support::at;

/// Appendix C.4's own example: the plan wrote at 14:03, the user edited again at 15:12, and the
/// recovery target is the 14:02 snapshot.
fn appendix_c4() -> Vec<ObjectObservation> {
    vec![ObjectObservation::new(
        "/etc/nginx/nginx.conf",
        ObservedState::changed(at(15, 12), "sha256:user-edit"),
    )]
}

fn class_of(impact: &NewerStateImpact, object: &str) -> Option<NewerStateClass> {
    impact
        .items()
        .iter()
        .find(|item| item.object() == object)
        .map(ono_change_core::NewerStateItem::class)
}

fn detail_of(impact: &NewerStateImpact, object: &str) -> String {
    impact
        .items()
        .iter()
        .find(|item| item.object() == object)
        .map(|item| item.detail().to_owned())
        .unwrap_or_default()
}

#[test]
fn should_report_a_conflict_when_the_restored_object_was_edited_again_after_the_plan() {
    let observations = appendix_c4();
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf")
        .captured("/etc/nginx/nginx.conf", "sha256:before-the-plan"),
    );
    assert_eq!(
        class_of(&impact, "/etc/nginx/nginx.conf"),
        Some(NewerStateClass::Conflicting),
        "Appendix C.4: Ono MUST show a target-level conflict when the same file was edited again"
    );
}

#[test]
fn should_name_the_instant_of_the_edit_recovery_would_discard() {
    let observations = appendix_c4();
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf")
        .captured("/etc/nginx/nginx.conf", "sha256:before-the-plan"),
    );
    assert!(
        detail_of(&impact, "/etc/nginx/nginx.conf").contains("15:12"),
        "Appendix C.4: the plan says recovery would discard the 15:12 edit"
    );
}

#[test]
fn should_count_a_conflict_as_a_loss_that_recovery_would_take_away() {
    let observations = appendix_c4();
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf"),
    );
    assert_eq!(impact.losses().len(), 1, "§24.3: newer state discarded");
    assert_eq!(impact.losses()[0].object(), "/etc/nginx/nginx.conf");
}

#[test]
fn should_record_the_instant_of_a_conflicting_change_on_the_item() {
    let observations = appendix_c4();
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf"),
    );
    assert_eq!(
        impact.items()[0].changed_instant(),
        Some(at(15, 12)),
        "Appendix C.4 shows the instant beside the object"
    );
}

#[test]
fn should_not_report_a_loss_when_the_only_change_since_the_asset_is_the_plans_own() {
    let observations = vec![ObjectObservation::new(
        "/etc/nginx/nginx.conf",
        ObservedState::changed(at(14, 3), "sha256:written-by-the-plan"),
    )];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf"),
    );
    assert!(
        impact.losses().is_empty(),
        "Appendix C.4's conflict is the second edit: undoing the plan's own write is what \
         recovery is for"
    );
}

#[test]
fn should_treat_a_change_to_the_restored_object_as_a_conflict_when_the_plan_instant_is_unknown() {
    let observations = appendix_c4();
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .restoring("/etc/nginx/nginx.conf"),
    );
    assert_eq!(
        class_of(&impact, "/etc/nginx/nginx.conf"),
        Some(NewerStateClass::Conflicting),
        "§56.3: without the plan's own instant, a change cannot be attributed and the safe \
         direction is to show the conflict"
    );
}

#[test]
fn should_preserve_unrelated_newer_state_when_a_selective_restore_touches_one_file() {
    // §59.6 and §55.8 case 37.
    let observations = vec![
        ObjectObservation::new(
            "/etc/nginx/nginx.conf",
            ObservedState::changed(at(14, 3), "sha256:written-by-the-plan"),
        ),
        ObjectObservation::new(
            "/etc/hosts",
            ObservedState::changed(at(15, 0), "sha256:hosts"),
        ),
        ObjectObservation::new(
            "/etc/ssh/sshd_config",
            ObservedState::changed(at(15, 30), "sha256:sshd"),
        ),
    ];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf"),
    );
    assert_eq!(
        class_of(&impact, "/etc/hosts"),
        Some(NewerStateClass::PreservedByMethod),
        "§59.6: selective restore preserves unrelated newer /etc state"
    );
    assert_eq!(
        class_of(&impact, "/etc/ssh/sshd_config"),
        Some(NewerStateClass::PreservedByMethod),
        "§55.8 case 37: selective recovery preserves an unrelated newer file"
    );
}

#[test]
fn should_need_no_destructive_acceptance_when_a_selective_restore_loses_nothing() {
    let observations = vec![
        ObjectObservation::new(
            "/etc/nginx/nginx.conf",
            ObservedState::changed(at(14, 3), "sha256:written-by-the-plan"),
        ),
        ObjectObservation::new(
            "/etc/hosts",
            ObservedState::changed(at(15, 0), "sha256:hosts"),
        ),
    ];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .applied_at(at(14, 3))
        .restoring("/etc/nginx/nginx.conf"),
    );
    assert!(
        !impact.requires_destructive_acceptance(),
        "§40.1 applied to recovery: the ordinary selective restore stays a one-step operation"
    );
}

#[test]
fn should_discard_every_newer_object_in_the_domain_when_the_method_is_a_dataset_rollback() {
    let observations = vec![
        ObjectObservation::new(
            "/var/lib/app/db",
            ObservedState::changed(at(15, 0), "sha256:db"),
        ),
        ObjectObservation::new(
            "/var/log/app.log",
            ObservedState::changed(at(15, 5), "sha256:log"),
        ),
    ];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::DatasetRollback,
        at(14, 2),
        &observations,
    ));
    assert_eq!(
        class_of(&impact, "/var/lib/app/db"),
        Some(NewerStateClass::DiscardedByMethod)
    );
    assert_eq!(
        class_of(&impact, "/var/log/app.log"),
        Some(NewerStateClass::DiscardedByMethod),
        "§24.2: a full dataset rollback discards everything written since the snapshot"
    );
}

#[test]
fn should_discard_every_newer_object_in_the_domain_when_the_method_replaces_a_subvolume() {
    let observations = vec![ObjectObservation::new(
        "/home/ada/notes.md",
        ObservedState::changed(at(15, 0), "sha256:notes"),
    )];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::SubvolumeReplacement,
        at(14, 2),
        &observations,
    ));
    assert_eq!(
        class_of(&impact, "/home/ada/notes.md"),
        Some(NewerStateClass::DiscardedByMethod),
        "§14.4: replacing a subvolume takes everything written into it since with it"
    );
}

#[test]
fn should_gate_a_rollback_that_discards_newer_state() {
    let observations = vec![ObjectObservation::new(
        "/var/lib/app/db",
        ObservedState::changed(at(15, 0), "sha256:db"),
    )];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::DatasetRollback,
        at(14, 2),
        &observations,
    ));
    assert!(
        impact.requires_destructive_acceptance(),
        "§24.5: no recovery execution occurs without the explicit gate"
    );
}

#[test]
fn should_classify_an_unobservable_object_as_unknown() {
    let observations = vec![ObjectObservation::new(
        "/mnt/remote/config",
        ObservedState::unestablished("the mount did not answer"),
    )];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::SelectiveFileRestore,
        at(14, 2),
        &observations,
    ));
    assert_eq!(
        class_of(&impact, "/mnt/remote/config"),
        Some(NewerStateClass::Unknown),
        "§56.3: a recovery fact that cannot be established is not an assumption"
    );
}

#[test]
fn should_gate_a_recovery_whose_scope_holds_an_object_nobody_could_observe() {
    let observations = vec![ObjectObservation::new(
        "/mnt/remote/config",
        ObservedState::unestablished("the mount did not answer"),
    )];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::SelectiveFileRestore,
        at(14, 2),
        &observations,
    ));
    assert!(
        impact.requires_destructive_acceptance(),
        "§56.3: an unestablished recovery fact blocks rather than being waved through"
    );
}

#[test]
fn should_say_why_an_object_could_not_be_observed() {
    let observations = vec![ObjectObservation::new(
        "/mnt/remote/config",
        ObservedState::unestablished("the mount did not answer"),
    )];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::SelectiveFileRestore,
        at(14, 2),
        &observations,
    ));
    assert!(
        detail_of(&impact, "/mnt/remote/config").contains("the mount did not answer"),
        "§45: a refusal carries the reason it rests on"
    );
}

#[test]
fn should_classify_an_object_as_unknown_when_neither_a_digest_nor_a_change_time_is_known() {
    let observations = vec![ObjectObservation::new(
        "/etc/motd",
        ObservedState::present("sha256:motd"),
    )];
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::SelectiveFileRestore,
        at(14, 2),
        &observations,
    ));
    assert_eq!(
        class_of(&impact, "/etc/motd"),
        Some(NewerStateClass::Unknown),
        "§2.4: whether the object changed could not be established, and unknown is not promoted"
    );
}

#[test]
fn should_report_nothing_for_an_object_whose_content_still_matches_the_asset() {
    let observations = vec![ObjectObservation::new(
        "/etc/motd",
        ObservedState::changed(at(15, 0), "sha256:motd"),
    )];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .captured("/etc/motd", "sha256:motd"),
    );
    assert!(
        impact.items().is_empty(),
        "Appendix C.3 classifies changes since the asset, and identical content is not one"
    );
}

#[test]
fn should_report_a_removed_object_as_newer_state() {
    let observations = vec![ObjectObservation::new(
        "/etc/nginx/conf.d/extra.conf",
        ObservedState::removed(at(15, 0)),
    )];
    let impact = analyse(
        &ConflictRequest::new(RestoreMethod::DatasetRollback, at(14, 2), &observations)
            .captured("/etc/nginx/conf.d/extra.conf", "sha256:extra"),
    );
    assert_eq!(
        class_of(&impact, "/etc/nginx/conf.d/extra.conf"),
        Some(NewerStateClass::DiscardedByMethod),
        "a deletion after the asset is a change the recovery would undo"
    );
}

#[test]
fn should_keep_a_newer_extra_file_when_a_directory_restore_uses_the_default_policy() {
    let observations = vec![ObjectObservation::new(
        "/etc/nginx/conf.d/new-site.conf",
        ObservedState::changed(at(15, 0), "sha256:new-site"),
    )];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .restoring("/etc/nginx/conf.d"),
    );
    assert_eq!(
        class_of(&impact, "/etc/nginx/conf.d/new-site.conf"),
        Some(NewerStateClass::PreservedByMethod),
        "Appendix C.6: default selective directory restore MUST NOT delete newer extra files"
    );
}

#[test]
fn should_discard_a_newer_extra_file_when_the_objective_requires_an_exact_tree() {
    let observations = vec![ObjectObservation::new(
        "/etc/nginx/conf.d/new-site.conf",
        ObservedState::changed(at(15, 0), "sha256:new-site"),
    )];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .restoring("/etc/nginx/conf.d")
        .directory_policy(DirectoryRestorePolicy::ExactTree),
    );
    assert_eq!(
        class_of(&impact, "/etc/nginx/conf.d/new-site.conf"),
        Some(NewerStateClass::DiscardedByMethod),
        "Appendix C.6: exact-tree equivalence deletes files the asset never held"
    );
}

#[test]
fn should_not_treat_a_sibling_directory_as_being_inside_the_restored_one() {
    let observations = vec![ObjectObservation::new(
        "/etc/nginx/conf.d-backup/old.conf",
        ObservedState::changed(at(15, 0), "sha256:old"),
    )];
    let impact = analyse(
        &ConflictRequest::new(
            RestoreMethod::SelectiveFileRestore,
            at(14, 2),
            &observations,
        )
        .restoring("/etc/nginx/conf.d")
        .directory_policy(DirectoryRestorePolicy::ExactTree),
    );
    assert_eq!(
        class_of(&impact, "/etc/nginx/conf.d-backup/old.conf"),
        Some(NewerStateClass::PreservedByMethod),
        "§13.4's rule about prefixes: a shared prefix is not containment"
    );
}

#[test]
fn should_carry_the_provider_native_objects_a_rollback_would_destroy() {
    // §24.5's second worked example.
    let observations: Vec<ObjectObservation> = Vec::new();
    let impact = analyse(
        &ConflictRequest::new(RestoreMethod::DatasetRollback, at(14, 2), &observations)
            .destroying("tank/data@later-1")
            .destroying("tank/data@later-2")
            .discarding(ByteSize::from_bytes(18 * 1024 * 1024 * 1024)),
    );
    assert_eq!(impact.destroyed_assets().len(), 2, "§24.5: will destroy");
    assert_eq!(
        impact.discarded_bytes(),
        Some(ByteSize::from_bytes(18 * 1024 * 1024 * 1024)),
        "§24.5: 18 GiB changed blocks since the snapshot"
    );
    assert!(
        impact.requires_destructive_acceptance(),
        "§13.6: Ono never adds a destructive flag silently"
    );
}

#[test]
fn should_mark_the_analysis_complete_once_it_has_run() {
    let observations: Vec<ObjectObservation> = Vec::new();
    let impact = analyse(&ConflictRequest::new(
        RestoreMethod::SelectiveFileRestore,
        at(14, 2),
        &observations,
    ));
    assert!(
        impact.is_complete(),
        "§62.8: an analysis that ran is a different state from one that did not"
    );
}

/// Appendix I.5, which the specification calls a core acceptance scenario: a plan changed a config
/// at 10:00, a package changed many `/etc` files at 12:00, and at 13:00 the operator wants the
/// 10:00 state back for one config.
fn appendix_i5() -> Vec<ObjectObservation> {
    vec![
        ObjectObservation::new(
            "/etc/app/app.conf",
            ObservedState::changed(at(10, 0), "sha256:written-by-the-plan"),
        ),
        ObjectObservation::new(
            "/etc/ld.so.conf",
            ObservedState::changed(at(12, 0), "sha256:package"),
        ),
        ObjectObservation::new(
            "/etc/ssl/openssl.cnf",
            ObservedState::changed(at(12, 0), "sha256:package"),
        ),
        ObjectObservation::new(
            "/etc/systemd/system.conf",
            ObservedState::changed(at(12, 0), "sha256:package"),
        ),
    ]
}

fn i5_request(method: RestoreMethod, observations: &[ObjectObservation]) -> ConflictRequest<'_> {
    ConflictRequest::new(method, at(9, 59), observations)
        .applied_at(at(10, 0))
        .restoring("/etc/app/app.conf")
        .captured("/etc/app/app.conf", "sha256:before-the-plan")
}

#[test]
fn should_achieve_the_goal_with_a_selective_restore_when_a_later_package_changed_other_files() {
    let observations = appendix_i5();
    let impact = analyse(&i5_request(
        RestoreMethod::SelectiveFileRestore,
        &observations,
    ));
    assert!(
        impact.losses().is_empty(),
        "Appendix I.5: selective file restore satisfies the objective and takes nothing else"
    );
    assert_eq!(
        impact.preserved().len(),
        3,
        "the three files the 12:00 package update changed survive"
    );
}

#[test]
fn should_show_that_a_full_root_rollback_would_discard_the_later_package_changes() {
    let observations = appendix_i5();
    let impact = analyse(&i5_request(RestoreMethod::DatasetRollback, &observations));
    let discarded: Vec<&str> = impact.losses().iter().map(|item| item.object()).collect();
    assert!(
        discarded.contains(&"/etc/ld.so.conf")
            && discarded.contains(&"/etc/ssl/openssl.cnf")
            && discarded.contains(&"/etc/systemd/system.conf"),
        "Appendix I.5: Ono MUST show that full root rollback would discard the 12:00 package \
         changes, and it discarded {discarded:?}"
    );
}

#[test]
fn should_prefer_the_method_whose_impact_is_smaller_when_both_reach_the_goal() {
    let observations = appendix_i5();
    let selective = analyse(&i5_request(
        RestoreMethod::SelectiveFileRestore,
        &observations,
    ));
    let rollback = analyse(&i5_request(RestoreMethod::DatasetRollback, &observations));
    assert!(
        selective.losses().len() < rollback.losses().len(),
        "Appendix I.5 proves recovery is goal-oriented rather than snapshot-oriented"
    );
    assert!(
        !selective.requires_destructive_acceptance() && rollback.requires_destructive_acceptance(),
        "§24.5: only the destructive one needs the explicit gate"
    );
}

#[test]
fn should_analyse_the_same_observations_to_the_same_impact_twice() {
    let observations = appendix_i5();
    let first = analyse(&i5_request(RestoreMethod::DatasetRollback, &observations));
    let second = analyse(&i5_request(RestoreMethod::DatasetRollback, &observations));
    assert_eq!(
        first, second,
        "§4.4 seals a plan over its analysis, so two derivations must compare equal"
    );
}
