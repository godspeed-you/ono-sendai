#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! Scope, exclusions and discovery (spec v0.6 §15.1, §15.2, Appendix A.4, B.7, G.2).

mod support;

use std::path::Path;

use ono_change_core::{
    NonPersistentReason, ProtectionMode, RecoveryObjective, RecoveryProvider, RestoreMethod,
};
use ono_recovery_files::{
    FileProtectionLimits, FileRecoveryProvider, FileRecoveryStore, ObjectKind, PROVIDER_ID,
    ScanMode, scan,
};
use ono_testkit::{SkipReason, skipped};
use support::Fixture;

fn domain_of(fixture: &Fixture, path: &Path) -> ono_change_core::PersistenceDomain {
    fixture
        .provider
        .resolve_domain(&path.display().to_string())
        .expect("the path resolves")
        .expect("the path is this provider's business")
}

fn candidates_for(fixture: &Fixture, path: &Path) -> Vec<ono_change_core::RecoveryCandidate> {
    let domain = domain_of(fixture, path);
    fixture
        .provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery answers")
}

#[test]
fn should_resolve_a_regular_file_to_a_persistent_domain() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let domain = domain_of(&fixture, &configuration);
    assert!(
        domain.is_protectable(),
        "§15.1: a regular file on a persistent filesystem is what this provider protects"
    );
    assert_eq!(domain.object_kind(), "file");
    assert_eq!(
        domain.object(),
        Some(configuration.display().to_string().as_str())
    );
}

#[test]
fn should_resolve_a_symlink_as_a_symlink_rather_than_as_its_target() {
    let fixture = Fixture::new();
    let target = fixture.write("etc/real.conf", "worker_processes 1;\n");
    let link = fixture.path("etc/link.conf");
    std::os::unix::fs::symlink(&target, &link).expect("the symlink can be made");

    let domain = domain_of(&fixture, &link);
    assert_eq!(
        domain.object_kind(),
        "symlink",
        "§15.1: symlinks are protected as symlinks, and §43.5 forbids resolving through one"
    );
    assert_eq!(domain.object(), Some(link.display().to_string().as_str()));
}

#[test]
fn should_resolve_a_dangling_symlink_rather_than_refusing_it() {
    let fixture = Fixture::new();
    let link = fixture.path("etc/dangling.conf");
    std::fs::create_dir_all(fixture.path("etc")).expect("the directory can be made");
    std::os::unix::fs::symlink("/nowhere/at/all", &link).expect("the symlink can be made");

    let domain = domain_of(&fixture, &link);
    assert!(
        domain.is_protectable(),
        "§15.1: a link whose target is missing is still a link, and its own bytes are protectable"
    );
}

#[test]
fn should_refuse_a_socket_rather_than_archive_it() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.path("run")).expect("the directory can be made");
    let socket = fixture.path("run/nginx.sock");
    let _listener =
        std::os::unix::net::UnixListener::bind(&socket).expect("the socket can be bound");

    let domain = domain_of(&fixture, &socket);
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::NoProvider),
        "§15.2: a socket is not something this provider silently archives"
    );
    assert!(domain.detail().contains("endpoint"));
}

#[test]
fn should_refuse_a_fifo_rather_than_archive_it() {
    let fixture = Fixture::new();
    std::fs::create_dir_all(fixture.path("run")).expect("the directory can be made");
    let fifo = fixture.path("run/pipe");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .expect("the fifo can be made");

    let domain = domain_of(&fixture, &fifo);
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::NoProvider),
        "§15.2: a FIFO carries a transfer rather than a state to restore"
    );
}

#[test]
fn should_refuse_a_device_node_rather_than_archive_it() {
    let fixture = Fixture::new();
    let domain = domain_of(&fixture, Path::new("/dev/null"));
    assert!(
        domain.refusal().is_some(),
        "§15.2: a device node names a driver, and copying it copies nothing behind it"
    );
    assert!(
        !domain.is_protectable(),
        "§15.2: devices are excluded from what this provider archives"
    );
}

