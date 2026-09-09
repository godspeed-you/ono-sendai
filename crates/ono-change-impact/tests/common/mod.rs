//! Fixtures shared by the impact, risk and mutation suites.
//!
//! Everything here is built from the shipped contracts and the declared v0.4 relations, so a
//! fixture cannot describe a world the real spatial layer could not hold. Nothing reads a clock:
//! every observation is made at [`NOW`], which is what makes the suites deterministic (AGENTS.md
//! section 11).

#![allow(
    dead_code,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a shared test fixture is used by some suites and not by others (AGENTS.md section 16)"
)]

use jiff::Timestamp;
use ono_change_core::{
    ActionRole, EffectConfidence, EffectDomain, EffectKind, Execution, FrozenTarget, PlanAction,
    PlanId, ProposedEffect,
};
use ono_spatial_core::{
    BootIdentity, Confidence, Projection, RelationType, RelationshipEdge, SpatialObject,
    SpatialScope, SpatialType,
};
use ono_spatial_index::{FreshnessPolicy, SpatialIndex};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// The instant every fixture observation is made at.
pub const NOW: Timestamp = Timestamp::UNIX_EPOCH;

/// The scope the fixtures are observed in.
pub fn scope() -> SpatialScope {
    SpatialScope::host("testbox", BootIdentity::new("testbox", "boot-a"))
}

/// A projection into that scope, at [`NOW`].
pub fn projection() -> Projection {
    Projection::new(scope(), NOW)
}

/// A record of the shipped schema `schema`, carrying `fields`.
pub fn record(schema: &str, fields: &[(&str, Value)]) -> RecordValue {
    let id: SchemaId = schema.parse().expect("a well-formed schema id");
    let contract = builtin_schemas()
        .get(&id)
        .unwrap_or_else(|| panic!("{schema} is a shipped contract"));
    let mut builder = RecordValue::builder(contract, Provenance::local("test", id));
    for (name, value) in fields {
        builder = builder
            .set(name, value.clone())
            .unwrap_or_else(|error| panic!("{schema}.{name}: {}", error.message()));
    }
    builder.build()
}

/// A service place, as systemd would describe it.
pub fn service(name: &str) -> SpatialObject {
    projection()
        .project_as(
            &record(
                "ono.service/1",
                &[
                    ("name", Value::string(name)),
                    ("state", Value::string("running")),
                    ("provider", Value::string("systemd")),
                ],
            ),
            SpatialType::Service,
        )
        .expect("a service projects")
}

/// A process place.
pub fn process(pid: i64, name: &str) -> SpatialObject {
    projection()
        .project_as(
            &record(
                "ono.process/1",
                &[
                    ("pid", Value::Int(i128::from(pid))),
                    ("name", Value::string(name)),
                    ("state", Value::string("sleeping")),
                    ("started", Value::string("2026-08-10T06:12:00Z")),
                ],
            ),
            SpatialType::Process,
        )
        .expect("a process projects")
}

/// A file place, identified by device and inode as `ono.file/1` requires.
pub fn file(path: &str, inode: i64) -> SpatialObject {
    projection()
        .project_as(
            &record(
                "ono.file/1",
                &[
                    ("path", Value::string(path)),
                    (
                        "name",
                        Value::string(path.rsplit('/').next().unwrap_or(path)),
                    ),
                    ("kind", Value::string("file")),
                    ("device", Value::Int(64)),
                    ("inode", Value::Int(i128::from(inode))),
                ],
            ),
            SpatialType::File,
        )
        .expect("a file projects")
}

/// A listening socket place.
pub fn listener(inode: i64, port: i64) -> SpatialObject {
    let endpoint = Value::Record(std::sync::Arc::new(record(
        "ono.endpoint/1",
        &[
            (
                "address",
                Value::Ip("0.0.0.0".parse().expect("a fixture address")),
            ),
            ("port", Value::Int(i128::from(port))),
        ],
    )));
    projection()
        .project_as(
            &record(
                "ono.socket/1",
                &[
                    ("protocol", Value::string("tcp")),
                    ("family", Value::string("inet")),
                    ("inode", Value::Int(i128::from(inode))),
                    ("state", Value::string("listen")),
                    ("local", endpoint),
                ],
            ),
            SpatialType::Listener,
        )
        .expect("a listener projects")
}

