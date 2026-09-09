//! The §46 records the views read, built as records rather than encoded from a domain type.
//!
//! `ono-change-render` sits below the crate that owns the change vocabulary and cannot see a
//! `ChangePlan`, so its fixtures are `ono.change-plan/1`, `ono.recovery-plan/1` and
//! `ono.recovery-asset/1` records assembled by hand — which is also what a test of a renderer
//! should be: the wire shape, and nothing that could quietly agree with the renderer because both
//! came from the same encoder.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_value::{
    ByteSize, Duration, FieldDef, FieldType, MapValue, Provenance, RecordValue, Schema, SchemaId,
    Value,
};

/// A fixed instant, because §50 makes rendering deterministic and a test may not read a clock.
#[must_use]
pub fn instant() -> Timestamp {
    Timestamp::UNIX_EPOCH
}

/// `now`, `seconds` after the fixed instant.
#[must_use]
pub fn later(seconds: i64) -> Timestamp {
    Timestamp::from_second(instant().as_second() + seconds).expect("a representable instant")
}

/// A string value.
#[must_use]
pub fn s(text: &str) -> Value {
    Value::string(text)
}

/// An integer value.
#[must_use]
pub fn i(number: usize) -> Value {
    Value::Int(i128::try_from(number).expect("a representable count"))
}

/// A duration of `seconds`.
#[must_use]
pub fn seconds(seconds: i64) -> Value {
    Value::Duration(Duration::from_nanoseconds(
        i128::from(seconds) * 1_000_000_000,
    ))
}

/// A list of string values.
#[must_use]
pub fn list(values: &[&str]) -> Value {
    Value::list(values.iter().map(|value| s(value)))
}

/// An anonymous map, as §46's nested `record` fields carry one.
#[must_use]
pub fn map(fields: &[(&str, Value)]) -> Value {
    let mut map = MapValue::new();
    for (name, value) in fields {
        map.insert(Arc::from(*name), value.clone());
    }
    Value::Map(Arc::new(map))
}

/// A record of `schema_id` carrying exactly `fields`, in the order given.
#[must_use]
pub fn record(schema_id: &str, fields: &[(&str, Value)]) -> RecordValue {
    let mut builder = Schema::builder(SchemaId::new(schema_id, 1), schema_id);
    for (name, _) in fields {
        builder = builder.field(FieldDef::new(name, FieldType::Any));
    }
    let schema = Arc::new(builder.build().expect("a well-formed schema"));
    let mut record = RecordValue::builder(
        schema,
        Provenance::local("test", SchemaId::new(schema_id, 1)),
    );
    for (name, value) in fields {
        record = record
            .set(name, value.clone())
            .unwrap_or_else(|error| panic!("the fixture sets {name}: {error:?}"));
    }
    record.build()
}

/// The same record as a list element.
#[must_use]
pub fn nested(schema_id: &str, fields: &[(&str, Value)]) -> Value {
    Value::Record(Arc::new(record(schema_id, fields)))
}

// -------------------------------------------------------------------------------------------
// §64's nginx plan
// -------------------------------------------------------------------------------------------

/// One `ono.plan-action/1`.
#[must_use]
pub fn action(
    ordinal: usize,
    role: &str,
    summary: &str,
    target: Option<&str>,
    status: &str,
    effects: Value,
) -> Value {
    nested(
        "ono.plan-action",
        &[
            ("id", s(&format!("action/{ordinal:04x}"))),
            ("ordinal", i(ordinal)),
            ("role", s(role)),
            ("summary", s(summary)),
            ("target", target.map_or(Value::Null, s)),
            ("idempotency", s("idempotent")),
            ("proposed_effects", effects),
            ("status", s(status)),
        ],
    )
}