#[test]
fn should_exclude_a_device_node_as_a_device_when_it_classifies_one() {
    // No unprivileged test can make a device node on a persistent filesystem, so the path-level
    // refusal above comes from /dev being a pseudo filesystem. The classification that excludes
    // a device anywhere else is exercised here, on a real device node.
    let Ok(metadata) = std::fs::symlink_metadata("/dev/null") else {
        skipped(
            SkipReason::MissingKernelFeature,
            "this host has no /dev/null to classify",
        );
        return;
    };
    let kind = ObjectKind::of(&metadata);
    assert_eq!(
        kind,
        ObjectKind::CharacterDevice,
        "§43.5: an lstat of /dev/null describes a character device"
    );
    assert!(
        !kind.is_protectable(),
        "§15.1: devices are excluded from what this provider archives"
    );
    assert!(
        kind.exclusion()
            .is_some_and(|reason| reason.contains("device node")),
        "§15.2: the exclusion says why a device is excluded, got {:?}",
        kind.exclusion()
    );
}

#[test]
fn should_refuse_a_path_on_a_pseudo_filesystem_rather_than_archive_it() {
    let fixture = Fixture::new();
    let domain = domain_of(&fixture, Path::new("/proc/self/status"));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::Pseudo),
        "Appendix B.7 and §15.2: procfs is a runtime interface, whatever its path looks like"
    );
}

#[test]
fn should_refuse_a_path_on_a_volatile_filesystem_rather_than_archive_it() {
    let fixture = Fixture::new();
    let domain = domain_of(&fixture, Path::new("/dev/shm"));
    assert_eq!(
        domain.refusal(),
        Some(NonPersistentReason::Volatile),
        "Appendix B.7: a tmpfs is a persistent-looking path whose contents do not survive a reboot"
    );
}

#[test]
fn should_refuse_a_path_that_is_not_there_rather_than_assuming_it_is_gone() {
    let fixture = Fixture::new();
    let error = fixture
        .provider
        .resolve_domain(&fixture.path("etc/absent.conf").display().to_string())
        .expect_err("a path that cannot be stated is not a resolution");
    assert_eq!(
        error.code().name(),
        "change.target_unresolved",
        "§56.3: an unresolved path is a refusal rather than an assumption"
    );
}

#[test]
fn should_refuse_to_archive_a_socket_that_sits_inside_a_protected_tree() {
    let fixture = Fixture::new();
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let socket = fixture.path("etc/nginx.sock");
    let _listener =
        std::os::unix::net::UnixListener::bind(&socket).expect("the socket can be bound");

    let domain = domain_of(&fixture, &fixture.path("etc"));
    let error = fixture
        .provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect_err("a tree holding a socket is refused rather than partly archived");
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
    assert!(
        error.help().is_some_and(|help| help.contains("socket")),
        "§15.2: the refusal names what it would have had to skip"
    );
}

#[test]
fn should_offer_one_candidate_for_a_file_on_a_persistent_filesystem() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let candidates = candidates_for(&fixture, &configuration);
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert_eq!(candidate.provider(), PROVIDER_ID);
    assert_eq!(
        candidate.scope_width(),
        1,
        "Appendix A.4: the scope is one object wide"
    );
}

#[test]
fn should_offer_a_selective_file_restore_so_a_small_backup_can_dominate_a_rollback() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let candidates = candidates_for(&fixture, &configuration);
    let method = candidates[0].restore_method();
    assert_eq!(method, RestoreMethod::SelectiveFileRestore);
    assert!(
        method.destructiveness() < RestoreMethod::DatasetRollback.destructiveness(),
        "Appendix A.4: a small configuration-file backup dominates a root-dataset rollback for \
         one file because its recovery blast radius is smaller"
    );
    assert!(
        !method.discards_newer_state(),
        "Appendix C.3: a selective restore does not discard everything written since"
    );
}

