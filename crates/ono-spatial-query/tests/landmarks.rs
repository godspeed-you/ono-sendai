//! The built-in landmark rules as a caller sees them — spec v0.4 §26.2, §26.3, §3.7, §2.11.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{NOW, projection, record, scope, socket_with, with};
use jiff::{Span, Timestamp};
use ono_spatial_core::{BootIdentity, LandmarkReason, SpatialScope, SpatialType};
use ono_spatial_query::{LandmarkThresholds, landmarks_of_object};
use ono_value::{RecordValue, Value};

/// Long enough after the fixtures' own timestamps that nothing counts as a recent start.
const LATER: Timestamp = Timestamp::constant(86_400, 0);

/// The reasons the engine found on `record`, at `now`.
fn reasons_at(record: &RecordValue, now: Timestamp) -> Vec<LandmarkReason> {
    reasons_with(record, &LandmarkThresholds::default(), now)
}

fn reasons_with(
    record: &RecordValue,
    thresholds: &LandmarkThresholds,
    now: Timestamp,
) -> Vec<LandmarkReason> {
    landmarks_of_object(&place_of(record), Some(record), thresholds, &scope(), now)
        .iter()
        .map(ono_spatial_core::Landmark::reason)
        .collect()
}

/// The place a fixture record is. `ono.socket/1` needs the caller's type, because the same
/// schema serves a listener and a connection (§14.3, §14.4).
fn place_of(record: &RecordValue) -> ono_spatial_core::SpatialObject {
    place_in(record, projection())
}

fn place_in(
    record: &RecordValue,
    projection: ono_spatial_core::Projection,
) -> ono_spatial_core::SpatialObject {
    let candidates = ono_spatial_core::spatial_types_of(&record.schema().id().to_string());
    let object_type = candidates.first().copied().unwrap_or(SpatialType::Process);
    projection
        .project_as(record, object_type)
        .unwrap_or_else(|error| panic!("the fixture is a place: {}", error.message()))
}

/// A process record with an explicit start time and cpu share.
fn process_at(pid: i64, name: &str, started: Timestamp, cpu: Option<f64>) -> RecordValue {
    let mut fields = vec![
        ("pid", Value::Int(i128::from(pid))),
        ("name", Value::string(name)),
        ("state", Value::string("running")),
        ("started", Value::Timestamp(started)),
    ];
    if let Some(cpu) = cpu {
        fields.push(("cpu", Value::Float(cpu)));
    }
    record("ono.process/1", &fields)
}

#[test]
fn should_promote_a_listener_bound_beyond_loopback_as_a_public_listener() {
    // §26.2 names "public listener" a built-in network rule and §3.7 fixes its reason. The
    // fixture binds `0.0.0.0`, which is reachable from outside the host.
    let listener = socket_with(4242, Some("listen"), None);

    assert!(
        reasons_at(&listener, LATER).contains(&LandmarkReason::PublicListener),
        "spec §26.2/§3.7: a socket listening on 0.0.0.0 is a public listener"
    );
}

#[test]
fn should_not_promote_a_listener_that_only_reaches_this_host() {
    let loopback = with(
        socket_with(4243, Some("listen"), None),
        "local",
        Value::Record(std::sync::Arc::new(record(
            "ono.endpoint/1",
            &[(
                "address",
                Value::Ip("127.0.0.1".parse().expect("a fixture address")),
            )],
        ))),
    );

    assert!(
        !reasons_at(&loopback, LATER).contains(&LandmarkReason::PublicListener),
        "spec §26.3: a loopback listener reaches nobody else, and promoting it would turn a \
         busy host into an alert board"
    );
}

#[test]
fn should_promote_a_process_that_started_inside_the_change_window() {
    // §26.2's "unexpected exit/recent start", spelled with §3.7's `recently_changed`.
    let now = Timestamp::UNIX_EPOCH
        .checked_add(Span::new().hours(10))
        .expect("a fixture instant");
    let fresh = process_at(
        99,
        "sleep",
        now.checked_sub(Span::new().seconds(3)).expect("just now"),
        None,
    );
    let old = process_at(
        98,
        "init",
        now.checked_sub(Span::new().hours(9)).expect("long ago"),
        None,
    );

    assert!(reasons_at(&fresh, now).contains(&LandmarkReason::RecentlyChanged));
    assert!(
        !reasons_at(&old, now).contains(&LandmarkReason::RecentlyChanged),
        "spec §26.3: outside the change window a start is history, not a landmark"
    );
}

#[test]
fn should_follow_the_configured_threshold_when_deciding_that_cpu_is_high() {
    // §26.3: "Thresholds MUST be inspectable and configurable." A rule that ignored the setting
    // would make the setting a decoration.
    let now = Timestamp::UNIX_EPOCH
        .checked_add(Span::new().hours(10))
        .expect("a fixture instant");
    let busy = process_at(101, "compile", Timestamp::UNIX_EPOCH, Some(50.0));

    assert!(
        !reasons_at(&busy, now).contains(&LandmarkReason::HighCpu),
        "50% is below the conservative default of 80%"
    );
    let eager = LandmarkThresholds {
        high_cpu_percent: 40.0,
        ..LandmarkThresholds::default()
    };
    assert!(
        reasons_with(&busy, &eager, now).contains(&LandmarkReason::HighCpu),
        "spec §26.3: the threshold the user configured is the threshold the rule uses"
    );
}

