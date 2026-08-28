//! The provider conformance rules of spec v0.4 §42, and the permission honesty of §35.2.
//!
//! §42 requires four things of any provider that exposes objects to spatial navigation, and
//! these are them as executable rules:
//!
//! - **§42.1 identity** — repeated observations of one live object resolve to one `SpatialId`,
//!   and two objects never share one;
//! - **§42.2 reuse safety** — a reused pid cannot silently resolve a tombstoned place to a
//!   different object;
//! - **§42.3 relation integrity** — covered in `relations.rs`: no edge reaches an id the index
//!   does not hold;
//! - **§42.4 permission** — denied information produces `permission_denied` or `unknown`, never a
//!   false empty collection.
//!
//! §35.2 is the same rule one layer up: the six states must stay distinct all the way from the
//! provider's record to the neighborhood group a place view renders.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{NOW, bridge, index, record};
use jiff::{Span, Timestamp};
use ono_core::ErrorCode;
use ono_spatial_core::{
    Liveness, PermissionState, SpatialType, Tombstone, TombstoneRegistry, relation,
};
use ono_value::{ErrorValue, Value};

fn process(pid: i64, started: &str, extra: &[(&str, Value)]) -> ono_value::RecordValue {
    let mut fields = vec![
        ("pid", Value::Int(i128::from(pid))),
        ("name", Value::string("nginx")),
        ("state", Value::string("running")),
        ("started", Value::string(started)),
        ("pid_namespace", Value::Int(4_026_531_836)),
    ];
    fields.extend(extra.iter().cloned());
    record("ono.process/1", &fields)
}

fn denied(what: &str) -> Value {
    ErrorValue::new(
        ErrorCode::IoPermissionDenied,
        format!("permission denied reading {what}"),
    )
    .into_value()
}

#[test]
fn should_resolve_repeated_observations_of_one_object_to_the_same_identity() {
    // §42.1's identity test, on the seam a provider actually reaches the index through.
    let mut index = index();
    let mut bridge = bridge();
    let first = bridge.absorb(
        &mut index,
        &[process(1842, "2026-08-10T06:12:00Z", &[])],
        NOW,
    );
    let again = bridge.absorb(
        &mut index,
        &[process(1842, "2026-08-10T06:12:00Z", &[])],
        NOW + Span::new().seconds(5),
    );

    let id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    assert!(first.added().contains(&id));
    assert!(
        again.reconciled().contains(&id),
        "the second observation is the same place, five seconds older"
    );
    assert_eq!(index.of_type(SpatialType::Process).len(), 1);
}

#[test]
fn should_never_give_two_different_objects_one_identity() {
    // The other half of §42.1. Two processes that differ only in start time are the pid-reuse
    // case, and they are two places.
    let mut index = index();
    let mut bridge = bridge();
    let outcome = bridge.absorb(
        &mut index,
        &[
            process(1842, "2026-08-10T06:12:00Z", &[]),
            process(1842, "2026-08-11T09:03:00Z", &[]),
        ],
        NOW,
    );

    assert!(outcome.refused().is_empty(), "{:?}", outcome.refused());
    assert_eq!(
        index.of_type(SpatialType::Process).len(),
        2,
        "a reused pid is a different object and must not collapse into the old place (§42.2)"
    );
}

#[test]
fn should_keep_a_containers_pid_one_apart_from_the_hosts() {
    // §10.2's own reason for the fourth identity part: "pid namespace identity". Two inits with
    // pid 1 and the same start time are the same object only if nothing distinguishes them.
    let init = |namespace: i64| {
        record(
            "ono.process/1",
            &[
                ("pid", Value::Int(1)),
                ("name", Value::string("init")),
                ("state", Value::string("running")),
                ("started", Value::string("2026-08-10T06:00:00Z")),
                ("pid_namespace", Value::Int(i128::from(namespace))),
            ],
        )
    };
    let projection = common::projection();
    let host = projection
        .project_as(&init(4_026_531_836), SpatialType::Process)
        .expect("the host init projects");
    let contained = projection
        .project_as(&init(4_026_533_331), SpatialType::Process)
        .expect("the container init projects");

    assert_ne!(
        host.spatial_id(),
        contained.spatial_id(),
        "without the pid namespace these two would be one place, and entering either would \
         arrive at whichever was registered first (§10.2)"
    );
}

