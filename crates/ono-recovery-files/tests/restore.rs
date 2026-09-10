#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! Writing state back: atomic replacement, directory semantics and TOCTOU (§15.4, C.6, §43.5).

mod support;

use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::Path;

use ono_change_core::{DirectoryRestorePolicy, RecoveryAssetType, RecoveryProvider, RecoveryScope};
use ono_recovery_files::{FileRecoveryProvider, FileRecoveryStore};
use support::{Fixture, recovery_action, supports_xattrs};

fn inode_of(path: &Path) -> u64 {
    std::fs::symlink_metadata(path)
        .expect("the path exists")
        .ino()
}

fn exact_tree_provider(fixture: &Fixture) -> FileRecoveryProvider {
    let store = FileRecoveryStore::open(fixture.path("exact-store")).expect("a store opens");
    FileRecoveryProvider::new(store, fixture.provider.now())
        .with_directory_policy(DirectoryRestorePolicy::ExactTree)
}

fn protect_with(provider: &FileRecoveryProvider, path: &Path) -> ono_change_core::RecoveryAsset {
    let domain = provider
        .resolve_domain(&path.display().to_string())
        .expect("the path resolves")
        .expect("the path is protectable");
    let candidates = provider
        .discover(&domain, ono_change_core::RecoveryObjective::PreserveExact)
        .expect("discovery answers");
    let actions = provider
        .plan_protection(&candidates, ono_change_core::ProtectionMode::Prefer)
        .expect("planning answers");
    provider.create(&actions[0]).expect("the copy is made")
}

#[test]
fn should_put_the_captured_bytes_back_when_it_restores() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture.write("etc/nginx.conf", "worker_processes 8; # broken\n");

    fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect("the object comes back");
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("the file is readable"),
        "worker_processes 1;\n",
        "§15: the provider puts back the bytes it copied"
    );
}

#[test]
fn should_replace_the_live_file_rather_than_truncate_it_in_place() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture.write("etc/nginx.conf", "worker_processes 8;\n");
    let before = inode_of(&configuration);

    fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect("the object comes back");
    assert_ne!(
        inode_of(&configuration),
        before,
        "§15.4: restoration uses temp-file + fsync + atomic rename rather than truncating the \
         live file in place, so the object that is there afterwards is a different one"
    );
}

#[test]
fn should_leave_the_original_intact_when_the_replacement_cannot_be_written() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture.write("etc/nginx.conf", "worker_processes 8;\n");
    let before = inode_of(&configuration);
    // The stored copy is sound and the directory is writable; what cannot be written is the
    // replacement, because a directory already occupies the place it is staged in. Were the
    // staging place ever renamed, the restore would succeed and this test would fail loudly.
    std::fs::create_dir(fixture.path("etc/.ono-recovery.nginx.conf.staging"))
        .expect("the staging place can be occupied");

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err("a replacement that cannot be written is not renamed over anything");
    assert_eq!(error.code().name(), "recovery.apply_failed");
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("the original is still there"),
        "worker_processes 8;\n",
        "§15.4: nothing is renamed until the replacement is complete, so the live file is untouched"
    );
    assert_eq!(inode_of(&configuration), before);
}

#[test]
fn should_leave_the_original_intact_when_the_stored_copy_cannot_be_read() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    fixture.write("etc/nginx.conf", "worker_processes 8;\n");
    let before = inode_of(&configuration);
    std::fs::remove_file(Path::new(asset.reference()).join("objects/00000001"))
        .expect("the stored copy can be removed");

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err("a restore that cannot read its copy does not write anything");
    assert_eq!(error.code().name(), "recovery.asset_not_found");
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("the file is readable"),
        "worker_processes 8;\n",
        "§15.4: nothing is renamed until the replacement is complete, so the live file is untouched"
    );
    assert_eq!(inode_of(&configuration), before);
}

#[test]
fn should_leave_no_staging_file_behind_when_a_restore_fails() {
    // The refusal here is the digest check, which runs before anything is staged, so this guards
    // the order: verify the copy, then stage it. A failure after the staging file exists would be
    // `write_all` or `sync_all` returning an I/O error, which no unprivileged test can provoke.
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::write(
        Path::new(asset.reference()).join("objects/00000001"),
        "tampered\n",
    )
    .expect("the stored copy can be changed");

    fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err("a copy that does not match its digest is not written back");
    let leftovers: Vec<_> = std::fs::read_dir(fixture.path("etc"))
        .expect("the directory can be listed")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".ono-recovery"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "Appendix F.1: a failed prepare leaves no partial artefact behind"
    );
}

#[test]
fn should_refuse_a_stored_copy_that_no_longer_matches_its_digest() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::write(
        Path::new(asset.reference()).join("objects/00000001"),
        "tampered\n",
    )
    .expect("the stored copy can be changed");

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err("§11.4: the copy must be the state that was protected");
    assert_eq!(error.code().name(), "recovery.apply_failed");
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("the file is readable"),
        "worker_processes 1;\n",
        "the live file keeps whatever it had, because nothing was written"
    );
}