/// One `ono.proposed-effect/1`.
#[must_use]
pub fn effect(object: &str, domain: &str, kind: &str, irreversible: bool) -> Value {
    nested(
        "ono.proposed-effect",
        &[
            ("id", s(&format!("effect/{object}"))),
            ("object", s(object)),
            ("domain", s(domain)),
            ("kind", s(kind)),
            ("confidence", s("expected")),
            ("explanation", s("the provider declared this effect")),
            ("irreversible", Value::Bool(irreversible)),
        ],
    )
}

/// §10.3's matrix for a plan that is PROTECTED and still excludes real things (Appendix A.6).
#[must_use]
pub fn protected_rows() -> Value {
    Value::list([
        nested(
            "ono.protection-coverage",
            &[
                ("domain", s("filesystem-persistent")),
                ("objective", s("preserve-exact")),
                ("protection", s("protected")),
                ("satisfied", Value::Bool(true)),
                ("required", Value::Bool(true)),
                ("declared_irrelevant", Value::Bool(false)),
                ("consistency", s("filesystem-consistent")),
                (
                    "exclusions",
                    Value::list([map(&[
                        ("domain", s("filesystem-persistent")),
                        ("subject", s("/home")),
                        ("reason", s("a separate dataset")),
                        ("irreversible", Value::Bool(false)),
                    ])]),
                ),
                ("note", s("covered by the ZFS recovery point")),
            ],
        ),
        nested(
            "ono.protection-coverage",
            &[
                ("domain", s("process-runtime")),
                ("objective", s("restore-semantic")),
                ("protection", s("compensatable")),
                ("satisfied", Value::Bool(true)),
                ("required", Value::Bool(true)),
                ("declared_irrelevant", Value::Bool(false)),
                ("consistency", Value::Null),
                ("exclusions", Value::list([])),
                ("note", s("the service can be restarted")),
            ],
        ),
    ])
}

/// The plan-level exclusions §10.3 requires beside the level, two of them irreversible.
#[must_use]
pub fn protected_exclusions() -> Value {
    Value::list([
        map(&[
            ("domain", s("filesystem-persistent")),
            ("subject", s("/home")),
            ("reason", s("a separate dataset")),
            ("irreversible", Value::Bool(false)),
        ]),
        map(&[
            ("domain", s("process-runtime")),
            ("subject", s("process memory")),
            ("reason", s("process identity cannot be captured")),
            ("irreversible", Value::Bool(false)),
        ]),
        map(&[
            ("domain", s("network-runtime")),
            ("subject", s("active TCP sessions")),
            ("reason", s("a live session cannot be re-established")),
            ("irreversible", Value::Bool(true)),
        ]),
        map(&[
            ("domain", s("external-side-effect")),
            ("subject", s("requests already served externally")),
            ("reason", s("the response has left the machine")),
            ("irreversible", Value::Bool(true)),
        ]),
    ])
}

/// A matrix whose required persistent domain nothing covers (§10.2's UNPROTECTED).
#[must_use]
pub fn unprotected_rows() -> Value {
    Value::list([nested(
        "ono.protection-coverage",
        &[
            ("domain", s("filesystem-persistent")),
            ("objective", s("preserve-exact")),
            ("protection", s("unprotected")),
            ("satisfied", Value::Bool(false)),
            ("required", Value::Bool(true)),
            ("declared_irrelevant", Value::Bool(false)),
            ("consistency", Value::Null),
            ("exclusions", Value::list([])),
            ("note", s("no provider offered a recovery asset")),
        ],
    )])
}

/// §9.5's summary of §64's impact graph.
#[must_use]
pub fn nginx_summary() -> Value {
    map(&[
        ("direct_targets", i(2)),
        ("direct_effects", i(0)),
        ("dependents", i(4)),
        ("transitive", i(1)),
        ("external", i(1)),
        ("boundaries", i(1)),
        ("hosts", i(0)),
        ("complete", Value::Bool(true)),
        ("truncated_reason", Value::Null),
    ])
}

