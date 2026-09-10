//! Discovery, dataset boundaries and path mapping (§13.1, §13.4, Appendix B.8, G.2).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_change_core::{
    ConsistencyClass, EffectDomain, PersistenceDomain, RecoveryObjective, RecoveryProvider,
    RestoreMethod, ToolOutput,
};
use ono_recovery_zfs::{MountTable, ZFS, ZfsProvider};

use support::{CHILD_DATASET, PARENT_DATASET, ROOT_DATASET, Script, code, out, provider, runner};

const CHILD_FILE: &str = "/tank/data/customer/db.sqlite";
const PARENT_FILE: &str = "/tank/data/notes.txt";

fn resolved(path: &str) -> PersistenceDomain {
    let tools = runner(vec![(ZFS, out("list-filesystems"))]);
    provider(&tools)
        .resolve_domain(path)
        .expect("the recorded layout resolves")
        .unwrap_or_else(|| panic!("{path} lies on a ZFS dataset in the recorded layout"))
}

#[test]
fn should_resolve_a_path_to_the_dataset_the_mount_table_names() {
    let domain = resolved(CHILD_FILE);
    assert_eq!(
        domain.object(),
        Some(CHILD_DATASET),
        "Appendix B.8: dataset identity comes from mount metadata"
    );
    assert_eq!(domain.object_kind(), "zfs-dataset");
    assert!(domain.is_protectable());
}

#[test]
fn should_make_the_dataset_the_snapshot_boundary_section_thirteen_four_says_it_is() {
    assert_eq!(
        resolved(CHILD_FILE).boundary(),
        Some(CHILD_DATASET),
        "§13.4: the dataset is the boundary, and the child is not part of the parent's"
    );
}

#[test]
fn should_resolve_a_file_in_the_parent_dataset_to_the_parent_rather_than_to_the_child() {
    assert_eq!(resolved(PARENT_FILE).object(), Some(PARENT_DATASET));
}

#[test]
fn should_not_infer_a_dataset_from_a_directory_that_is_merely_named_like_one() {
    let domain = resolved("/tank/home/tank/data/customer/db.sqlite");
    assert_eq!(
        domain.object(),
        Some("tank/home"),
        "Appendix B.8: a directory named `tank/data/customer` inside `tank/home` is not a dataset"
    );
}

#[test]
fn should_refuse_to_claim_a_path_on_a_filesystem_that_is_not_zfs() {
    let tools = runner(Vec::new());
    let resolved = provider(&tools)
        .resolve_domain("/pool/tank/data/customer")
        .expect("a non-ZFS mount is answered, not refused");
    assert!(
        resolved.is_none(),
        "Appendix B.8 and G.2: `/pool` is an ext4 mount in the recorded table, whatever its name \
         suggests"
    );
}

#[test]
fn should_refuse_to_resolve_anything_when_the_mount_table_is_empty() {
    let tools = runner(Vec::new());
    let error = ZfsProvider::new(tools)
        .reading_mounts(MountTable::default())
        .resolve_domain("/tank/data")
        .expect_err("§56.3: without a mount table nothing is assumed in its place");
    assert_eq!(code(&error), "change.target_unresolved");
}

#[test]
fn should_refuse_the_resolution_when_zfs_denies_the_listing_for_want_of_privilege() {
    let tools = runner(vec![(ZFS, out("unprivileged-list"))]);
    let error = provider(&tools)
        .resolve_domain(CHILD_FILE)
        .expect_err("§43.4: a refusal for want of privilege is not an absent dataset");
    assert_eq!(code(&error), "change.privilege_required");
}

#[test]
fn should_refuse_when_the_mount_source_names_a_dataset_zfs_does_not_report() {
    let tools = runner(vec![(ZFS, ToolOutput::ok(""))]);
    let error = provider(&tools)
        .resolve_domain(CHILD_FILE)
        .expect_err("Appendix B.8: two readings that disagree do not become a protection claim");
    assert_eq!(code(&error), "change.target_unresolved");
}

/// Whether a candidate protects more than one dataset at one point (§13.3).
fn spans_datasets(candidate: &ono_change_core::RecoveryCandidate) -> bool {
    candidate
        .scope()
        .covers()
        .iter()
        .filter(|covered| !covered.starts_with('/'))
        .count()
        > 1
}

