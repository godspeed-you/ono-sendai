#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! §14.3: snapshots are not recursive across nested subvolumes.
//!
//! This is the file the crate exists for. The recorded `nested-live.txt` and
//! `nested-in-snapshot.txt` are the evidence, and everything asserted here is a consequence of
//! them.

mod support;

use std::path::Path;

use ono_change_core::{RecoveryObjective, RecoveryProvider};
use support::{HOME_ID, NESTED_ID, ROOT_ID, VAR_ID, fixture, provider};

/// The paths §14.3's worked example changes, in the recorded layout.
const ETC_TARGET: &str = "/mnt/root/etc/nginx/nginx.conf";
const VAR_TARGET: &str = "/mnt/root/var/lib/app/state.db";
const NESTED_TARGET: &str = "/mnt/root/var/lib-app/state.db";

#[test]
fn should_detect_the_nested_subvolume_boundary_when_listing_a_filesystem() {
    let provider = provider(vec![fixture("subvol-list")]);
    let boundaries = provider
        .boundaries(Path::new("/mnt/top"))
        .expect("the recorded listing");
    let nested = boundaries
        .iter()
        .find(|boundary| boundary.id() == NESTED_ID)
        .expect("§55.4 case 18: the nested subvolume boundary is detected");
    let parent = boundaries
        .iter()
        .find(|boundary| boundary.id() == VAR_ID)
        .expect("its parent is in the same listing");
    assert!(
        nested.is_nested_in(parent),
        "§14.3: `@var/lib-app` is inside `@var`, and that is a snapshot boundary"
    );
    assert!(
        !parent.is_nested_in(nested),
        "nesting is not symmetric, and a boundary is not nested inside itself"
    );
}

#[test]
fn should_require_a_separate_snapshot_of_each_changed_subvolume() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(
            Path::new("/mnt/root"),
            &[Path::new(ETC_TARGET), Path::new(VAR_TARGET)],
        )
        .expect("the boundaries are readable");
    let names: Vec<&str> = required
        .required()
        .iter()
        .map(|entry| entry.boundary().tree_path())
        .collect();
    assert_eq!(
        names,
        vec!["@", "@var"],
        "§14.3's worked example: changing /etc and /var/lib/app requires a snapshot of the root \
         subvolume and a snapshot of @var, because a snapshot of the root does not contain what \
         is mounted at /var"
    );
}

#[test]
fn should_not_snapshot_a_subvolume_the_plan_does_not_touch() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(
            Path::new("/mnt/root"),
            &[Path::new(ETC_TARGET), Path::new(VAR_TARGET)],
        )
        .expect("the boundaries are readable");
    assert!(
        !required.includes("@home"),
        "§14.3 and §62.7: protection follows the planned mutation scope. `@home` is not touched, \
         so it is not snapshotted — blanket-snapshotting every filesystem on the host is the \
         anti-pattern §62.7 names"
    );
    assert!(
        required
            .required()
            .iter()
            .all(|entry| entry.boundary().id() != HOME_ID),
        "not by id either"
    );
}

#[test]
fn should_require_a_separate_asset_for_a_target_inside_a_nested_subvolume() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(Path::new("/mnt/root"), &[Path::new(NESTED_TARGET)])
        .expect("the boundaries are readable");
    let ids: Vec<u64> = required
        .required()
        .iter()
        .map(|entry| entry.boundary().id())
        .collect();
    assert_eq!(
        ids,
        vec![NESTED_ID],
        "§55.4 case 19: a required nested subvolume receives a separate snapshot. A snapshot of \
         @var is not offered as covering /mnt/root/var/lib-app, because inside that snapshot the \
         path is an empty directory"
    );
}