/// §64's nginx plan as `ono.change-plan/1`, in whatever `state` a test needs.
#[must_use]
pub fn nginx_plan_in(state: &str, statuses: &[&str]) -> RecordValue {
    let status = |index: usize| *statuses.get(index).unwrap_or(&"pending");
    record(
        "ono.change-plan",
        &[
            ("id", s("a82f1c0d9e4b7a63")),
            ("revision", i(3)),
            ("kind", s("change")),
            ("state", s(state)),
            (
                "intent",
                s("replace nginx configuration and restart service"),
            ),
            ("source", s("plan { replace file /etc/nginx/nginx.conf }")),
            ("session", s("session-1")),
            ("created_at", Value::Timestamp(instant())),
            (
                "targets",
                Value::list([
                    map(&[
                        ("schema", s("ono.file/1")),
                        ("identity", s("/etc/nginx/nginx.conf")),
                        ("label", s("/etc/nginx/nginx.conf")),
                    ]),
                    map(&[
                        ("schema", s("ono.service/1")),
                        ("identity", s("nginx.service")),
                        ("label", s("nginx.service")),
                    ]),
                ]),
            ),
            (
                "actions",
                Value::list([
                    action(
                        1,
                        "prepare",
                        "snapshot rpool/ROOT/debian",
                        None,
                        status(0),
                        Value::list([]),
                    ),
                    action(
                        2,
                        "mutate",
                        "replace nginx.conf",
                        Some("/etc/nginx/nginx.conf"),
                        status(1),
                        Value::list([effect(
                            "/etc/nginx/nginx.conf",
                            "filesystem-persistent",
                            "modify",
                            false,
                        )]),
                    ),
                    action(
                        3,
                        "mutate",
                        "validate nginx configuration",
                        None,
                        status(2),
                        Value::list([]),
                    ),
                    action(
                        4,
                        "mutate",
                        "restart nginx.service",
                        Some("nginx.service"),
                        status(3),
                        Value::list([effect(
                            "active TCP sessions",
                            "network-runtime",
                            "interrupt",
                            true,
                        )]),
                    ),
                    action(
                        5,
                        "verify",
                        "verify service running",
                        None,
                        status(4),
                        Value::list([]),
                    ),
                ]),
            ),
            (
                "effects",
                Value::list([
                    effect(
                        "/etc/nginx/nginx.conf",
                        "filesystem-persistent",
                        "modify",
                        false,
                    ),
                    effect("active TCP sessions", "network-runtime", "interrupt", true),
                ]),
            ),
            ("impact_summary", nginx_summary()),
            ("protection", protected_rows()),
            ("protection_level", s("protected")),
            ("protection_mode", s("prefer")),
            ("coverage_exclusions", protected_exclusions()),
            ("risk", s("moderate")),
            (
                "risk_findings",
                Value::list([map(&[
                    ("dimension", s("downtime")),
                    ("class", s("moderate")),
                    ("rule", s("rule.service.restart")),
                    (
                        "reason",
                        s("restarting nginx interrupts the connections it is serving"),
                    ),
                ])]),
            ),
            ("strategy", s("sequential")),
            (
                "verification_contracts",
                Value::list([
                    contract("required", "nginx.service", "== running"),
                    contract("required", "socket :443", "exists"),
                    contract("advisory", "worker count", "== 4"),
                ]),
            ),
            ("accepted_risk_overrides", Value::list([])),
            ("requires_privilege", Value::Bool(true)),
            ("digest", s("f0f1f2f3f4f5f6f7")),
        ],
    )
}

/// One verification contract as `ono.change-plan/1` carries it.
#[must_use]
pub fn contract(class: &str, subject: &str, expression: &str) -> Value {
    map(&[
        ("id", s(&format!("check/{subject}"))),
        ("class", s(class)),
        ("subject", s(subject)),
        ("expression", s(expression)),
        ("equivalence_domain", Value::Null),
    ])
}

/// §64's plan, sealed and unapplied.
#[must_use]
pub fn sealed_nginx_plan() -> RecordValue {
    nginx_plan_in("sealed", &[])
}

