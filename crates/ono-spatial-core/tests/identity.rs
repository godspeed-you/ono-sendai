//! `SpatialId` stability, the three tiers and process identity — spec v0.4 §3.1, §10, §42.1,
//! §42.2, and the unit coverage §43.1 requires ("`SpatialId` stability rules").

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_spatial_core::{
    BootIdentity, IdentityTier, ProcessIdentity, SpatialId, SpatialIdentity, SpatialType,
};

fn boot() -> BootIdentity {
    BootIdentity::new("testbox", "4d0a1f2b-0000-4000-8000-000000000001")
}

#[test]
fn should_resolve_two_observations_of_one_object_to_the_same_id_when_the_facts_are_the_same() {
    // §42.1: "Repeated observations of the same live object MUST resolve to the same
    // `SpatialId` within the provider's advertised identity tier."
    let first = SpatialIdentity::stable(SpatialType::Service, [("name", "nginx.service")]);
    let second = SpatialIdentity::stable(SpatialType::Service, [("name", "nginx.service")]);
    assert_eq!(first.spatial_id(), second.spatial_id());
}

#[test]
fn should_give_a_different_id_to_a_different_object_of_the_same_type() {
    let nginx = SpatialIdentity::stable(SpatialType::Service, [("name", "nginx.service")]);
    let postgres = SpatialIdentity::stable(SpatialType::Service, [("name", "postgres.service")]);
    assert_ne!(nginx.spatial_id(), postgres.spatial_id());
}

#[test]
fn should_give_a_different_id_to_the_same_name_under_a_different_type() {
    // §3.1: identity is the object, not its rendering. A user called `nginx` and a service
    // called `nginx` are two objects, and a shared display name may not merge them.
    let user = SpatialIdentity::stable(SpatialType::User, [("name", "nginx")]);
    let service = SpatialIdentity::stable(SpatialType::Service, [("name", "nginx")]);
    assert_ne!(user.spatial_id(), service.spatial_id());
}

#[test]
fn should_keep_the_display_name_out_of_identity_when_it_changes() {
    // §3.1: "The display name is not identity." A service renamed in its unit file's Description
    // is the same service; nothing about the display name is in the identity, so there is no way
    // for one to reach the id.
    let identity = SpatialIdentity::stable(SpatialType::Service, [("name", "nginx.service")]);
    assert!(
        identity
            .components()
            .iter()
            .all(|(field, _)| field != "display_name" && field != "label"),
        "identity components are the facts that make the object, got {:?}",
        identity.components()
    );
}

#[test]
fn should_show_the_identity_tier_in_the_rendered_id_so_persistence_is_never_implied() {
    // §10.1: "The renderer MUST NOT imply stable persistence for Tier C objects." The tier
    // travels with the id, so a renderer never has to guess.
    for tier in IdentityTier::ALL {
        let identity = SpatialIdentity::new(*tier, SpatialType::Endpoint, [("addr", "10.0.0.1")]);
        assert_eq!(identity.spatial_id().tier(), Some(*tier));
    }
    assert!(!IdentityTier::Observation.implies_persistence());
    assert!(IdentityTier::Stable.implies_persistence());
    assert!(IdentityTier::Lifetime.implies_persistence());
}

#[test]
fn should_refuse_to_read_back_a_hand_written_id() {
    // §3.1: the id is opaque. A user copies one; nobody composes one, because a composed id
    // would be a way around the identity rules.
    assert_eq!(SpatialId::parse("process/1842"), None);
    assert_eq!(SpatialId::parse("ono:stable:not-hex"), None);
    assert_eq!(
        SpatialId::parse("ono:invented:0123456789abcdef0123456789abcdef"),
        None
    );
    let real = SpatialId::of_space("compute.services");
    assert_eq!(SpatialId::parse(real.as_str()), Some(real));
}

#[test]
fn should_keep_a_canonical_spaces_id_the_same_across_sessions() {
    // §7.1 makes the root an orientation anchor: it is the same place in every session, because
    // nothing about a session went into its identity.
    assert_eq!(SpatialId::of_space("system"), SpatialId::of_space("system"));
    assert_ne!(
        SpatialId::of_space("system"),
        SpatialId::of_space("compute")
    );
}

#[test]
fn should_give_a_reused_pid_a_different_identity_than_the_process_that_had_it_before() {
    // §42.2, the reuse-safety test: "the provider MUST prove that identifier reuse cannot
    // silently resolve a tombstoned place to a different object". §2.8 and §10.2 are why: the
    // pid is an attribute, the start time is what makes the lifetime.
    let first = ProcessIdentity::new(boot(), 1842, 1_700_000_000, Some(4_026_531_836));
    let reused = ProcessIdentity::new(boot(), 1842, 1_700_009_999, Some(4_026_531_836));
    assert_eq!(first.pid(), reused.pid(), "the test is about a reused pid");
    assert_ne!(
        first.spatial_id(),
        reused.spatial_id(),
        "§10.2: pid alone is not identity, so a reused pid is a different process"
    );
}

#[test]
fn should_give_the_same_pid_in_two_namespaces_two_identities() {
    // §10.2 lists the pid namespace among the four parts for the reason this test states: the
    // same number means different processes in different namespaces (§16.2).
    let host = ProcessIdentity::new(boot(), 1, 1_700_000_000, Some(4_026_531_836));
    let container = ProcessIdentity::new(boot(), 1, 1_700_000_000, Some(4_026_533_331));
    assert_ne!(host.spatial_id(), container.spatial_id());
}

#[test]
fn should_give_the_same_process_on_two_hosts_two_identities() {
    // §10.2's boot identity covers both a reboot and a second host: neither may collide.
    let here = ProcessIdentity::new(boot(), 1842, 1_700_000_000, None);
    let there = ProcessIdentity::new(
        BootIdentity::new("web01", "4d0a1f2b-0000-4000-8000-000000000001"),
        1842,
        1_700_000_000,
        None,
    );
    let after_reboot = ProcessIdentity::new(
        BootIdentity::new("testbox", "9999ffff-0000-4000-8000-000000000002"),
        1842,
        1_700_000_000,
        None,
    );
    assert_ne!(here.spatial_id(), there.spatial_id());
    assert_ne!(here.spatial_id(), after_reboot.spatial_id());
}

#[test]
fn should_resolve_the_same_process_to_the_same_id_however_often_it_is_observed() {
    // §42.1 again, for the tier that needs it most: a process observed twice is one place, or
    // navigation to it is not navigation at all.
    let identity = ProcessIdentity::new(boot(), 1842, 1_700_000_000, Some(4_026_531_836));
    let again = ProcessIdentity::new(boot(), 1842, 1_700_000_000, Some(4_026_531_836));
    assert_eq!(identity.spatial_id(), again.spatial_id());
}

#[test]
fn should_record_an_unreadable_pid_namespace_as_unknown_rather_than_as_the_root() {
    // §2.17: unknown is visible. Guessing the root namespace would silently merge a container's
    // pid 1 with the host's.
    let unknown = ProcessIdentity::new(boot(), 1, 1_700_000_000, None);
    let root = ProcessIdentity::new(boot(), 1, 1_700_000_000, Some(4_026_531_836));
    assert_eq!(unknown.pid_namespace(), None);
    assert_ne!(unknown.spatial_id(), root.spatial_id());
}

#[test]
fn should_say_when_the_boot_it_names_is_not_known() {
    assert!(boot().is_known());
    assert!(!BootIdentity::unknown_boot("testbox").is_known());
}