#[test]
fn should_restore_the_mode_it_captured() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o640))
        .expect("the mode can be set");
    let asset = fixture.protect(&configuration);
    std::fs::set_permissions(&configuration, std::fs::Permissions::from_mode(0o666))
        .expect("the mode can be widened");

    fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect("the object comes back");
    assert_eq!(
        std::fs::symlink_metadata(&configuration)
            .expect("the file exists")
            .permissions()
            .mode()
            & 0o7777,
        0o640,
        "Appendix C.7: this provider restores mode, and says so in its coverage"
    );
}

#[test]
fn should_restore_a_symlink_as_a_symlink() {
    let fixture = Fixture::new();
    let target = fixture.write("etc/real.conf", "worker_processes 1;\n");
    let link = fixture.path("etc/link.conf");
    std::os::unix::fs::symlink(&target, &link).expect("the symlink can be made");
    let asset = fixture.protect(&link);
    std::fs::remove_file(&link).expect("the link can be removed");
    std::fs::write(&link, "a regular file now").expect("something else takes its place");

    fixture
        .provider
        .restore(&recovery_action(Some(&link)), &asset)
        .expect_err("§43.5: a link was protected and a regular file is there now");
    std::fs::remove_file(&link).expect("the impostor can be removed");
    fixture
        .provider
        .restore(&recovery_action(Some(&link)), &asset)
        .expect("with the path clear, the link comes back");
    assert_eq!(
        std::fs::read_link(&link).expect("it is a link"),
        target,
        "§15.1: a symlink is restored as a symlink pointing where it pointed"
    );
}

#[test]
fn should_recreate_an_object_that_was_deleted_after_it_was_protected() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::remove_file(&configuration).expect("the file can be removed");

    fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect("a deleted object comes back");
    assert_eq!(
        std::fs::read_to_string(&configuration).expect("the file is readable"),
        "worker_processes 1;\n",
        "§32.1: a deleted file is exactly what a recovery asset is for"
    );
}

#[test]
fn should_keep_a_newer_file_the_archive_never_held_when_a_directory_is_restored() {
    let fixture = Fixture::new();
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&fixture.path("etc"));
    let newer = fixture.write("etc/local.conf", "added afterwards\n");

    let report = fixture
        .provider
        .restore_reporting(&recovery_action(None), &asset)
        .expect("the tree comes back");
    assert!(
        newer.exists(),
        "Appendix C.6: default selective directory restore MUST NOT delete newer extra files"
    );
    assert!(report.kept().contains(&newer));
    assert!(report.removed().is_empty());
}

#[test]
fn should_remove_a_newer_file_when_the_policy_requires_an_exact_tree() {
    let fixture = Fixture::new();
    let provider = exact_tree_provider(&fixture);
    fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = protect_with(&provider, &fixture.path("etc"));
    let newer = fixture.write("etc/local.conf", "added afterwards\n");

    let report = provider
        .restore_reporting(&recovery_action(None), &asset)
        .expect("the tree comes back");
    assert!(
        !newer.exists(),
        "Appendix C.6: exact-tree equivalence is the case where a newer file is deleted"
    );
    assert!(report.removed().contains(&newer));
}

#[test]
fn should_restore_only_the_object_the_action_names() {
    let fixture = Fixture::new();
    let first = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let second = fixture.write("etc/mime.types", "text/plain txt;\n");
    let asset = fixture.protect(&fixture.path("etc"));
    fixture.write("etc/nginx.conf", "worker_processes 8;\n");
    fixture.write("etc/mime.types", "text/csv csv;\n");

    fixture
        .provider
        .restore(&recovery_action(Some(&first)), &asset)
        .expect("the named object comes back");
    assert_eq!(
        std::fs::read_to_string(&first).expect("readable"),
        "worker_processes 1;\n"
    );
    assert_eq!(
        std::fs::read_to_string(&second).expect("readable"),
        "text/csv csv;\n",
        "§13.5: a selective restore reads back the wanted objects and leaves everything else alone"
    );
}

#[test]
fn should_restore_a_whole_tree_when_the_action_names_no_single_object() {
    let fixture = Fixture::new();
    let first = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let second = fixture.write("etc/conf.d/gzip.conf", "gzip on;\n");
    let asset = fixture.protect(&fixture.path("etc"));
    fixture.write("etc/nginx.conf", "worker_processes 8;\n");
    std::fs::remove_file(&second).expect("the nested file can be removed");

    fixture
        .provider
        .restore(&recovery_action(None), &asset)
        .expect("the tree comes back");
    assert_eq!(
        std::fs::read_to_string(&first).expect("readable"),
        "worker_processes 1;\n"
    );
    assert_eq!(
        std::fs::read_to_string(&second).expect("readable"),
        "gzip on;\n",
        "§15.1: a small directory tree is protected and restored as a tree"
    );
}