#[test]
fn should_refuse_to_hold_one_provider_object_as_two_places() {
    // The enforcement half: `ono.process/1` is identified by `(pid, started)`, which cannot tell
    // two namespaces apart. Registering both in one scope is a provider conformance failure, and
    // the index says so rather than quietly holding one object as two places (§40, §42.1).
    let init = |namespace: i64| {
        record(
            "ono.process/1",
            &[
                ("pid", Value::Int(1)),
                ("name", Value::string("init")),
                ("state", Value::string("running")),
                ("started", Value::string("2026-08-10T06:00:00Z")),
                ("pid_namespace", Value::Int(i128::from(namespace))),
            ],
        )
    };
    let mut index = index();
    let mut bridge = bridge();
    let outcome = bridge.absorb(&mut index, &[init(4_026_531_836), init(4_026_533_331)], NOW);

    assert_eq!(index.of_type(SpatialType::Process).len(), 1);
    assert_eq!(
        outcome.refused().first().map(ono_value::ErrorValue::code),
        Some(ErrorCode::SpatialIdentityConflict),
        "got {:?}",
        outcome.refused()
    );
}

#[test]
fn should_never_resolve_a_tombstoned_place_to_the_object_that_reused_its_identifier() {
    // §42.2, and §44.7's identity replacement: nginx restarts, the old process becomes a
    // tombstone, the new one takes the pid. Entering the tombstoned place must never arrive at
    // the new process.
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(
        &mut index,
        &[process(1842, "2026-08-10T06:12:00Z", &[])],
        NOW,
    );
    let old = index
        .of_type(SpatialType::Process)
        .first()
        .expect("the first process is a place")
        .object()
        .spatial_id()
        .clone();

    let removed_at = NOW + Span::new().seconds(12);
    let mut tombstones = TombstoneRegistry::new(Span::new().minutes(5));
    index.remove(&old);
    bridge.absorb(
        &mut index,
        &[process(1842, "2026-08-11T09:03:00Z", &[])],
        removed_at,
    );
    let new = index
        .of_type(SpatialType::Process)
        .first()
        .expect("the replacement is a place")
        .object()
        .spatial_id()
        .clone();
    tombstones.record(
        Tombstone::new(old.clone(), SpatialType::Process, "nginx", removed_at).replaced_by(
            new.clone(),
            relation::spec("process.parent_of")
                .expect("a declared relation")
                .relation_type(),
        ),
    );

    assert_ne!(old, new, "the pid was reused; the object was not");
    let now = removed_at + Span::new().seconds(1);
    assert!(matches!(
        tombstones.resolve(&old, index.contains(&old), now),
        Liveness::Tombstoned(_)
    ));
    assert!(
        !index.contains(&old),
        "the old place is gone; nothing may resolve its identity to the new process"
    );
}