/// §64's plan while it is still a draft, with nothing resolved beyond its intent.
#[must_use]
pub fn draft_nginx_plan() -> RecordValue {
    nginx_plan_in("draft", &[])
}

/// A plan that failed at its fourth action, with one action never executed (Appendix E.5).
#[must_use]
pub fn failed_plan() -> RecordValue {
    nginx_plan_in(
        "apply-failed",
        &["succeeded", "succeeded", "succeeded", "failed", "pending"],
    )
}

/// A plan carrying nothing but an intent, for the sections that must still answer (§10.5).
#[must_use]
pub fn empty_plan() -> RecordValue {
    record(
        "ono.change-plan",
        &[
            ("id", s("0f0e0d0c0b0a0908")),
            ("revision", i(1)),
            ("state", s("draft")),
            ("intent", s("do nothing yet")),
        ],
    )
}

/// §9's impact graph for the nginx plan, with §9.6's boundary on the end of it.
#[must_use]
pub fn nginx_impact() -> RecordValue {
    impact_graph(true, Value::Null)
}

/// The same graph, cut short by a traversal budget (§9.5, §52.2).
#[must_use]
pub fn bounded_impact() -> RecordValue {
    impact_graph(false, s("the interactive budget was reached"))
}

fn impact_graph(complete: bool, reason: Value) -> RecordValue {
    let node = |id: &str, label: &str, class: &str, relation: Option<&str>| {
        map(&[
            ("id", s(id)),
            ("label", s(label)),
            ("object_type", s("ono.process/1")),
            ("class", s(class)),
            ("depth", i(1)),
            ("relation", relation.map_or(Value::Null, s)),
            ("confidence", s("exact")),
            ("host", Value::Null),
        ])
    };
    record(
        "ono.impact-graph",
        &[
            ("plan_id", s("a82f1c0d9e4b7a63")),
            (
                "nodes",
                Value::list([
                    node("/etc/nginx/nginx.conf", "nginx.conf", "direct-target", None),
                    node("nginx.service", "nginx.service", "direct-target", None),
                    node(
                        "worker/1",
                        "worker 1",
                        "dependent",
                        Some("service.controls_process"),
                    ),
                    node(
                        "worker/2",
                        "worker 2",
                        "dependent",
                        Some("service.controls_process"),
                    ),
                    node(
                        "worker/3",
                        "worker 3",
                        "dependent",
                        Some("service.controls_process"),
                    ),
                    node(
                        "worker/4",
                        "worker 4",
                        "dependent",
                        Some("service.controls_process"),
                    ),
                    node(
                        ":443",
                        ":443",
                        "transitive-related",
                        Some("process.listens_on"),
                    ),
                    node(
                        "sessions",
                        "14 active client connections",
                        "external-side-effect",
                        None,
                    ),
                ]),
            ),
            (
                "boundaries",
                Value::list([map(&[
                    ("at", s("nginx")),
                    ("beyond", s("external API")),
                    (
                        "reason",
                        s("the outbound HTTPS request leaves this machine"),
                    ),
                ])]),
            ),
            ("direct_targets", i(2)),
            ("direct_effects", i(0)),
            ("dependents", i(4)),
            ("transitive", i(1)),
            ("external", i(1)),
            ("boundary_count", i(1)),
            ("hosts", i(0)),
            ("complete", Value::Bool(complete)),
            ("truncated_reason", reason),
        ],
    )
}

/// An impact graph that reached nothing at all.
#[must_use]
pub fn empty_impact() -> RecordValue {
    record(
        "ono.impact-graph",
        &[
            ("plan_id", s("a82f1c0d9e4b7a63")),
            ("nodes", Value::list([])),
            ("boundaries", Value::list([])),
            ("direct_targets", i(0)),
            ("dependents", i(0)),
            ("transitive", i(0)),
            ("external", i(0)),
            ("boundary_count", i(0)),
            ("hosts", i(0)),
            ("complete", Value::Bool(true)),
            ("truncated_reason", Value::Null),
        ],
    )
}