#[test]
fn should_refuse_an_object_the_archive_does_not_hold() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let elsewhere = fixture.write("etc/mime.types", "text/plain txt;\n");

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&elsewhere)), &asset)
        .expect_err("§11.2: an asset covers what it covers");
    assert_eq!(error.code().name(), "recovery.scope_mismatch");
}

#[test]
fn should_refuse_to_write_through_a_symlink_that_replaced_the_protected_file() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    let elsewhere = fixture.write("etc/passwd", "root:x:0:0:root:/root:/bin/sh\n");
    std::fs::remove_file(&configuration).expect("the file can be removed");
    std::os::unix::fs::symlink(&elsewhere, &configuration).expect("the link can be made");

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err("§43.5: a replaced symlink target is exactly what must not be written through");
    assert_eq!(error.code().name(), "change.target_changed");
    assert_eq!(
        std::fs::read_to_string(&elsewhere).expect("readable"),
        "root:x:0:0:root:/root:/bin/sh\n",
        "§43.5: protection of one object followed by mutation of a replaced symlink target is \
         unacceptable, so the file behind the link is untouched"
    );
}

#[test]
fn should_refuse_when_a_directory_inside_the_tree_was_replaced_by_a_symlink() {
    let fixture = Fixture::new();
    fixture.write("etc/conf.d/gzip.conf", "gzip on;\n");
    let asset = fixture.protect(&fixture.path("etc"));
    let elsewhere = fixture.path("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("the directory can be made");
    std::fs::remove_dir_all(fixture.path("etc/conf.d")).expect("the directory can be removed");
    std::os::unix::fs::symlink(&elsewhere, fixture.path("etc/conf.d"))
        .expect("the link can be made");

    let error = fixture
        .provider
        .restore(&recovery_action(None), &asset)
        .expect_err("§43.5: the walk refuses a component that is no longer a directory");
    assert_eq!(error.code().name(), "change.target_changed");
    assert!(
        !elsewhere.join("gzip.conf").exists(),
        "§43.5: nothing is written through the link"
    );
}

#[test]
fn should_refuse_when_the_directory_that_held_the_object_is_not_the_one_that_was_protected() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);
    std::fs::remove_dir_all(fixture.path("etc")).expect("the directory can be removed");
    std::fs::create_dir(fixture.path("etc")).expect("a new directory takes its place");

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err(
            "§43.5: the identity of the directory is re-checked before anything is written",
        );
    assert_eq!(error.code().name(), "change.target_changed");
    assert!(
        !configuration.exists(),
        "§43.5: the refusal happens before the write"
    );
}

#[test]
fn should_refuse_to_restore_from_an_asset_that_is_not_ready() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration).expired();

    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect_err("§11.4: only a validated asset is usable for recovery");
    assert_eq!(error.code().name(), "recovery.asset_invalid");
}

#[test]
fn should_refuse_to_restore_from_an_asset_another_provider_owns() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let foreign = ono_change_core::RecoveryAsset::proposed(
        "ono.recovery.zfs",
        RecoveryAssetType::ZfsSnapshot,
        "tank/etc@ono-a82f",
        RecoveryScope::new("zfs-dataset", "tank/etc", "localhost"),
        fixture.provider.now(),
    );
    let error = fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &foreign)
        .expect_err("§12.1: only the owning provider restores from an asset");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}

#[test]
fn should_restore_extended_attributes_where_the_filesystem_carries_them() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    if !supports_xattrs(fixture.root()) {
        let coverage = fixture
            .provider
            .metadata_coverage(&configuration)
            .expect("the coverage can be measured");
        assert!(
            !coverage.xattrs,
            "Appendix C.7: a filesystem without extended attributes is reported as a gap rather \
             than as coverage"
        );
        ono_testkit::skipped(
            ono_testkit::SkipReason::MissingKernelFeature,
            "the scratch filesystem carries no extended attributes, so only the reported gap was \
             checked",
        );
        return;
    }
    rustix::fs::lsetxattr(
        &configuration,
        "user.ono.marker",
        b"kept",
        rustix::fs::XattrFlags::empty(),
    )
    .expect("the attribute can be set");
    let asset = fixture.protect(&configuration);
    rustix::fs::lremovexattr(&configuration, "user.ono.marker")
        .expect("the attribute can be removed");

    fixture
        .provider
        .restore(&recovery_action(Some(&configuration)), &asset)
        .expect("the object comes back");
    let mut value = [0_u8; 32];
    let read = rustix::fs::lgetxattr(&configuration, "user.ono.marker", &mut value[..])
        .expect("Appendix C.7: an attribute this provider claims to restore comes back");
    assert_eq!(&value[..read], b"kept");
}

#[test]
fn should_report_no_metadata_gap_when_it_restored_everything_it_holds() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);

    let report = fixture
        .provider
        .restore_reporting(&recovery_action(Some(&configuration)), &asset)
        .expect("the object comes back");
    assert!(
        report.metadata_gaps().is_empty(),
        "Appendix C.7: a restore of this process's own file puts back everything it captured"
    );
    assert_eq!(report.restored(), &[configuration]);
}