#[test]
fn should_report_a_denied_group_as_denied_rather_than_as_an_empty_one() {
    // §42.4 and §35.2's own example: "files  permission denied for 14 process FDs" is a
    // different fact from "files  0", and the difference must survive reaching a group.
    let detail = record(
        "ono.process-detail/1",
        &[
            ("pid", Value::Int(1842)),
            ("name", Value::string("nginx")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            ("open_files", denied("/proc/1842/fd")),
        ],
    );
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(&mut index, &[detail], NOW);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();

    let groups = index.relation_summary(&process_id, 10, NOW);
    let files = groups
        .iter()
        .find(|group| group.label() == "file")
        .expect("a process place has a `file` exit whether or not it could be read");

    assert_eq!(files.state(), PermissionState::PermissionDenied);
    assert_eq!(
        files.total(),
        None,
        "§2.17: a count nobody could take is not zero"
    );
    assert!(
        files
            .detail()
            .is_some_and(|detail| detail.contains("permission denied")),
        "the group says what was refused, got {:?}",
        files.detail()
    );
}

#[test]
fn should_keep_an_unreadable_owner_apart_from_a_process_that_has_none() {
    // The `user` reference is where `hidepid` bites first. A process whose owner this user may
    // not read is not a process with no owner.
    let hidden = record(
        "ono.process/1",
        &[
            ("pid", Value::Int(4419)),
            ("name", Value::string("rustc")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            ("user", denied("/proc/4419/status")),
        ],
    );
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(
        &mut index,
        &[hidden, process(1842, "2026-08-10T06:12:00Z", &[])],
        NOW,
    );

    let refused = bridge
        .resolve(SpatialType::Process, "4419")
        .expect("the process is a place")
        .clone();
    let plain = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();

    let state = |id: &ono_spatial_core::SpatialId| {
        index
            .relation_summary(id, 10, NOW)
            .into_iter()
            .find(|group| group.label() == "user")
            .expect("a process place has a `user` exit")
            .state()
    };
    assert_eq!(state(&refused), PermissionState::PermissionDenied);
    assert_eq!(
        state(&plain),
        PermissionState::Empty,
        "a process record that simply carried no owner is empty, not denied (§35.2)"
    );
}

#[test]
fn should_keep_a_group_no_provider_answers_for_apart_from_one_that_is_empty() {
    // §35.2's `unsupported`: nothing is installed that could answer, which is not the same as
    // "there are none".
    let unavailable = record(
        "ono.process/1",
        &[
            ("pid", Value::Int(1842)),
            ("name", Value::string("nginx")),
            ("state", Value::string("running")),
            ("started", Value::string("2026-08-10T06:12:00Z")),
            (
                "container",
                ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    "no container runtime answers on this host",
                )
                .into_value(),
            ),
        ],
    );
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(&mut index, &[unavailable], NOW);
    let process_id = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();

    let container = index
        .relation_summary(&process_id, 10, NOW)
        .into_iter()
        .find(|group| group.label() == "container")
        .expect("a process place has a `container` exit");
    assert_eq!(container.state(), PermissionState::Unsupported);
}

#[test]
fn should_map_every_refusal_a_provider_can_state_to_one_of_the_six_states() {
    // §35.2: "These states MUST remain distinct." The mapping is total, so a refusal can never
    // arrive at a place view as absence.
    for (code, expected) in [
        (
            ErrorCode::IoPermissionDenied,
            PermissionState::PermissionDenied,
        ),
        (
            ErrorCode::SpatialPermissionDenied,
            PermissionState::PermissionDenied,
        ),
        (ErrorCode::ProviderUnavailable, PermissionState::Unsupported),
        (ErrorCode::ProviderUnsupported, PermissionState::Unsupported),
        (ErrorCode::IoNotFound, PermissionState::Unknown),
    ] {
        assert_eq!(
            PermissionState::of_refusal(&ErrorValue::new(code, "refused")),
            expected,
            "{code:?} must reach a place view as {expected:?}"
        );
    }
}

#[test]
fn should_leave_a_readable_exit_alone() {
    // The counterpart of the cases above: nothing about the honesty machinery may turn a group
    // that *was* read into a state.
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(
        &mut index,
        &[
            process(1, "2026-08-10T06:00:00Z", &[]),
            process(1842, "2026-08-10T06:12:00Z", &[("ppid", Value::Int(1))]),
        ],
        NOW,
    );
    let child = bridge
        .resolve(SpatialType::Process, "1842")
        .expect("the process is a place")
        .clone();
    let parent = index
        .relation_summary(&child, 10, NOW)
        .into_iter()
        .find(|group| group.label() == "parent")
        .expect("a process place has a `parent` exit");
    assert_eq!(parent.state(), PermissionState::Available);
    assert_eq!(parent.total(), Some(1));
}

/// Keeps the unused import honest when the suite is compiled without the timestamp helper.
const _: Timestamp = NOW;
