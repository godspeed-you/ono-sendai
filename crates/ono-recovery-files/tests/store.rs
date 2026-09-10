#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

//! The private store and the secret policy (spec v0.6 §15.3, §15.5, §44).

mod support;

use std::os::unix::fs::PermissionsExt as _;

use ono_change_core::{RecoveryGoal, RecoveryProvider};
use ono_recovery_files::{DIRECTORY_MODE, FILE_MODE, FileRecoveryStore};
use support::Fixture;

/// The one string no rendering may ever contain.
const SECRET: &str = "ssl_certificate_key /etc/ssl/private/site.key; # hunter2";

fn mode_of(path: &std::path::Path) -> u32 {
    std::fs::symlink_metadata(path)
        .expect("the path exists")
        .permissions()
        .mode()
        & 0o7777
}

#[test]
fn should_create_the_store_private_to_its_owner_when_it_is_opened() {
    let fixture = Fixture::new();
    assert_eq!(
        mode_of(fixture.provider.store().root()),
        DIRECTORY_MODE,
        "§15.3 and §44.3: the recovery store's permissions MUST prevent other users from reading \
         sensitive configuration"
    );
}

#[test]
fn should_tighten_a_store_directory_a_careless_umask_left_readable() {
    let fixture = Fixture::new();
    let root = fixture.path("wide-store");
    std::fs::create_dir_all(&root).expect("the directory can be made");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777))
        .expect("the mode can be widened");
    FileRecoveryStore::open(&root).expect("the store opens over an existing directory");
    assert_eq!(
        mode_of(&root),
        DIRECTORY_MODE,
        "§44.3: a store that was left world-readable is narrowed before anything is written to it"
    );
}

#[test]
fn should_write_every_recovery_copy_private_to_its_owner_when_an_asset_is_created() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", SECRET);
    let asset = fixture.protect(&configuration);

    let directory = std::path::PathBuf::from(asset.reference());
    assert_eq!(
        mode_of(&directory),
        DIRECTORY_MODE,
        "§15.3: the archive's own directory is private to its owner"
    );
    for entry in walk(&directory) {
        let expected = if entry.is_dir() {
            DIRECTORY_MODE
        } else {
            FILE_MODE
        };
        assert_eq!(
            mode_of(&entry),
            expected,
            "§44.3: `{}` holds a recovery copy and MUST NOT be readable by another user",
            entry.display()
        );
    }
}

#[test]
fn should_keep_the_protected_content_out_of_the_asset_when_it_is_rendered() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", SECRET);
    let asset = fixture.protect(&configuration);

    let rendered = format!("{asset:?}");
    assert!(
        !rendered.contains("hunter2"),
        "§15.5 and §44.2: recovery copies may contain secrets and their contents MUST NOT appear \
         in default rendering"
    );
    assert!(
        rendered.contains("nginx.conf"),
        "§44.1: the metadata may appear, which is what makes the asset inspectable at all"
    );
}

#[test]
fn should_keep_the_protected_content_out_of_the_recovery_plan_when_it_is_rendered() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", SECRET);
    let asset = fixture.protect(&configuration);

    let fragment = fixture
        .provider
        .plan_recovery(&asset, None, RecoveryGoal::RestoreChangedObjects)
        .expect("a ready asset plans its own recovery");
    assert!(
        !format!("{fragment:?}").contains("hunter2"),
        "§44.2: contents MUST NOT appear in default rendering, and a plan fragment is rendering"
    );
}

#[test]
fn should_keep_the_copied_bytes_only_inside_the_store_when_an_asset_is_created() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", SECRET);
    let asset = fixture.protect(&configuration);

    let store = fixture.provider.store().root().to_path_buf();
    let copies: Vec<_> = walk(&store)
        .into_iter()
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|text| text.contains("hunter2")))
        .collect();
    assert_eq!(
        copies.len(),
        1,
        "§15.3: exactly one copy exists, and it is inside the recovery store"
    );
    let outside: Vec<_> = walk(fixture.root())
        .into_iter()
        .filter(|path| !path.starts_with(&store))
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|text| text.contains("hunter2")))
        .collect();
    assert_eq!(
        outside,
        vec![configuration.clone()],
        "§15.3: outside the store, the only file holding the protected content is the file itself"
    );
    assert!(
        asset.reference().starts_with(&store.display().to_string()),
        "§15.3: the asset's reference names a place inside the Ono recovery store"
    );
}

#[test]
fn should_write_nothing_outside_the_scratch_directory_when_an_asset_is_created() {
    let fixture = Fixture::new();
    let configuration = fixture.write("etc/nginx.conf", "worker_processes 1;\n");
    let asset = fixture.protect(&configuration);

    let scratch = fixture.root().display().to_string();
    assert!(
        asset.reference().starts_with(&scratch),
        "AGENTS.md §11: a suite writes only inside its own scratch directory"
    );
    for object in asset.scope().covers() {
        assert!(
            object.starts_with(&scratch),
            "every object the asset covers is inside the scratch directory"
        );
    }
}

#[test]
fn should_report_the_store_as_unavailable_when_its_directory_is_gone() {
    let fixture = Fixture::new();
    std::fs::remove_dir_all(fixture.provider.store().root()).expect("the store can be removed");
    let availability = fixture.provider.availability();
    assert!(
        !availability.is_available(),
        "§12.2: a provider that cannot run says so rather than answering discovery with nothing"
    );
    assert!(
        availability
            .reason()
            .is_some_and(|reason| reason.contains("cannot be read")),
        "§55.6 case 29: the reason is a sentence a person can act on"
    );
}

#[test]
fn should_report_the_store_as_unavailable_when_its_permissions_are_widened() {
    let fixture = Fixture::new();
    std::fs::set_permissions(
        fixture.provider.store().root(),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("the mode can be widened");
    let availability = fixture.provider.availability();
    assert!(
        !availability.is_available(),
        "§44.3: a store other users can read is not a store this provider will write secrets into"
    );
}

#[test]
fn should_report_the_store_as_available_when_it_is_private_and_writable() {
    let fixture = Fixture::new();
    let availability = fixture.provider.availability();
    assert!(availability.is_available());
    assert!(
        availability.reason().is_none(),
        "§12.2: an available provider has nothing to explain"
    );
}

#[test]
fn should_decline_to_protect_its_own_store_when_it_is_asked_to() {
    let fixture = Fixture::new();
    let inside = fixture.provider.store().root().join("something");
    let domain = fixture
        .provider
        .resolve_domain(&inside.display().to_string())
        .expect("resolving a path inside the store is not an error");
    assert!(
        domain.is_none(),
        "§15.3: the recovery store is not this provider's business to protect, and archiving it \
         into itself would recurse"
    );
}

/// Every path beneath `root`, directories included.
fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        }
        found.push(path);
    }
    found
}
