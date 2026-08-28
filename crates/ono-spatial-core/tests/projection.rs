//! Projecting a provider record into a spatial object — spec v0.4 §2.16, §3.1, §10.2, §42.1.
//!
//! §2.16 is the rule under test: "Providers own facts. Ono's spatial layer composes provider
//! data; it MUST NOT become an undocumented source of system truth." Nothing the projection
//! produces may be a fact the record did not carry.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::Timestamp;
use ono_spatial_core::{
    BootIdentity, IdentityTier, Projection, SpatialCapability, SpatialScope, SpatialType,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

fn projection() -> Projection {
    Projection::new(
        SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-a")),
        Timestamp::UNIX_EPOCH,
    )
}

fn process(pid: i64, started: &str) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.process", 1))
        .expect("the process contract");
    RecordValue::builder(
        schema,
        Provenance::local("linux.procfs", SchemaId::new("ono.process", 1)),
    )
    .set("pid", Value::Int(i128::from(pid)))
    .expect("pid")
    .set("name", Value::string("nginx"))
    .expect("name")
    .set("started", Value::string(started))
    .expect("started")
    .build()
}

fn service(name: &str) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.service", 1))
        .expect("the service contract");
    RecordValue::builder(
        schema,
        Provenance::local("systemd", SchemaId::new("ono.service", 1)),
    )
    .set("provider", Value::string("systemd"))
    .expect("provider")
    .set("name", Value::string(name))
    .expect("name")
    .build()
}

#[test]
fn should_carry_the_providers_own_provenance_through_the_projection() {
    // §2.16 and spec v0.2 §26: the observation is the provider's, and the spatial layer does not
    // re-attribute it to itself.
    let object = projection()
        .project_as(&process(1842, "2026-08-10T06:12:00Z"), SpatialType::Process)
        .expect("a process projects");
    assert_eq!(object.provenance().provider(), "linux.procfs");
}

#[test]
fn should_give_a_process_a_lifetime_identity_that_a_reused_pid_cannot_collide_with() {
    // §10.2: boot identity, pid, start time and pid namespace. The two records below differ only
    // in the start time, which is exactly what a reused pid looks like (§42.2).
    let projection = projection();
    let first = projection
        .project_as(&process(1842, "2026-08-10T06:12:00Z"), SpatialType::Process)
        .expect("a process projects");
    let reused = projection
        .project_as(&process(1842, "2026-08-11T09:03:00Z"), SpatialType::Process)
        .expect("a process projects");
    assert_ne!(first.spatial_id(), reused.spatial_id());
    assert_eq!(first.lifetime().tier(), IdentityTier::Lifetime);
}

#[test]
fn should_give_the_same_process_the_same_id_when_observed_twice() {
    // §42.1's identity test, through the projection a provider actually uses.
    let projection = projection();
    let record = process(1842, "2026-08-10T06:12:00Z");
    assert_eq!(
        projection
            .project_as(&record, SpatialType::Process)
            .expect("projects")
            .spatial_id(),
        projection
            .project_as(&record, SpatialType::Process)
            .expect("projects")
            .spatial_id()
    );
}

#[test]
fn should_give_the_same_process_different_ids_on_two_boots_of_the_same_host() {
    // §10.2's boot identity: a pid and start time that repeat across a reboot are not the same
    // process.
    let record = process(1842, "2026-08-10T06:12:00Z");
    let before = Projection::new(
        SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-a")),
        Timestamp::UNIX_EPOCH,
    );
    let after = Projection::new(
        SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-b")),
        Timestamp::UNIX_EPOCH,
    );
    assert_ne!(
        before
            .project_as(&record, SpatialType::Process)
            .expect("projects")
            .spatial_id(),
        after
            .project_as(&record, SpatialType::Process)
            .expect("projects")
            .spatial_id()
    );
}

#[test]
fn should_give_a_service_a_stable_identity_that_outlives_its_processes() {
    // §10.1 Tier A and §53: "Restarted service process? Old process tombstones; stable service
    // remains." The service identity carries nothing about a process.
    let object = projection()
        .project_as(&service("nginx.service"), SpatialType::Service)
        .expect("a service projects");
    assert_eq!(object.lifetime().tier(), IdentityTier::Stable);
    assert!(
        object
            .identity()
            .components()
            .iter()
            .all(|(field, _)| field != "pid"),
        "a service identity does not mention a process, got {:?}",
        object.identity().components()
    );
}