// -------------------------------------------------------------------------------------------
// §13.8's recovery asset
// -------------------------------------------------------------------------------------------

/// §13.8's ZFS recovery point, proposed rather than created (§2.1), with an estimated size.
#[must_use]
pub fn zfs_asset() -> RecordValue {
    asset_record(
        "b8174e2a9c3d5f60",
        "proposed",
        Value::Null,
        Some(ByteSize::from_bytes(327_155_712)),
        true,
        Value::Null,
        false,
    )
}

/// The same asset, created, validated, with an exact size and a fixed expiry (§11.4, §37.5).
#[must_use]
pub fn ready_asset() -> RecordValue {
    asset_record(
        "b8174e2a9c3d5f60",
        "ready",
        map(&[
            ("exists", Value::Bool(true)),
            ("identity_matches", Value::Bool(true)),
            ("scope_matches", Value::Bool(true)),
            ("restore_available", Value::Bool(true)),
            ("permissions_present", Value::Bool(true)),
            ("at", Value::Timestamp(instant())),
            (
                "detail",
                s("the snapshot exists and a restore path was confirmed"),
            ),
        ]),
        Some(ByteSize::from_bytes(327_155_712)),
        false,
        Value::Timestamp(later(24 * 3_600)),
        false,
    )
}

/// The same asset under an explicit hold, which §37.2 keeps out of automatic removal.
#[must_use]
pub fn held_asset() -> RecordValue {
    asset_record(
        "b8174e2a9c3d5f60",
        "ready",
        Value::Null,
        Some(ByteSize::from_bytes(327_155_712)),
        false,
        Value::Timestamp(later(24 * 3_600)),
        true,
    )
}

/// The same asset, already past its retention window (§37.1).
#[must_use]
pub fn expired_asset() -> RecordValue {
    asset_record(
        "b8174e2a9c3d5f60",
        "ready",
        Value::Null,
        Some(ByteSize::from_bytes(327_155_712)),
        false,
        Value::Timestamp(later(60)),
        false,
    )
}

/// An asset whose size nobody measured — v0.2 §35.3's unknown, never a zero.
#[must_use]
pub fn unmeasured_asset() -> RecordValue {
    record(
        "ono.recovery-asset",
        &[
            ("id", s("0b21ee7714a3c5d9")),
            ("provider", s("ono.recovery.btrfs")),
            ("type", s("btrfs-snapshot")),
            ("reference", s("@root.ono-91aa")),
            ("host", s("local")),
            (
                "scope",
                map(&[
                    ("domain", s("@root")),
                    ("domain_kind", s("btrfs-subvolume")),
                    ("covers", Value::list([])),
                    ("host", s("local")),
                ]),
            ),
            ("created_at", Value::Timestamp(instant())),
            ("source_plan", Value::Null),
            ("state", s("proposed")),
            ("consistency", s("unknown")),
            ("restore_method", s("subvolume-replacement")),
            ("validation", Value::Null),
            ("retention", seconds(24 * 3_600)),
            ("expires_at", Value::Null),
            ("held", Value::Bool(false)),
            ("initial_size", Value::Null),
            ("retained_size", Value::Null),
            ("size_estimated", Value::Bool(true)),
            ("requires_reboot", Value::Bool(false)),
            ("requires_offline", Value::Bool(false)),
            ("shares_failure_domain", Value::Bool(true)),
            ("exclusions", Value::list([])),
        ],
    )
}

/// An asset whose restore needs a reboot (§13.7, §19.1).
#[must_use]
pub fn rebooting_asset() -> RecordValue {
    let mut fields = asset_fields(
        "b8174e2a9c3d5f60",
        "ready",
        Value::Null,
        Some(ByteSize::from_bytes(1)),
        false,
        Value::Null,
        false,
    );
    for field in &mut fields {
        if field.0 == "requires_reboot" {
            field.1 = Value::Bool(true);
        }
    }
    record("ono.recovery-asset", &fields)
}