/// A control group place, which is what proves shared role membership (§28.3).
pub fn cgroup(path: &str) -> SpatialObject {
    projection()
        .project_as(
            &record(
                "ono.cgroup/1",
                &[
                    ("path", Value::string(path)),
                    (
                        "name",
                        Value::string(path.rsplit('/').next().unwrap_or(path)),
                    ),
                ],
            ),
            SpatialType::Cgroup,
        )
        .expect("a cgroup projects")
}

/// A container place, the other kind of object a membership relation groups (§28.3).
pub fn container(id: &str) -> SpatialObject {
    projection()
        .project_as(
            &record(
                "ono.container/1",
                &[
                    ("id", Value::string(id)),
                    ("state", Value::string("running")),
                ],
            ),
            SpatialType::Container,
        )
        .expect("a container projects")
}

/// A host place, the one kind whose exits are acquired at `external` cost (v0.4 §34.2).
pub fn host(name: &str) -> SpatialObject {
    projection()
        .project_as(
            &record(
                "ono.host/1",
                &[
                    ("name", Value::string(name)),
                    ("source", Value::string("link-table")),
                ],
            ),
            SpatialType::Host,
        )
        .expect("a host projects")
}

/// An edge asserting `relation` between two places, at the confidence the provider claimed.
pub fn edge(
    source: &SpatialObject,
    target: &SpatialObject,
    relation: &str,
    confidence: Confidence,
) -> RelationshipEdge {
    RelationshipEdge::new(
        source.spatial_id().clone(),
        target.spatial_id().clone(),
        RelationType::new(relation).expect("a declared relation"),
        confidence,
        Provenance::local("systemd", SchemaId::new("ono.service", 1)),
        NOW,
    )
}

/// An index holding `objects` and `edges`, observed at [`NOW`].
pub fn world(objects: &[SpatialObject], edges: &[RelationshipEdge]) -> SpatialIndex {
    let mut index = SpatialIndex::new(FreshnessPolicy::default());
    for object in objects {
        index
            .register(object.clone(), NOW)
            .expect("a fixture object registers");
    }
    for edge in edges {
        index.record_edge(edge.clone());
    }
    index
}

/// The identity of the fixture plan every action belongs to.
pub fn plan() -> PlanId {
    PlanId::derive(&["ono-change-impact", "fixture"])
}

/// A mutating action against `target`, carried out through a provider operation (§2.17).
pub fn mutate(ordinal: usize, summary: &str, target: &str) -> PlanAction {
    PlanAction::new(
        &plan(),
        ordinal,
        ActionRole::Mutate,
        summary,
        Execution::ProviderAction {
            provider: "ono.change.systemd".into(),
            operation: "ono.service.restart".into(),
            arguments: Vec::new(),
        },
    )
    .on(target)
}

/// An action the operator declared opaque (§6.3).
pub fn opaque(ordinal: usize, summary: &str, description: &str) -> PlanAction {
    PlanAction::new(
        &plan(),
        ordinal,
        ActionRole::Mutate,
        summary,
        Execution::Opaque {
            description: description.into(),
            program: None,
            argv: Vec::new(),
        },
    )
}

/// A verifying action, which changes nothing (§3.3).
pub fn verify(ordinal: usize, summary: &str) -> PlanAction {
    PlanAction::new(
        &plan(),
        ordinal,
        ActionRole::Verify,
        summary,
        Execution::ProviderAction {
            provider: "ono.change.systemd".into(),
            operation: "ono.service.get".into(),
            arguments: Vec::new(),
        },
    )
}

/// An effect of `action`, landing on `object`.
pub fn effect(
    action: &PlanAction,
    domain: EffectDomain,
    kind: EffectKind,
    confidence: EffectConfidence,
    explanation: &str,
    object: &str,
) -> ProposedEffect {
    ProposedEffect::new(action.id().clone(), domain, kind, confidence, explanation).on(object)
}

/// A frozen target for a place the index holds (§7.1).
pub fn target_of(object: &SpatialObject, schema: &str) -> FrozenTarget {
    FrozenTarget::new(
        schema,
        object.spatial_id().as_str(),
        object.display_name().to_owned(),
    )
    .at_place(object.spatial_id().as_str())
}

/// A frozen target for something the spatial layer does not hold.
pub fn target_named(schema: &str, identity: &str, label: &str) -> FrozenTarget {
    FrozenTarget::new(schema, identity, label)
}

/// The identity text of a place, which is what a frozen target and an effect both name.
pub fn id_of(object: &SpatialObject) -> &str {
    object.spatial_id().as_str()
}