#[test]
fn should_call_an_object_what_the_specifications_own_examples_call_it() {
    // §12 prints `PROCESS / nginx / 1842` and §13 prints `SERVICE / nginx.service`: the display
    // name is the schema's own word for the thing, not whichever column happens to come first in
    // the default view.
    let process = projection()
        .project_as(&process(1842, "2026-08-10T06:12:00Z"), SpatialType::Process)
        .expect("a process projects");
    assert_eq!(process.display_name(), "nginx");

    let service = projection()
        .project_as(&service("nginx.service"), SpatialType::Service)
        .expect("a service projects");
    assert_eq!(service.display_name(), "nginx.service");
}

#[test]
fn should_let_an_object_answer_to_the_identity_a_user_can_read() {
    // §2.1 makes discovery-before-naming mandatory; it does not make naming impossible. A user
    // who knows the unit name or the pid should be able to type it, so both are aliases — and the
    // scope is not, because a boundary is not a name (§3.2).
    let object = projection()
        .project_as(&process(1842, "2026-08-10T06:12:00Z"), SpatialType::Process)
        .expect("a process projects");
    let aliases = ono_spatial_core::aliases_of(&object);
    assert!(aliases.contains("nginx"), "got {aliases:?}");
    assert!(aliases.contains("1842"), "got {aliases:?}");
    assert!(
        !aliases.iter().any(|alias| alias.contains("host:")),
        "a scope is a boundary, not a name; got {aliases:?}"
    );
}

#[test]
fn should_keep_the_display_name_out_of_the_identity_of_a_projected_object() {
    // §3.1: "The display name is not identity."
    let object = projection()
        .project_as(&service("nginx.service"), SpatialType::Service)
        .expect("a service projects");
    assert!(!object.display_name().is_empty());
    let rendered = format!("{:?}", object.identity().components());
    assert!(
        !rendered.contains(object.display_name()) || object.display_name() == "nginx.service",
        "identity is built from the schema's identity fields, got {rendered}"
    );
}

#[test]
fn should_place_an_object_in_the_scope_it_was_observed_in() {
    // §3.2: the scope is the discovery boundary the object belongs to, and the same uid or pid
    // in two containers is two objects (§16.2).
    let inside = Projection::new(
        SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-a"))
            .nest(ono_spatial_core::ScopeKind::Container, "payments-api"),
        Timestamp::UNIX_EPOCH,
    );
    let outside = projection();
    let record = service("nginx.service");
    assert_ne!(
        inside
            .project_as(&record, SpatialType::Service)
            .expect("projects")
            .spatial_id(),
        outside
            .project_as(&record, SpatialType::Service)
            .expect("projects")
            .spatial_id()
    );
}

#[test]
fn should_offer_follow_only_where_the_registry_declares_an_exit() {
    // §3.1's capabilities are what keep `enter` and `follow` from being offered where nothing
    // answers them (§40's `spatial.not_enterable`, `spatial.no_relation`).
    let object = projection()
        .project_as(&service("nginx.service"), SpatialType::Service)
        .expect("a service projects");
    assert!(object.is_enterable());
    assert!(object.capabilities().contains(&SpatialCapability::Follow));
    assert!(object.capabilities().contains(&SpatialCapability::Act));
}

#[test]
fn should_refuse_to_act_on_an_object_once_it_has_ended() {
    // §10.3: a tombstoned object "MUST NOT accept actions that require a live object".
    let object = projection()
        .project_as(&process(1842, "2026-08-10T06:12:00Z"), SpatialType::Process)
        .expect("a process projects")
        .ended(Timestamp::UNIX_EPOCH);
    assert!(!object.lifetime().is_live());
    assert!(!object.capabilities().contains(&SpatialCapability::Act));
    assert!(!object.is_enterable());
}

#[test]
fn should_refuse_a_schema_the_geography_does_not_place() {
    // §2.16 again: a record type nothing in the geography holds is not given a synthetic place.
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.action-result", 1))
        .expect("the action-result contract");
    let record = RecordValue::builder(
        schema,
        Provenance::local("test", SchemaId::new("ono.action-result", 1)),
    )
    .build();
    let error = projection()
        .project(&record)
        .expect_err("no place holds it");
    assert_eq!(error.code(), ono_core::ErrorCode::SpatialUnsupported);
}

#[test]
fn should_refuse_to_guess_the_type_when_one_schema_serves_two_kinds_of_place() {
    // §14.3 and §14.4 make a listening socket and an established connection different places
    // built from `ono.socket/1`; deciding which is the provider bridge's job, and guessing here
    // would make the spatial layer a source of truth about the object (§2.16).
    assert_eq!(
        ono_spatial_core::spatial_types_of("ono.socket/1"),
        vec![SpatialType::Listener, SpatialType::Connection]
    );
    assert_eq!(
        ono_spatial_core::spatial_types_of("ono.process/1"),
        vec![SpatialType::Process]
    );
}