fn discovered(path: &str) -> Vec<ono_change_core::RecoveryCandidate> {
    let tools = Script::survey().runner();
    provider(&tools)
        .discover(&bare(path), RecoveryObjective::PreserveExact)
        .expect("the recorded pool is discoverable")
}

/// A domain that states only the path, so the provider does the resolution §13.1 asks it to.
fn bare(path: &str) -> PersistenceDomain {
    PersistenceDomain::refused(
        path,
        ono_change_core::ResolvedMount::new("0:0", "/", "zfs", "-", "/"),
        ono_change_core::NonPersistentReason::Unresolved,
        "the caller supplies the path only",
    )
}

#[test]
fn should_offer_a_candidate_over_the_dataset_that_actually_holds_the_target() {
    let candidates = discovered(CHILD_FILE);
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.scope().domain() == CHILD_DATASET),
        "§13.1: discovery resolves the dataset the target lives in"
    );
}

#[test]
fn should_not_offer_the_parent_dataset_as_covering_a_file_in_a_child_dataset() {
    // Appendix G.2 truth test: zfs-child-dataset.
    let parent = discovered(PARENT_FILE);
    for candidate in &parent {
        assert!(
            !candidate.scope().covers_object(CHILD_FILE),
            "§13.4: a snapshot of {PARENT_DATASET} does not protect {CHILD_FILE}"
        );
        assert!(
            !candidate.scope().covers_object(CHILD_DATASET) || spans_datasets(candidate),
            "§13.4: only an explicitly recursive candidate names the child dataset"
        );
    }
}

#[test]
fn should_name_the_child_dataset_as_an_exclusion_of_the_parent_candidate() {
    let parent = discovered(PARENT_FILE);
    let exact = parent
        .iter()
        .find(|candidate| !spans_datasets(candidate))
        .expect("the parent dataset has a non-recursive candidate");
    assert!(
        exact
            .exclusions()
            .iter()
            .any(|exclusion| exclusion.subject() == CHILD_DATASET),
        "§13.4: the plan says which datasets the snapshot does not protect"
    );
}

#[test]
fn should_state_that_a_dataset_snapshot_holds_no_process_or_network_state() {
    let candidates = discovered(CHILD_FILE);
    let exact = candidates.first().expect("a candidate was offered");
    for subject in ["process state", "network sessions"] {
        assert!(
            exact
                .exclusions()
                .iter()
                .any(|exclusion| exclusion.subject() == subject),
            "§11.5 and §33.1: a filesystem snapshot does not hold {subject}"
        );
    }
}

#[test]
fn should_offer_a_recursive_candidate_when_a_child_dataset_lies_inside_the_target_tree() {
    let candidates = discovered("/tank/data");
    let recursive = candidates
        .iter()
        .find(|candidate| spans_datasets(candidate))
        .expect("§13.3: several descendant datasets at one logical point may be taken recursively");
    assert!(recursive.scope().covers_object(CHILD_DATASET));
    assert!(recursive.scope().covers_object(PARENT_DATASET));
}

#[test]
fn should_not_offer_a_recursive_candidate_for_a_dataset_with_no_child_in_the_tree() {
    let candidates = discovered(CHILD_FILE);
    assert!(
        candidates
            .iter()
            .all(|candidate| !spans_datasets(candidate)),
        "§13.3: recursion is used where descendants must be protected, not by habit"
    );
}

#[test]
fn should_state_the_consistency_a_zfs_snapshot_actually_achieves() {
    let candidates = discovered(CHILD_FILE);
    let exact = candidates.first().expect("a candidate was offered");
    assert_eq!(
        exact.consistency(),
        ConsistencyClass::FilesystemConsistent,
        "§11.3 and §39.1: a dataset point-in-time is filesystem-consistent and claims no more"
    );
    assert_eq!(exact.domain(), EffectDomain::FilesystemPersistent);
}

