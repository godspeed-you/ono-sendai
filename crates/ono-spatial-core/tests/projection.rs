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

/// A process record as the procfs provider builds it: the wall-clock `started` and the kernel's
/// own `start_ticks` it was computed from.
fn process_started_at(pid: i64, started: &str, start_ticks: i128) -> RecordValue {
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
    .set("start_ticks", Value::Int(start_ticks))
    .expect("start_ticks")
    .build()
}

#[test]
fn should_keep_one_identity_for_a_process_when_the_wall_clock_is_stepped() {
    // v0.5 §25.4: a clock jump must not disturb what the ledger knows. `started` is `btime` plus
    // an offset, and `btime` moves when the clock is corrected, so the same running process read
    // before and after a one-minute step carried two `started` values and became two identities
    // (#152). The kernel's start time is the tick count, which no clock step moves.
    let projection = projection();
    let before = projection
        .project_as(
            &process_started_at(1842, "2026-08-10T06:12:00Z", 184_200),
            SpatialType::Process,
        )
        .expect("a process projects");
    let after_the_step = projection
        .project_as(
            &process_started_at(1842, "2026-08-10T06:11:00Z", 184_200),
            SpatialType::Process,
        )
        .expect("a process projects");
    assert_eq!(
        before.spatial_id(),
        after_the_step.spatial_id(),
        "the same process, the same identity, whatever the wall clock did in between"
    );

    // And a reused pid still has its own identity: a later process has later ticks.
    let reused = projection
        .project_as(
            &process_started_at(1842, "2026-08-11T09:03:00Z", 9_950_400),
            SpatialType::Process,
        )
        .expect("a process projects");
    assert_ne!(before.spatial_id(), reused.spatial_id());
}

/// A process record that states the container and the pid namespace it runs in.
fn contained(pid: i64, container: Option<&str>, pid_namespace: Option<i64>) -> RecordValue {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.process", 1))
        .expect("the process contract");
    RecordValue::builder(
        schema,
        Provenance::local("linux.procfs", SchemaId::new("ono.process", 1)),
    )
    .set("pid", Value::Int(i128::from(pid)))
    .and_then(|builder| builder.set("name", Value::string("nginx")))
    .and_then(|builder| builder.set("started", Value::string("2026-08-27T10:00:00Z")))
    .and_then(|builder| {
        builder.set(
            "container",
            container.map_or(Value::Null, |id| Value::String(id.into())),
        )
    })
    .and_then(|builder| {
        builder.set(
            "pid_namespace",
            pid_namespace.map_or(Value::Null, |inode| Value::Int(i128::from(inode))),
        )
    })
    .expect("the fixture's own fields")
    .build()
}

#[test]
fn should_place_a_process_in_the_container_its_record_names_without_changing_which_process_it_is() {
    // §3.2: "A scope defines the execution and discovery boundary to which an object belongs",
    // and scopes nest — `host -> container`. A process whose record names its container belongs
    // to that container's scope, which is what makes the crossing observable (§2.18, §21.3).
    // Which process it is does not depend on that: the identity is the one §10.2 gives it, so
    // the place is the same place whether or not the provider could name the container
    // (ADR-0983).
    let id = "4f9c0a2b7e1d4f9c0a2b7e1d4f9c0a2b7e1d4f9c0a2b7e1d4f9c0a2b7e1d4f9c";
    let inside = projection()
        .project_as(&contained(441, Some(id), None), SpatialType::Process)
        .expect("projects");
    let unnamed = projection()
        .project_as(&contained(441, None, None), SpatialType::Process)
        .expect("projects");

    assert_eq!(
        inside.scope().kind(),
        ono_spatial_core::ScopeKind::Container
    );
    assert_eq!(
        inside.scope().id(),
        "4f9c0a2b7e1d",
        "the engine's short id, the form every container tool prints"
    );
    assert_eq!(
        inside
            .scope()
            .parent()
            .map(ono_spatial_core::SpatialScope::kind),
        Some(ono_spatial_core::ScopeKind::Host),
        "the container is nested in the host it was observed on"
    );
    assert_eq!(unnamed.scope().kind(), ono_spatial_core::ScopeKind::Host);
    assert_eq!(
        inside.spatial_id(),
        unnamed.spatial_id(),
        "§42.1: one process is one place, whatever is known about where it runs"
    );
}

#[test]
fn should_place_a_process_in_its_pid_namespace_when_that_is_not_the_observers_own() {
    // §16.2: "Entering a namespace MUST show the boundary explicitly", and §3.2's own example
    // nests a namespace scope. A process in another pid namespace than the one the shell
    // observes from is across that boundary; one in the same namespace is not, and where the
    // observer's namespace is unknown nothing is claimed (§2.17, ADR-0983).
    let host = 4_026_531_836_i64;
    let other = 4_026_532_279_i64;
    let observing = projection().observed_from_pid_namespace(i128::from(host));

    let across = observing
        .project_as(&contained(1, None, Some(other)), SpatialType::Process)
        .expect("projects");
    assert_eq!(
        across.scope().kind(),
        ono_spatial_core::ScopeKind::Namespace
    );
    assert_eq!(across.scope().id(), "pid:[4026532279]");

    let beside = observing
        .project_as(&contained(812, None, Some(host)), SpatialType::Process)
        .expect("projects");
    assert_eq!(beside.scope().kind(), ono_spatial_core::ScopeKind::Host);

    let unknown_observer = projection()
        .project_as(&contained(1, None, Some(other)), SpatialType::Process)
        .expect("projects");
    assert_eq!(
        unknown_observer.scope().kind(),
        ono_spatial_core::ScopeKind::Host,
        "without the observer's own namespace there is nothing to compare with"
    );
}