fn asset_record(
    id: &str,
    state: &str,
    validation: Value,
    size: Option<ByteSize>,
    estimated: bool,
    expires: Value,
    held: bool,
) -> RecordValue {
    record(
        "ono.recovery-asset",
        &asset_fields(id, state, validation, size, estimated, expires, held),
    )
}

fn asset_fields(
    id: &str,
    state: &str,
    validation: Value,
    size: Option<ByteSize>,
    estimated: bool,
    expires: Value,
    held: bool,
) -> Vec<(&'static str, Value)> {
    let size = size.map_or(Value::Null, Value::ByteSize);
    vec![
        ("id", s(id)),
        ("provider", s("ono.recovery.zfs")),
        ("type", s("zfs-snapshot")),
        ("reference", s("rpool/ROOT/debian@ono-a82f")),
        ("host", s("local")),
        (
            "scope",
            map(&[
                ("domain", s("rpool/ROOT/debian")),
                ("domain_kind", s("zfs-dataset")),
                ("covers", list(&["/etc/nginx/nginx.conf"])),
                ("host", s("local")),
            ]),
        ),
        ("created_at", Value::Timestamp(instant())),
        ("source_plan", s("a82f1c0d9e4b7a63")),
        ("state", s(state)),
        ("consistency", s("filesystem-consistent")),
        ("restore_method", s("selective-file-restore")),
        ("validation", validation),
        ("retention", seconds(24 * 3_600)),
        ("expires_at", expires),
        ("held", Value::Bool(held)),
        ("initial_size", size.clone()),
        ("retained_size", size),
        ("size_estimated", Value::Bool(estimated)),
        ("requires_reboot", Value::Bool(false)),
        ("requires_offline", Value::Bool(false)),
        ("shares_failure_domain", Value::Bool(true)),
        (
            "exclusions",
            Value::list([map(&[
                ("subject", s("/home")),
                ("reason", s("a separate dataset")),
            ])]),
        ),
    ]
}

// -------------------------------------------------------------------------------------------
// §24's recovery plans
// -------------------------------------------------------------------------------------------

/// §24.4's selective recovery: two newer files, both left alone.
#[must_use]
pub fn selective_recovery() -> RecordValue {
    record(
        "ono.recovery-plan",
        &[
            ("id", s("r91c4d2e8b6a3f150")),
            ("source_plan", s("a82f1c0d9e4b7a63")),
            ("source_assets", list(&["b8174e2a9c3d5f60"])),
            ("goal", s("restore-changed-objects")),
            ("method", s("selective-file-restore")),
            ("target_state", s("rpool/ROOT/debian@ono-a82f")),
            ("restores", list(&["/etc/nginx/nginx.conf"])),
            (
                "newer_state",
                Value::list([
                    map(&[
                        ("object", s("/etc/ssh/sshd_config")),
                        ("class", s("preserved-by-method")),
                        (
                            "detail",
                            s("changed after the plan, outside the restore set"),
                        ),
                    ]),
                    map(&[
                        ("object", s("/etc/hosts")),
                        ("class", s("preserved-by-method")),
                        (
                            "detail",
                            s("changed after the plan, outside the restore set"),
                        ),
                    ]),
                ]),
            ),
            ("newer_state_analysed", Value::Bool(true)),
            ("destroyed_assets", Value::list([])),
            ("discarded_size", Value::Null),
            (
                "unrecoverable_effects",
                Value::list([map(&[
                    ("subject", s("TCP sessions")),
                    ("domain", s("network-runtime")),
                    ("reason", s("a live session cannot be re-established")),
                    ("compensation", s("clients reconnect")),
                ])]),
            ),
            (
                "metadata_restored",
                list(&["content", "mode", "owner/group"]),
            ),
            (
                "metadata_gaps",
                list(&[
                    "ACLs",
                    "extended attributes",
                    "file capabilities",
                    "SELinux labels",
                    "hard-link relationships",
                ]),
            ),
            ("directory_policy", s("keep-extra-files")),
            ("risk", s("moderate")),
            ("requires_reboot", Value::Bool(false)),
            ("requires_offline", Value::Bool(false)),
            ("requires_acceptance", Value::Bool(false)),
            ("state", s("recovery-planned")),
        ],
    )
}