#[test]
fn should_offer_no_parent_snapshot_as_cover_for_a_nested_target() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(
            Path::new("/mnt/root"),
            &[Path::new(VAR_TARGET), Path::new(NESTED_TARGET)],
        )
        .expect("the boundaries are readable");
    assert_eq!(
        required.required().len(),
        2,
        "§14.3: a change in @var and a change in @var/lib-app are two protection requirements, \
         however close together the two paths look"
    );
    let nested = required
        .required()
        .iter()
        .find(|entry| entry.boundary().id() == NESTED_ID)
        .expect("the nested subvolume is required in its own right");
    assert_eq!(
        nested.changed(),
        &[std::sync::Arc::from(NESTED_TARGET)],
        "and the change inside it belongs to it, not to its parent"
    );
    let parent = required
        .required()
        .iter()
        .find(|entry| entry.boundary().id() == VAR_ID)
        .expect("@var is required for its own change");
    assert!(
        !parent.changed().iter().any(|path| path.contains("lib-app")),
        "the parent's snapshot is never credited with the nested path"
    );
}

#[test]
fn should_name_every_nested_subvolume_a_parent_snapshot_leaves_out() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(Path::new("/mnt/root"), &[Path::new(VAR_TARGET)])
        .expect("the boundaries are readable");
    let var = required
        .required()
        .first()
        .expect("@var is the subvolume that changes");
    let excluded: Vec<&str> = var
        .nested()
        .iter()
        .map(|nested| nested.tree_path())
        .collect();
    assert_eq!(
        excluded,
        vec!["@var/lib-app"],
        "§55.4 case 18: the coverage a parent snapshot claims excludes every nested subvolume \
         beneath it, and each one is named"
    );
}

#[test]
fn should_exclude_a_separately_mounted_subvolume_from_the_root_snapshot() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let layout = provider
        .layout(Path::new("/mnt/root"))
        .expect("the boundaries are readable");
    let root = layout.by_id(ROOT_ID).expect("the root subvolume");
    let holes: Vec<&str> = layout
        .nested_within(root)
        .iter()
        .map(|nested| nested.tree_path())
        .collect();
    assert_eq!(
        holes,
        vec!["@home", "@var", "@var/lib-app"],
        "§59.3: with a separate /var, Ono must not claim the root snapshot covers it. `@var` is a \
         sibling in the filesystem tree and a hole in the snapshot all the same, because what the \
         snapshot holds at `var/` is the empty directory the mount covers up"
    );
}

#[test]
fn should_document_why_the_exclusion_exists_from_the_recorded_pair() {
    let live = fixture("nested-live");
    let in_snapshot = fixture("nested-in-snapshot");
    let entries = |output: &ono_change_core::ToolOutput| {
        output
            .stdout()
            .lines()
            .filter(|line| line.starts_with('d') || line.starts_with('-'))
            .filter(|line| !line.ends_with(" .") && !line.ends_with(" .."))
            .count()
    };
    assert_eq!(
        entries(&live),
        1,
        "the live /mnt/root/var/lib-app holds a `state` directory"
    );
    assert_eq!(
        entries(&in_snapshot),
        0,
        "and inside a read-only snapshot of its parent @var, the same path is empty. That is \
         §14.3 demonstrated by a real filesystem rather than asserted: a snapshot of a parent \
         subvolume does not contain the live contents of a nested subvolume, so every recovery \
         asset over @var must exclude @var/lib-app and the plan must snapshot it separately"
    );
    let provider = provider(vec![fixture("subvol-list-root")]);
    let layout = provider
        .layout(Path::new("/mnt/root"))
        .expect("the boundaries are readable");
    let var = layout.by_id(VAR_ID).expect("@var");
    assert_eq!(
        layout.nested_within(var).len(),
        1,
        "and the provider's own boundary arithmetic finds exactly the subvolume the pair of \
         recorded listings proves is empty inside the snapshot"
    );
}

