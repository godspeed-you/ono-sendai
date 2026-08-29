//! The landmark engine (spec v0.4 §26, §3.7, §24.1).
//!
//! §26.1 gives landmarks two jobs — orientation anchors, and relevance ranking for huge systems —
//! and §26.2 lists the conservative built-in rules the core should provide. This module is those
//! rules. It is handed one object, the record its provider last answered with, and the thresholds
//! §26.3 requires to be configurable, and it answers with the reasons that object deserves
//! attention for.
//!
//! Three rules bound it, and they are the difference between a landmark engine and decoration:
//!
//! - **A landmark is driven by real state (§2.11, §2.16).** Every reason below reads a field a
//!   schema in `docs/spec/schemas/` declares, and the evidence it carries is that field's value in
//!   the user's own terms. Nothing here reads the system itself.
//! - **A reason that cannot be evidenced is not a landmark.** A null field is unknown, and unknown
//!   is not a promotion (§2.17). `docs/spec/spatial/landmarks.yaml` says it outright: "A landmark
//!   whose evidence is unavailable is not a landmark; it is an unknown."
//! - **Conservative by default (§26.3).** "Ono MUST avoid pretending that a local heuristic is an
//!   incident", so the thresholds start high and `privileged` never promotes an object on its own
//!   — §26.2 asks for it "when context makes it relevant", and the context that makes it relevant
//!   is that something else already promoted the object.
//!
//! The reasons are the closed set of §3.7; a rule of §26.2 that no §3.7 reason names, and a rule
//! whose evidence no installed provider serves, is deliberately absent rather than approximated
//! (ADR-0163).

use jiff::{Span, Timestamp};
use ono_spatial_core::{Landmark, LandmarkReason, SpatialObject, SpatialScope, SpatialType};
use ono_value::{RecordValue, Value};

/// The thresholds of §26.3, as `docs/spec/spatial/landmarks.yaml` declares them and the
/// `spatial.landmarks.*` settings spell them.
///
/// The defaults are the registry's defaults; a session that reads the user's configuration
/// replaces them, which is what makes §26.3's "inspectable and configurable" true rather than
/// advertised.
#[derive(Debug, Clone)]
pub struct LandmarkThresholds {
    /// Whether the engine runs at all (`spatial.landmarks.enabled`).
    pub enabled: bool,
    /// CPU percent at or above which an object is a `high_cpu` landmark.
    pub high_cpu_percent: f64,
    /// Used share of a filesystem, in percent, that makes a `storage_pressure` landmark.
    pub storage_pressure_percent: f64,
    /// How far back "recent" reaches (`spatial.look.change_window`).
    pub change_window: Span,
}

impl Default for LandmarkThresholds {
    fn default() -> Self {
        Self {
            enabled: true,
            high_cpu_percent: 80.0,
            storage_pressure_percent: 90.0,
            change_window: Span::new().minutes(5),
        }
    }
}

/// The landmarks the built-in rules of §26.2 find on one object.
///
/// `record` is what the provider last answered with; without it only the rules that read identity
/// and scope can fire, because everything else would be a guess (§2.16).
#[must_use]
pub fn landmarks_of(
    object: &SpatialObject,
    record: Option<&RecordValue>,
    thresholds: &LandmarkThresholds,
    local: &SpatialScope,
    now: Timestamp,
) -> Vec<Landmark> {
    if !thresholds.enabled {
        return Vec::new();
    }
    let id = object.spatial_id().clone();
    let mut found: Vec<Landmark> = Vec::new();

    // --- scope (§26.2 security/scope, §2.18) --------------------------------------------------
    if let Some(boundary) = local.boundary_to(object.scope()) {
        let reason = if boundary.is_remote() {
            LandmarkReason::RemoteBoundary
        } else {
            LandmarkReason::SecurityBoundary
        };
        found.push(Landmark::built_in(
            id.clone(),
            reason,
            format!("{} boundary to {}", boundary.kind(), object.scope()),
        ));
    }

    let Some(record) = record else {
        return found;
    };

    // --- compute (§26.2) -----------------------------------------------------------------------
    if let Some(cpu) = percent(record, "cpu")
        && cpu >= thresholds.high_cpu_percent
    {
        found.push(Landmark::built_in(
            id.clone(),
            LandmarkReason::HighCpu,
            format!("cpu {cpu:.0}%"),
        ));
    }
    if state_of(record).is_some_and(|state| state == "failed") {
        found.push(Landmark::built_in(
            id.clone(),
            LandmarkReason::Failed,
            "state failed".to_owned(),
        ));
    }

    // --- recent change (§26.2 "unexpected exit/recent start", §3.7) ----------------------------
    if let Some((field, at)) = started_at(record)
        && let Ok(edge) = now.checked_sub(thresholds.change_window)
        && at >= edge
        && at <= now
    {
        found.push(Landmark::built_in(
            id.clone(),
            LandmarkReason::RecentlyChanged,
            format!("{field} {at}"),
        ));
    }

    // --- network (§26.2) -----------------------------------------------------------------------
    if let Some(address) = public_listen_address(object.object_type(), record) {
        found.push(Landmark::built_in(
            id.clone(),
            LandmarkReason::PublicListener,
            format!("listening on {address}"),
        ));
    }

    // --- storage (§26.2) -----------------------------------------------------------------------
    if let Some(used) = used_share(record)
        && used >= thresholds.storage_pressure_percent
        && !is_read_only(record)
    {
        found.push(Landmark::built_in(
            id.clone(),
            LandmarkReason::StoragePressure,
            format!("{used:.0}% used"),
        ));
    }

    // --- privilege (§26.2 "privileged process when context makes it relevant", §26.3) ----------
    // The context that makes it relevant is that the object is already worth looking at. A rule
    // that promoted every root-owned object would turn an ordinary Linux host into an alert
    // board, which is exactly what §26.3 forbids; as an attribute of something already promoted
    // it answers the question the user is actually asking.
    if !found.is_empty()
        && let Some(who) = privileged_owner(record)
    {
        found.push(Landmark::built_in(
            id.clone(),
            LandmarkReason::Privileged,
            who,
        ));
    }
    found
}