/// §24.5's full rollback: newer snapshots destroyed and live data discarded.
#[must_use]
pub fn rollback_recovery() -> RecordValue {
    record(
        "ono.recovery-plan",
        &[
            ("id", s("r91d5e3f9c7b4a261")),
            ("source_plan", Value::Null),
            ("source_assets", Value::list([])),
            ("goal", s("restore-domain")),
            ("method", s("dataset-rollback")),
            ("target_state", s("tank/data@ono-91aa")),
            ("restores", list(&["tank/data"])),
            (
                "newer_state",
                Value::list([map(&[
                    ("object", s("tank/data")),
                    ("class", s("discarded-by-method")),
                    ("detail", s("everything written since the snapshot")),
                ])]),
            ),
            ("newer_state_analysed", Value::Bool(true)),
            (
                "destroyed_assets",
                list(&["tank/data@later-1", "tank/data@later-2"]),
            ),
            (
                "discarded_size",
                Value::ByteSize(ByteSize::from_bytes(19_327_352_832)),
            ),
            ("unrecoverable_effects", Value::list([])),
            ("metadata_restored", Value::list([])),
            ("metadata_gaps", list(&["SELinux labels"])),
            ("directory_policy", s("exact-tree")),
            ("risk", s("high")),
            ("requires_reboot", Value::Bool(false)),
            ("requires_offline", Value::Bool(true)),
            ("requires_acceptance", Value::Bool(true)),
            ("state", s("recovery-planned")),
        ],
    )
}

/// A recovery nobody analysed for drift, which §62.8 forbids rendering as safe.
#[must_use]
pub fn unanalysed_recovery() -> RecordValue {
    record(
        "ono.recovery-plan",
        &[
            ("id", s("r7a2b8c4d0e6f9315")),
            ("source_plan", Value::Null),
            ("source_assets", Value::list([])),
            ("goal", s("restore-changed-objects")),
            ("method", s("selective-file-restore")),
            ("target_state", s("rpool/ROOT/debian@ono-a82f")),
            ("restores", list(&["/etc/nginx/nginx.conf"])),
            ("newer_state", Value::list([])),
            ("newer_state_analysed", Value::Bool(false)),
            ("destroyed_assets", Value::list([])),
            ("discarded_size", Value::Null),
            ("unrecoverable_effects", Value::list([])),
            ("metadata_restored", Value::list([])),
            ("metadata_gaps", Value::list([])),
            ("directory_policy", s("keep-extra-files")),
            ("risk", s("moderate")),
            ("requires_reboot", Value::Bool(false)),
            ("requires_offline", Value::Bool(false)),
            ("requires_acceptance", Value::Bool(true)),
            ("state", s("recovery-planned")),
        ],
    )
}

// -------------------------------------------------------------------------------------------
// §23 and §25's verification results
// -------------------------------------------------------------------------------------------

/// One `ono.change-verification/1`.
#[must_use]
pub fn result(class: &str, subject: &str, expression: &str, status: &str) -> RecordValue {
    record(
        "ono.change-verification",
        &[
            ("plan_id", s("a82f1c0d9e4b7a63")),
            ("check_id", s(&format!("check/{subject}"))),
            ("class", s(class)),
            ("subject", s(subject)),
            ("expression", s(expression)),
            ("status", s(status)),
            ("equivalence_domain", Value::Null),
            ("timestamp", Value::Timestamp(instant())),
        ],
    )
}

/// The results §23.4's example prints.
#[must_use]
pub fn nginx_results() -> Vec<RecordValue> {
    vec![
        result("required", "nginx.service", "== running", "passed"),
        result("required", "socket :443", "exists", "passed"),
        result("advisory", "worker count", "== 4", "passed"),
    ]
}