#[test]
fn should_keep_a_plain_directory_inside_a_snapshot_where_a_nested_subvolume_is_emptied() {
    let plain = fixture("plaindir-in-snapshot");
    assert!(
        plain.stdout().contains(" f"),
        "an ordinary directory's contents are inside the snapshot — the recorded \
         plaindir-in-snapshot.txt still holds its file `f`. Only a subvolume boundary empties a \
         path, which is why §14.3 is about boundaries and not about directories"
    );
}

#[test]
fn should_resolve_the_containing_subvolume_of_every_path_in_the_recorded_layout() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let layout = provider
        .layout(Path::new("/mnt/root"))
        .expect("the boundaries are readable");
    for (path, expected) in [
        ("/mnt/root/etc/nginx/nginx.conf", ROOT_ID),
        ("/mnt/root/looks-like-a-subvol/x", ROOT_ID),
        ("/mnt/root/home/william/notes", HOME_ID),
        ("/mnt/root/var/log/syslog", VAR_ID),
        ("/mnt/root/var/plain-dir/f", VAR_ID),
        ("/mnt/root/var/lib-app/state/db", NESTED_ID),
    ] {
        assert_eq!(
            layout.containing(Path::new(path)).map(|b| b.id()),
            Some(expected),
            "§14.1 and Appendix B.9: {path} belongs to subvolume {expected}, resolved from \
             metadata and mounts rather than from what the path looks like"
        );
    }
}

#[test]
fn should_report_a_path_no_subvolume_holds_rather_than_dropping_it() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(
            Path::new("/mnt/root"),
            &[Path::new(ETC_TARGET), Path::new("/elsewhere/file")],
        )
        .expect("the boundaries are readable");
    assert_eq!(
        required.unresolved(),
        &[std::sync::Arc::from("/elsewhere/file")],
        "§56.3: a path that maps to no subvolume is reported, because a silently shorter list is \
         how a plan comes to be called protected while a target is not"
    );
    assert_eq!(required.required().len(), 1);
}

#[test]
fn should_group_several_changes_in_one_subvolume_into_one_requirement() {
    let provider = provider(vec![fixture("subvol-list-root")]);
    let required = provider
        .required_protection(
            Path::new("/mnt/root"),
            &[
                Path::new(ETC_TARGET),
                Path::new("/mnt/root/etc/hosts"),
                Path::new("/mnt/root/usr/local/bin/tool"),
            ],
        )
        .expect("the boundaries are readable");
    assert_eq!(
        required.required().len(),
        1,
        "one subvolume is one snapshot, however many objects inside it a plan changes"
    );
    assert_eq!(required.required()[0].changed().len(), 3);
}

#[test]
fn should_refuse_to_list_boundaries_when_the_search_is_not_permitted() {
    let provider = provider(vec![fixture("unprivileged-list")]);
    let error = provider
        .boundaries(Path::new("/mnt/root"))
        .expect_err("§56.3: a boundary list nobody could read is not an empty boundary list");
    assert!(
        error.message().contains("btrfs subvolume list"),
        "the refusal names the command that could not answer"
    );
}

#[test]
fn should_discover_a_candidate_that_names_the_nested_subvolumes_it_does_not_hold() {
    let provider = provider(vec![
        fixture("fs-show-mount"),
        fixture("subvol-show-var"),
        fixture("subvol-list-root"),
        fixture("subvol-list-root"),
        fixture("fs-usage"),
    ]);
    let domain = provider
        .resolve_domain("/mnt/root/var/log/syslog")
        .expect("the resolution runs")
        .expect("the path is on Btrfs");
    let candidates = provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery runs");
    let candidate = candidates.first().expect("one subvolume, one candidate");
    assert!(
        candidate
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.subject().contains("@var/lib-app")),
        "§55.4 case 18: the candidate's coverage excludes the nested subvolume by name, so a \
         reader of the plan sees the hole before the snapshot is taken"
    );
    assert!(
        candidate
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.reason().contains("empty directory")),
        "and the reason says what is actually there instead"
    );
}
