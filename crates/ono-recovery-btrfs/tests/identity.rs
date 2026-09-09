#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Subvolume identity, and the trap Appendix B.9 sets for a provider that reads names (§14.1).

mod support;

use ono_change_core::RecoveryProvider;
use ono_recovery_btrfs::{SubvolumeRef, SCOPE_KIND};
use support::{FILESYSTEM, NESTED_ID, ROOT_ID, VAR_ID, fixture, provider};

/// The three answers `resolve_domain` asks for, in the order it asks them.
fn identity_script(show: &str) -> Vec<ono_change_core::ToolOutput> {
    vec![
        fixture("fs-show-mount"),
        fixture(show),
        fixture("subvol-list-root"),
    ]
}

#[test]
fn should_resolve_a_plain_directory_to_its_containing_subvolume_when_its_name_looks_like_one() {
    let provider = provider(identity_script("subvol-show-lookslike"));
    let domain = provider
        .resolve_domain("/mnt/root/looks-like-a-subvol")
        .expect("the resolution runs")
        .expect("the path is on a Btrfs filesystem");
    assert_eq!(
        domain.object(),
        Some(SubvolumeRef::new(FILESYSTEM, ROOT_ID, "@").reference().as_str()),
        "Appendix B.9: `looks-like-a-subvol` is an ordinary directory inside `@`, and a \
         subdirectory named like a subvolume is not sufficient evidence of one. Its state lives \
         in subvolume 256"
    );
    assert_eq!(domain.boundary(), Some("@"));
    assert!(
        domain.detail().contains("Not a Btrfs subvolume"),
        "the resolution says what the filesystem answered, so a reader can see the evidence \
         rather than trust the conclusion"
    );
}

#[test]
fn should_resolve_a_nested_subvolume_to_itself_rather_than_to_its_parent() {
    let provider = provider(identity_script("subvol-show-nested"));
    let domain = provider
        .resolve_domain("/mnt/root/var/lib-app")
        .expect("the resolution runs")
        .expect("the path is on a Btrfs filesystem");
    assert_eq!(
        domain.object(),
        Some(
            SubvolumeRef::new(FILESYSTEM, NESTED_ID, "@var/lib-app")
                .reference()
                .as_str()
        ),
        "§14.3: a nested subvolume is its own snapshot boundary, and a target inside it belongs \
         to it rather than to `@var`"
    );
}

#[test]
fn should_resolve_a_directory_inside_a_subvolume_to_that_subvolume() {
    let provider = provider(identity_script("subvol-show-plaindir"));
    let domain = provider
        .resolve_domain("/mnt/root/var/plain-dir")
        .expect("the resolution runs")
        .expect("the path is on a Btrfs filesystem");
    assert_eq!(
        domain.object(),
        Some(
            SubvolumeRef::new(FILESYSTEM, VAR_ID, "@var")
                .reference()
                .as_str()
        ),
        "§14.1: the containing subvolume is what holds the state, and `@var` is the deepest \
         subvolume the path descends into"
    );
    assert_eq!(domain.object_kind(), SCOPE_KIND);
}

#[test]
fn should_resolve_a_path_that_does_not_exist_yet_to_the_subvolume_that_will_hold_it() {
    let provider = provider(identity_script("subvol-show-missing"));
    let domain = provider
        .resolve_domain("/mnt/root/nothing-here")
        .expect("the resolution runs")
        .expect("the path is on a Btrfs filesystem");
    assert_eq!(domain.boundary(), Some("@"));
    assert!(
        domain.is_protectable(),
        "§11.2: a file a plan is about to create still has a persistence domain, and protecting \
         it is protecting the subvolume it will land in"
    );
}

#[test]
fn should_carry_both_halves_of_the_identity_the_specification_asks_for() {
    let provider = provider(identity_script("subvol-show-root"));
    let domain = provider
        .resolve_domain("/mnt/root/etc/nginx/nginx.conf")
        .expect("the resolution runs")
        .expect("the path is on a Btrfs filesystem");
    let reference = SubvolumeRef::parse(domain.object().expect("a resolved object"))
        .expect("the object names a subvolume");
    assert_eq!(
        reference.filesystem(),
        FILESYSTEM,
        "Appendix D.6: the protection action names the filesystem uuid as well as the subvolume id"
    );
    assert_eq!(reference.id(), ROOT_ID);
    assert_eq!(
        domain.mount().option("subvolid"),
        Some("256"),
        "Appendix B.9: the mount's own `subvolid=` is the second reading of the same identity"
    );
    assert_eq!(domain.mount().root(), "/@");
}

#[test]
fn should_answer_nothing_for_a_path_on_a_filesystem_this_provider_does_not_own() {
    let provider = provider(Vec::new());
    assert_eq!(
        provider
            .resolve_domain("/home/william/notes.txt")
            .expect("the resolution runs"),
        None,
        "a path served by no Btrfs mount is not this provider's business, which is a different \
         answer from `there is nothing to protect here`"
    );
}

#[test]
fn should_refuse_to_resolve_when_the_subvolume_search_is_not_permitted() {
    let provider = provider(vec![
        fixture("fs-show-mount"),
        fixture("unprivileged-list"),
        fixture("subvol-list-root"),
    ]);
    let error = provider
        .resolve_domain("/mnt/root/etc/nginx/nginx.conf")
        .expect_err("§56.3: an unprivileged refusal blocks rather than resolving to a guess");
    assert_eq!(error.code().name(), "recovery.provider_unavailable");
}

#[test]
fn should_refuse_to_resolve_when_the_filesystem_cannot_be_identified() {
    let provider = provider(vec![
        support::failure("ERROR: not a btrfs filesystem"),
        fixture("subvol-show-root"),
        fixture("subvol-list-root"),
    ]);
    assert!(
        provider
            .resolve_domain("/mnt/root/etc/nginx/nginx.conf")
            .is_err(),
        "§56.2: the exact filesystem is part of the identity, and a subvolume id without it is \
         unique to nothing"
    );
}

#[test]
fn should_refuse_a_reference_that_names_only_half_a_subvolume() {
    assert_eq!(
        SubvolumeRef::parse("1ba9ceee"),
        None,
        "§56.3: half a subvolume identity is not an identity"
    );
    assert_eq!(SubvolumeRef::parse("uuid:notanumber:@"), None);
    let reference = SubvolumeRef::parse("uuid-here:258:@var").expect("a whole reference");
    assert_eq!(reference.id(), 258);
    assert_eq!(reference.tree_path(), "@var");
}

#[test]
fn should_describe_a_subvolume_with_both_its_id_and_its_filesystem() {
    let described = SubvolumeRef::new(FILESYSTEM, VAR_ID, "@var").describe();
    assert!(described.contains("258"), "§14.1: the stable id is named");
    assert!(
        described.contains(FILESYSTEM),
        "§56.2: so is the filesystem it is unique within"
    );
}