#[test]
fn should_count_every_object_in_a_tree_as_the_scope_width() {
    let fixture = Fixture::new();
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    fixture.write("etc/conf.d/gzip.conf", "gzip on;\n");
    let candidates = candidates_for(&fixture, &fixture.path("etc"));
    assert_eq!(
        candidates[0].scope_width(),
        4,
        "Appendix A.4: the scope is exactly the objects the archive would hold — two directories \
         and two files"
    );
}

#[test]
fn should_offer_no_candidate_when_the_objective_is_compensation() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let domain = domain_of(&fixture, &configuration);
    let candidates = fixture
        .provider
        .discover(&domain, RecoveryObjective::Compensate)
        .expect("discovery answers");
    assert!(
        candidates.is_empty(),
        "Appendix A.2: a copy of prior bytes is not an inverse action, and claiming it would be \
         is §27.4's forbidden 'rollback'"
    );
}

#[test]
fn should_offer_no_candidate_for_a_domain_it_already_refused() {
    let fixture = Fixture::new();
    let domain = domain_of(&fixture, Path::new("/proc/self/status"));
    let candidates = fixture
        .provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("discovery answers");
    assert!(
        candidates.is_empty(),
        "Appendix B.7: nothing on a pseudo-filesystem is offered as protection"
    );
}

#[test]
fn should_report_a_measured_size_rather_than_an_estimate_when_it_offers_a_candidate() {
    let fixture = Fixture::new();
    let body = "worker_processes 1;\n";
    let configuration = fixture.write("etc/nginx.conf", body);
    let candidates = candidates_for(&fixture, &configuration);
    let cost = candidates[0].cost();
    assert!(
        !cost.is_estimated(),
        "§37.5: cost numbers MUST be labelled estimated where accounting is not exact — this \
         provider copies the bytes, so its numbers are exact"
    );
    assert_eq!(
        cost.initial_bytes().map(ono_value::ByteSize::bytes),
        Some(body.len() as u128)
    );
}

#[test]
fn should_refuse_a_file_larger_than_the_configured_single_file_limit() {
    let fixture = Fixture::new();
    let configuration = fixture.write("var/big.db", &"x".repeat(4096));
    let limits = FileProtectionLimits::default().with_object_bytes(1024);
    let error = scan(&configuration, &limits, ScanMode::Measure, 0)
        .expect_err("a file over the limit is refused rather than truncated");
    assert_eq!(error.code().name(), "recovery.asset_create_failed");
    assert_eq!(
        error.metadata().get("limit_name"),
        Some(&ono_value::Value::string("max_object_bytes")),
        "§15.2: the refusal names the bound that was crossed"
    );
    assert_eq!(
        error.metadata().get("measured"),
        Some(&ono_value::Value::Int(4096)),
        "§15.2: and the measurement that crossed it"
    );
}

#[test]
fn should_refuse_a_tree_larger_than_the_configured_total_limit() {
    let fixture = Fixture::new();
    fixture.write("var/one", &"x".repeat(700));
    fixture.write("var/two", &"y".repeat(700));
    let limits = FileProtectionLimits::default().with_total_bytes(1000);
    let error = scan(&fixture.path("var"), &limits, ScanMode::Measure, 0)
        .expect_err("a tree over the limit is refused rather than partly archived");
    assert_eq!(
        error.metadata().get("limit_name"),
        Some(&ono_value::Value::string("max_total_bytes")),
        "§15.2: arbitrarily large data trees are not silently archived"
    );
}

#[test]
fn should_refuse_a_tree_holding_more_objects_than_the_configured_count() {
    let fixture = Fixture::new();
    for index in 0..5 {
        fixture.write(&format!("var/file-{index}"), "x");
    }
    let limits = FileProtectionLimits::default().with_object_count(3);
    let error = scan(&fixture.path("var"), &limits, ScanMode::Measure, 0)
        .expect_err("a tree with too many objects is refused");
    assert_eq!(
        error.metadata().get("limit_name"),
        Some(&ono_value::Value::string("max_object_count")),
        "§15.2: the refusal names the bound that was crossed"
    );
    assert!(
        error
            .help()
            .is_some_and(|help| help.contains("Nothing was archived")),
        "§62.1: a truncated archive is protection that is not there, so nothing is written"
    );
}