#[test]
fn should_prefer_the_least_destructive_restore_method_when_offering_protection() {
    let candidates = discovered(CHILD_FILE);
    assert_eq!(
        candidates
            .first()
            .expect("a candidate was offered")
            .restore_method(),
        RestoreMethod::SelectiveFileRestore,
        "§13.5: for a plan changing a small number of files, prefer the method that minimises \
         unrelated rollback damage"
    );
}

#[test]
fn should_report_no_candidate_for_a_path_outside_zfs() {
    let tools = Script::survey().runner();
    let candidates = provider(&tools)
        .discover(&bare("/pool/tank/data"), RecoveryObjective::PreserveExact)
        .expect("a non-ZFS path is answered");
    assert!(
        candidates.is_empty(),
        "§55.6 case 29: nothing here is a different answer from an unavailable provider"
    );
}

#[test]
fn should_refuse_when_the_callers_resolution_and_the_mount_table_disagree() {
    let tools = Script::survey().runner();
    let claimed = PersistenceDomain::resolved(
        CHILD_FILE,
        ono_change_core::ResolvedMount::new("0:96", "/tank/data", "zfs", "tank/data", "/"),
        "zfs-dataset",
        PARENT_DATASET,
        "a caller that resolved the path to the parent dataset",
    );
    let error = provider(&tools)
        .discover(&claimed, RecoveryObjective::PreserveExact)
        .expect_err("Appendix B.8: a disagreement is not resolved by picking one");
    assert_eq!(code(&error), "change.target_unresolved");
}

#[test]
fn should_reproduce_the_not_protected_by_block_section_thirteen_four_prints() {
    let tools = Script::survey().runner();
    let report = provider(&tools)
        .boundary_report(ROOT_FILE, "ono-a82f-20260909T194500Z")
        .expect("the recorded pool is readable")
        .expect("the path lies on a ZFS dataset");
    let rendered = report.render();
    assert!(rendered.contains("TARGET\n  /altroot/debian/etc/nginx/nginx.conf"));
    assert!(rendered.contains("PERSISTENCE\n  dataset rpool/ROOT/debian"));
    assert!(
        rendered.contains("RECOVERY\n  snapshot rpool/ROOT/debian@ono-a82f-20260909T194500Z"),
        "§13.4's block names the snapshot that does protect the target"
    );
}

const ROOT_FILE: &str = "/altroot/debian/etc/nginx/nginx.conf";

#[test]
fn should_name_a_snapshot_of_another_dataset_under_not_protected_by() {
    let tools = Script::survey().runner();
    let report = provider(&tools)
        .boundary_report("/tank/data/customer/db.sqlite", "ono-b91c-20260909T194501Z")
        .expect("the recorded pool is readable")
        .expect("the path lies on a ZFS dataset");
    assert_eq!(report.dataset(), CHILD_DATASET);
    assert!(
        report
            .not_protected_by()
            .iter()
            .any(|other| other.as_ref() == "tank/data@ono-b91c-20260909T194501Z"),
        "§13.4: the parent's snapshot of the same name does not protect this file, and the block \
         says so"
    );
    assert!(report.render().contains("NOT PROTECTED BY"));
}

#[test]
fn should_name_the_enclosing_datasets_whose_snapshot_does_not_protect_a_child_dataset() {
    // §55.3 case 11: `tank/data` and `tank` enclose the child by name and by mount path, and a
    // snapshot of either stops at the child's boundary.
    let candidates = discovered(CHILD_FILE);
    for candidate in &candidates {
        let named: Vec<&str> = candidate
            .not_protecting()
            .iter()
            .map(AsRef::as_ref)
            .collect();
        assert!(
            named.contains(&PARENT_DATASET) && named.contains(&"tank"),
            "§13.4: the plan says which enclosing snapshots do not protect {CHILD_FILE}: {named:?}"
        );
        assert!(
            !named.contains(&CHILD_DATASET),
            "§13.4: the dataset that holds the target is the one whose snapshot does protect it"
        );
    }
}

#[test]
fn should_name_no_dataset_of_another_pool_as_enclosing_the_target() {
    let candidates = discovered(ROOT_FILE);
    let exact = candidates.first().expect("a candidate was offered");
    let named: Vec<&str> = exact.not_protecting().iter().map(AsRef::as_ref).collect();
    assert_eq!(
        named,
        ["rpool", "rpool/ROOT"],
        "§13.4: only the datasets that enclose `{ROOT_DATASET}` are named, in the order ZFS lists them"
    );
}