#[test]
fn should_promote_a_failed_service_and_say_why() {
    let failed = common::service("nginx.service", "failed");

    let landmarks = landmarks_of_object(
        &place_of(&failed),
        Some(&failed),
        &LandmarkThresholds::default(),
        &scope(),
        LATER,
    );

    let landmark = landmarks
        .iter()
        .find(|landmark| landmark.reason() == LandmarkReason::Failed)
        .expect("spec §26.2: a failed service is a landmark");
    assert!(
        !landmark.evidence().is_empty(),
        "spec §3.7: a landmark always exposes its reason, and §26.3 the fact behind it"
    );
}

#[test]
fn should_not_promote_an_object_only_because_it_is_privileged() {
    // §26.2 asks for the privileged rule "when context makes it relevant" and §26.3 forbids
    // turning every busy system into an alert board. A root-owned process that is otherwise
    // ordinary is ordinary.
    let root_owned = with(
        process_at(1, "systemd", Timestamp::UNIX_EPOCH, None),
        "user",
        Value::Record(std::sync::Arc::new(record(
            "ono.user/1",
            &[("uid", Value::Int(0)), ("name", Value::string("root"))],
        ))),
    );

    assert!(
        reasons_at(&root_owned, LATER).is_empty(),
        "spec §26.3: privilege alone is not an incident"
    );

    let root_listener = with(
        socket_with(4244, Some("listen"), None),
        "user",
        Value::Int(0),
    );
    let reasons = reasons_at(&root_listener, LATER);
    assert!(
        reasons.contains(&LandmarkReason::PublicListener)
            && reasons.contains(&LandmarkReason::Privileged),
        "spec §26.2: privilege is what makes an already-promoted object worth a second look, \
         got {reasons:?}"
    );
}

#[test]
fn should_say_nothing_at_all_when_the_landmark_engine_is_switched_off() {
    // §47's `spatial.landmarks.enabled`.
    let listener = socket_with(4245, Some("listen"), None);
    let off = LandmarkThresholds {
        enabled: false,
        ..LandmarkThresholds::default()
    };

    assert!(reasons_with(&listener, &off, LATER).is_empty());
}

#[test]
fn should_mark_an_object_from_another_host_as_a_remote_boundary() {
    // §26.2's "cross-host boundary"; §2.18 requires the crossing to be apparent.
    let listener = socket_with(4246, Some("listen"), None);
    let remote = SpatialScope::remote_host("web01", BootIdentity::new("web01", "boot-b"));
    let object = place_in(&listener, ono_spatial_core::Projection::new(remote, NOW));

    let reasons: Vec<LandmarkReason> = landmarks_of_object(
        &object,
        Some(&listener),
        &LandmarkThresholds::default(),
        &scope(),
        LATER,
    )
    .iter()
    .map(ono_spatial_core::Landmark::reason)
    .collect();

    assert!(
        reasons.contains(&LandmarkReason::RemoteBoundary),
        "spec §26.2/§3.7: an object in another host's scope is a remote boundary, got {reasons:?}"
    );
}

#[test]
fn should_not_promote_a_read_only_filesystem_that_is_full_as_storage_pressure() {
    // §26.2's storage rule is "filesystem near capacity", and §2.11 makes a landmark a reason to
    // look. A squashfs image is 100% used by construction — that is what a read-only image is —
    // so promoting one is not a warning, it is twenty of them on an ordinary host with snaps
    // mounted. Nothing can be written to it, so nothing can fill it.
    let full = record(
        "ono.filesystem/1",
        &[
            ("source", Value::string("/var/lib/snapd/snaps/core22.snap")),
            ("type", Value::string("squashfs")),
            ("target", Value::string("/snap/core22/1")),
            ("size", Value::Int(77_000_000)),
            ("used", Value::Int(77_000_000)),
            ("available", Value::Int(0)),
            ("read_only", Value::Bool(true)),
        ],
    );
    assert_eq!(
        reasons_at(&full, LATER),
        Vec::<LandmarkReason>::new(),
        "§26.2: a filesystem nothing can write to cannot be under storage pressure"
    );
}

#[test]
fn should_still_promote_a_writable_filesystem_above_the_threshold() {
    let full = record(
        "ono.filesystem/1",
        &[
            ("source", Value::string("/dev/nvme0n1p2")),
            ("type", Value::string("ext4")),
            ("target", Value::string("/")),
            ("size", Value::Int(100_000_000)),
            ("used", Value::Int(97_000_000)),
            ("available", Value::Int(3_000_000)),
            ("read_only", Value::Bool(false)),
        ],
    );
    assert_eq!(
        reasons_at(&full, LATER),
        vec![LandmarkReason::StoragePressure],
        "§26.2: the rule the read-only guard must not take with it"
    );
}

#[test]
fn should_still_promote_a_full_filesystem_that_does_not_say_whether_it_is_writable() {
    // §35.3: unknown is null, and null is not a claim that the filesystem is read-only. A
    // provider that does not answer the question leaves the rule as it was.
    let full = record(
        "ono.filesystem/1",
        &[
            ("source", Value::string("/dev/sdb1")),
            ("type", Value::string("ext4")),
            ("target", Value::string("/data")),
            ("size", Value::Int(100_000_000)),
            ("used", Value::Int(99_000_000)),
            ("available", Value::Int(1_000_000)),
        ],
    );
    assert_eq!(
        reasons_at(&full, LATER),
        vec![LandmarkReason::StoragePressure]
    );
}