#[test]
fn should_archive_nothing_when_a_limit_refuses_the_tree() {
    let fixture = Fixture::new();
    fixture.write("var/one", &"x".repeat(700));
    // The protection was planned by a provider the tree fitted; the provider that creates the
    // asset is bound by a limit the tree crosses, over the same store, so the refusal is
    // `create`'s own.
    let domain = domain_of(&fixture, &fixture.path("var"));
    let candidates = fixture
        .provider
        .discover(&domain, RecoveryObjective::PreserveExact)
        .expect("the tree is discoverable");
    let actions = fixture
        .provider
        .plan_protection(&candidates, ProtectionMode::Prefer)
        .expect("the tree is plannable");
    let action = actions.first().expect("one candidate makes one action");
    let limited = FileRecoveryProvider::new(
        FileRecoveryStore::open(fixture.path("store")).expect("the same store opens again"),
        support::at(support::NOW),
    )
    .with_limits(FileProtectionLimits::default().with_total_bytes(10));
    let error = limited
        .create(action)
        .expect_err("§15.2: a tree over the byte limit is refused");
    assert!(
        error.metadata().get("limit_name").is_some(),
        "§15.2: the refusal names the bound that was crossed, got {error:?}"
    );
    let store_entries = std::fs::read_dir(fixture.provider.store().root())
        .expect("the store can be listed")
        .count();
    assert_eq!(
        store_entries, 0,
        "§15.2: the refusal happens before anything is written"
    );
}

#[test]
fn should_state_the_metadata_it_cannot_restore_on_every_candidate() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let candidates = candidates_for(&fixture, &configuration);
    let exclusion = candidates[0]
        .exclusions()
        .iter()
        .find(|exclusion| exclusion.subject() == "metadata coverage")
        .expect("Appendix C.7: missing metadata support MUST be visible");
    assert!(
        exclusion.reason().contains("hard-link relationships"),
        "Appendix C.7: an archive writes one file per name, so hard links are named as a gap"
    );
}

#[test]
fn should_state_a_hard_linked_file_as_an_exclusion_rather_than_pretend_to_restore_the_link() {
    let fixture = Fixture::new();
    let first = fixture.write("etc/first.conf", "shared\n");
    let second = fixture.path("etc/second.conf");
    std::fs::hard_link(&first, &second).expect("the hard link can be made");

    let candidates = candidates_for(&fixture, &fixture.path("etc"));
    let named: Vec<&str> = candidates[0]
        .exclusions()
        .iter()
        .map(ono_change_core::RecoveryExclusion::subject)
        .collect();
    assert!(
        named.iter().any(|subject| subject.contains("first.conf")),
        "Appendix C.7: the file with two names is named, because the restore gives it one"
    );
}

#[test]
fn should_never_read_through_a_symlink_when_it_walks_a_tree() {
    let fixture = Fixture::new();
    let outside = fixture.write("outside/secret.pem", "PRIVATE KEY MATERIAL");
    std::fs::create_dir_all(fixture.path("etc")).expect("the directory can be made");
    std::os::unix::fs::symlink(&outside, fixture.path("etc/key.pem"))
        .expect("the symlink can be made");

    let capture = scan(
        &fixture.path("etc"),
        &FileProtectionLimits::default(),
        ScanMode::Capture,
        0,
    )
    .expect("a tree holding a symlink is protectable");
    let stored: Vec<u8> = capture
        .contents()
        .iter()
        .flat_map(|(_, bytes)| bytes.clone())
        .collect();
    assert!(
        !String::from_utf8_lossy(&stored).contains("PRIVATE KEY MATERIAL"),
        "§15.1 and §43.5: a symlink is archived as a symlink, and its target is never followed"
    );
    let link = capture
        .manifest()
        .entries()
        .iter()
        .find(|entry| entry.kind() == ono_recovery_files::ObjectKind::Symlink)
        .expect("the link is in the archive");
    assert_eq!(link.link_target(), Some(outside.as_path()));
}