/// One recovery verification observation, in the equivalence domain §25.1 puts it in.
#[must_use]
pub fn equivalence(domain: &str, subject: &str, state: &str) -> RecordValue {
    record(
        "ono.change-verification",
        &[
            ("plan_id", s("r91c4d2e8b6a3f150")),
            ("check_id", s(&format!("check/{subject}"))),
            ("class", s("required")),
            ("subject", s(subject)),
            ("expression", s("")),
            ("status", s("passed")),
            ("equivalence_domain", s(domain)),
            ("equivalence_state", s(state)),
            ("timestamp", Value::Timestamp(instant())),
        ],
    )
}

/// §25.2's worked recovery verification.
#[must_use]
pub fn recovery_results() -> Vec<RecordValue> {
    vec![
        equivalence("persistent-state", "nginx.conf", "restored"),
        equivalence("persistent-state", "package version", "restored"),
        equivalence("runtime-state", "service state", "restored"),
        equivalence("runtime-state", "worker PIDs", "different-as-expected"),
        equivalence("runtime-state", "TCP connections", "not-recoverable"),
        equivalence(
            "external-side-effect",
            "1 webhook request",
            "not-recoverable",
        ),
    ]
}

// -------------------------------------------------------------------------------------------
// Appendix E.2's long plan
// -------------------------------------------------------------------------------------------

/// A canary rollout of eighty-three actions, which Appendix E.2 collapses.
#[must_use]
pub fn long_plan() -> RecordValue {
    let mut actions = Vec::new();
    let mut ordinal = 0usize;
    for (count, role, summary) in [
        (20usize, "prepare", "recovery assets"),
        (20, "mutate", "update config"),
        (20, "mutate", "restart service"),
        (23, "verify", "service/listener checks"),
    ] {
        for _ in 0..count {
            ordinal += 1;
            actions.push(action(
                ordinal,
                role,
                summary,
                None,
                "pending",
                Value::list([]),
            ));
        }
    }
    record(
        "ono.change-plan",
        &[
            ("id", s("d46c9a1b3e5f7082")),
            ("revision", i(1)),
            ("state", s("sealed")),
            ("intent", s("roll the config out to every api host")),
            ("targets", Value::list([])),
            ("actions", Value::list(actions)),
            ("effects", Value::list([])),
            ("impact_summary", nginx_summary()),
            ("protection", protected_rows()),
            ("protection_level", s("protected")),
            ("coverage_exclusions", protected_exclusions()),
            ("risk", s("high")),
            ("risk_findings", Value::list([])),
            ("strategy", s("canary 1, then batch 3")),
            (
                "verification_contracts",
                Value::list([contract("required", "nginx.service", "== running")]),
            ),
            ("accepted_risk_overrides", Value::list([])),
        ],
    )
}

// -------------------------------------------------------------------------------------------
// Reading a rendered view
// -------------------------------------------------------------------------------------------

/// The headings of a rendered view: the lines that start at column zero and carry text.
#[must_use]
pub fn headings(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .filter(|line| !line.is_empty() && !line.starts_with(' '))
        .cloned()
        .collect()
}

/// Where `needle` first appears in `lines`, as a whole line or inside one.
#[must_use]
pub fn index_of(lines: &[String], needle: &str) -> Option<usize> {
    lines.iter().position(|line| line.contains(needle))
}

/// Whether any line contains `needle`.
#[must_use]
pub fn contains(lines: &[String], needle: &str) -> bool {
    index_of(lines, needle).is_some()
}

/// A plan carrying exactly a protection matrix, its level and its exclusions (§10.3).
pub fn plan_with(level: &str, rows: Value, exclusions: Value) -> RecordValue {
    record(
        "ono.change-plan",
        &[
            ("id", s("a82f1c0d9e4b7a63")),
            ("state", s("sealed")),
            ("intent", s("replace nginx configuration")),
            ("protection", rows),
            ("protection_level", s(level)),
            ("coverage_exclusions", exclusions),
        ],
    )
}