#[test]
fn should_read_the_pool_health_discovery_needs_for_the_proposed_operation() {
    let tools = Script::survey().runner();
    let layout = provider(&tools)
        .survey()
        .expect("the recorded pool is readable");
    let pool = layout.pool("tank").expect("`zpool list` reported tank");
    assert!(
        pool.is_healthy(),
        "§13.1: discovery establishes pool health sufficient for the proposed operation"
    );
    assert_eq!(pool.health.as_ref(), "ONLINE");
}

#[test]
fn should_read_the_clones_and_bookmarks_that_affect_destructive_rollback() {
    let tools = Script::survey().runner();
    let layout = provider(&tools)
        .survey()
        .expect("the recorded pool is readable");
    assert!(
        layout
            .clones_of("tank/data@ono-b91c-20260909T194501Z")
            .iter()
            .any(|clone| clone.name.as_ref() == "tank/cloned"),
        "§13.1: the clones that affect destructive rollback are part of discovery"
    );
    assert_eq!(
        layout.bookmarks_of(ROOT_DATASET).len(),
        1,
        "§13.1: so are the bookmarks"
    );
}

#[test]
fn should_read_a_snapshot_guid_rather_than_trusting_its_name() {
    let tools = Script::survey().runner();
    let layout = provider(&tools)
        .survey()
        .expect("the recorded pool is readable");
    let snapshot = layout
        .snapshot(support::ROOT_SNAPSHOT)
        .expect("the recorded pool holds it");
    assert_eq!(
        snapshot.guid.as_ref(),
        support::ROOT_SNAPSHOT_GUID,
        "§56.1: exact snapshot identity is the GUID"
    );
}

#[test]
fn should_treat_a_later_snapshot_of_the_same_second_as_newer_by_creation_order() {
    let tools = Script::survey().runner();
    let layout = provider(&tools)
        .survey()
        .expect("the recorded pool is readable");
    assert!(
        !layout.is_latest_snapshot(support::ROOT_SNAPSHOT),
        "Appendix D.5: ZFS prints creation to the second, so `-s creation` order is the evidence"
    );
    assert!(layout.is_latest_snapshot(support::NEWER_SNAPSHOT));
}

#[test]
fn should_say_so_when_the_pool_health_could_not_be_read_rather_than_dropping_the_note() {
    let tools = Script::survey()
        .answering(
            support::Slot::PoolList,
            ToolOutput::failed(1, "cannot open 'tank': no such pool\n"),
        )
        .runner();
    let candidates = provider(&tools)
        .discover(&bare(CHILD_FILE), RecoveryObjective::PreserveExact)
        .expect("the datasets are still discoverable");
    let exact = candidates.first().expect("a candidate was offered");
    assert!(
        exact
            .creation_requirements()
            .iter()
            .any(|requirement| requirement.contains("could not be read")),
        "§13.1 and §56.3: unknown pool health is stated, got {:?}",
        exact.creation_requirements()
    );
}

#[test]
fn should_state_that_a_degraded_pool_blocks_protection_when_discovering() {
    let listed = out("zpool-list")
        .stdout()
        .replace("1006297600\t0\t3\tONLINE", "1006297600\t0\t3\tDEGRADED");
    let status = out("zpool-status").stdout().replacen(
        "pool: tank\n state: ONLINE",
        "pool: tank\n state: DEGRADED",
        1,
    );
    let tools = Script::survey()
        .answering(support::Slot::PoolList, ToolOutput::ok(listed))
        .answering(support::Slot::PoolStatus, ToolOutput::ok(status))
        .runner();
    let candidates = provider(&tools)
        .discover(&bare(CHILD_FILE), RecoveryObjective::PreserveExact)
        .expect("the datasets are still discoverable");
    let exact = candidates.first().expect("a candidate was offered");
    assert!(
        exact
            .creation_requirements()
            .iter()
            .any(|requirement| requirement.contains("DEGRADED")
                && requirement.contains("not healthy")),
        "§13.1: the reason protection is blocked is stated, got {:?}",
        exact.creation_requirements()
    );
}