/// The `state` a provider reported, where the record carries one as a word.
fn state_of(record: &RecordValue) -> Option<String> {
    match record.get("state") {
        Some(Value::String(state)) => Some(state.to_string()),
        _ => None,
    }
}

/// A percentage field, however the provider spelled the number.
fn percent(record: &RecordValue, field: &str) -> Option<f64> {
    match record.get(field) {
        Some(Value::Percent(percent)) => Some(percent.value()),
        Some(Value::Float(number)) => Some(*number),
        Some(Value::Int(number)) => Some(*number as f64),
        _ => None,
    }
}

/// When the object began, in the provider's own words: a process started, a service has been in
/// its state since, a container was created.
fn started_at(record: &RecordValue) -> Option<(&'static str, Timestamp)> {
    for field in ["started", "since", "created"] {
        if let Some(Value::Timestamp(at)) = record.get(field) {
            return Some((field, *at));
        }
    }
    None
}

/// The address a listener is reachable at from outside this host, or `None` when it is not one.
///
/// §26.2's "public listener": a socket in `listen` state whose local address is not confined to
/// the loopback interface. A loopback listener is not reachable from anywhere else, and promoting
/// it would be the alert board §26.3 warns against.
fn public_listen_address(object_type: SpatialType, record: &RecordValue) -> Option<String> {
    if !object_type.is_a(SpatialType::Socket) {
        return None;
    }
    if state_of(record).as_deref() != Some("listen") {
        return None;
    }
    let local = record.get("local")?;
    let address = field_of(local, "address")?;
    let text = ono_value::canonical_text(&address).ok()?;
    if text.is_empty() || is_loopback(&text) {
        return None;
    }
    let port = field_of(local, "port")
        .and_then(|port| ono_value::canonical_text(&port).ok())
        .unwrap_or_default();
    if port.is_empty() {
        Some(text)
    } else {
        Some(format!("{text}:{port}"))
    }
}

/// One field of a nested value, whether the provider spelled it as a record or as a map.
///
/// `ono.socket/1` declares `local` as `ref<ono.endpoint/1>`; a provider that answers with the
/// endpoint record and one that answers with its fields are saying the same thing.
fn field_of(value: &Value, field: &str) -> Option<Value> {
    match value {
        Value::Record(record) => record.get(field).cloned(),
        Value::Map(map) => map.get(field).cloned(),
        _ => None,
    }
    .filter(|found| !found.is_null())
}

/// Whether an address reaches only this host.
fn is_loopback(address: &str) -> bool {
    match address.parse::<std::net::IpAddr>() {
        Ok(parsed) => parsed.is_loopback(),
        // A unix socket's "address" is a path: it never leaves the host.
        Err(_) => address.starts_with('/') || address.starts_with('@') || address.is_empty(),
    }
}

/// Whether the provider says the filesystem cannot be written to (§26.2).
///
/// A read-only image is full by construction — a squashfs snap is 100% used the moment it is
/// mounted — so "near capacity" is not a reason to look at it: nothing can fill it and nothing
/// can be freed. §35.3 keeps the guard narrow: only an explicit `read_only: true` suppresses the
/// rule, because a provider that does not answer the question has not said the filesystem is
/// read-only.
fn is_read_only(record: &RecordValue) -> bool {
    matches!(record.get("read_only"), Some(Value::Bool(true)))
}

/// How full a filesystem is, from the provider's own `used` and `size`.
fn used_share(record: &RecordValue) -> Option<f64> {
    let used = bytes(record, "used")?;
    let size = bytes(record, "size")?;
    if size == 0 {
        return None;
    }
    Some(used as f64 / size as f64 * 100.0)
}

fn bytes(record: &RecordValue, field: &str) -> Option<u128> {
    match record.get(field) {
        Some(Value::ByteSize(size)) => Some(size.bytes()),
        Some(Value::Int(number)) => u128::try_from(*number).ok(),
        _ => None,
    }
}

/// Who owns the object, when that owner is privileged (§26.2).
///
/// A provider spells the owner either as a numeric uid — a socket's `user` — or as the user
/// record a process carries; both answer the same question.
fn privileged_owner(record: &RecordValue) -> Option<String> {
    match record.get("user") {
        Some(Value::Int(0)) => Some("owned by uid 0".to_owned()),
        Some(Value::Record(user)) => match user.get("uid") {
            Some(Value::Int(0)) => Some(match user.get("name") {
                Some(Value::String(name)) => format!("running as {name}"),
                _ => "running as uid 0".to_owned(),
            }),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_not_promote_a_loopback_listener_as_a_public_one() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("0.0.0.0"));
        assert!(!is_loopback("10.0.0.5"));
        assert!(is_loopback("/run/dbus/system_bus_socket"));
    }
}
