//! The temporal error family of v0.5 §34, renumbered to `Ono-Sendai-E1301`…`E1314` by ADR-0610.
//!
//! Every other crate raises a temporal refusal through one of these constructors, so a code and
//! its metadata are decided once. The metadata is not decoration: §12.3's example refusal names
//! the sources that were asked and how far back each reaches, and §4.4's names both instants a
//! folded wall time maps to, because those are what the reader needs in order to ask again.
//!
//! Two things are deliberately *not* errors. §15.7: an unknown cause "is a valid outcome, not an
//! error". §34: "partial reconstruction is normally a successful result carrying partial
//! coverage", and [`coverage_gap`] is for the case where the operation explicitly requires
//! completeness.

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_spatial_core::SpatialScope;
use ono_value::{ErrorValue, MapValue, Value};

use crate::coverage::TemporalGap;
use crate::id::EventId;
use crate::ledger::SourceAvailability;
use crate::source::EvidenceSource;

/// A time selector that cannot resolve (§34 E1301).
#[must_use]
pub fn invalid_time(text: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalInvalidTime,
        format!("`{text}` is not a time this shell can resolve"),
    )
    .with_help(detail.to_owned())
    .with_metadata("selector", Value::string(text))
}

/// A local wall time that a DST fold makes ambiguous (§4.4, §25.6).
///
/// Both instants are named, because §4.4 requires disambiguation *by offset* and a reader cannot
/// choose an offset they have not been shown.
#[must_use]
pub fn ambiguous_local_time(text: &str, earlier: Timestamp, later: Timestamp) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalInvalidTime,
        format!("`{text}` happens twice: {earlier} and {later}"),
    )
    .with_help(format!(
        "a daylight-saving fold covers that wall time; name the offset, as in `{earlier}` or `{later}`"
    ))
    .with_metadata("selector", Value::string(text))
    .with_metadata("earlier", Value::Timestamp(earlier))
    .with_metadata("later", Value::Timestamp(later))
}

/// A local wall time that a DST gap skipped (§4.4, §25.6).
#[must_use]
pub fn skipped_local_time(text: &str, gap_from: Timestamp, gap_until: Timestamp) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalInvalidTime,
        format!(
            "`{text}` never happened: the local clock jumped between {gap_from} and {gap_until}"
        ),
    )
    .with_help("name an instant on either side of the daylight-saving jump".to_owned())
    .with_metadata("selector", Value::string(text))
    .with_metadata("gap_from", Value::Timestamp(gap_from))
    .with_metadata("gap_until", Value::Timestamp(gap_until))
}

/// No evidence source covers the requested time and scope (§34 E1302, §12.3).
///
/// The refusal lists every source that was asked, whether it answered and how far back it
/// reaches, so the next question can be a better one.
#[must_use]
pub fn not_recorded(
    scope: &SpatialScope,
    at: Timestamp,
    available: &[SourceAvailability],
) -> ErrorValue {
    let mut described = Vec::with_capacity(available.len());
    let mut lines = Vec::with_capacity(available.len());
    for source in available {
        let mut entry = MapValue::new();
        entry.insert("source".into(), Value::string(source.source.as_str()));
        entry.insert("available".into(), Value::Bool(source.available));
        entry.insert(
            "earliest".into(),
            source.earliest.map_or(Value::Null, Value::Timestamp),
        );
        entry.insert(
            "detail".into(),
            source.detail.as_deref().map_or(Value::Null, Value::string),
        );
        described.push(Value::Map(std::sync::Arc::new(entry)));
        lines.push(match (source.available, source.earliest) {
            (true, Some(earliest)) => {
                format!("{} reaches back to {earliest}", source.source.as_str())
            }
            (true, None) => format!("{} retains nothing", source.source.as_str()),
            (false, _) => format!(
                "{} did not answer{}",
                source.source.as_str(),
                source
                    .detail
                    .as_deref()
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default()
            ),
        });
    }
    ErrorValue::new(
        ErrorCode::TemporalNotRecorded,
        format!("nothing recorded {at} for {scope}"),
    )
    .with_help(if lines.is_empty() {
        "no source was asked, because none advertises history for this scope".to_owned()
    } else {
        lines.join("; ")
    })
    .with_metadata("at", Value::Timestamp(at))
    .with_metadata("scope", Value::string(&scope.to_string()))
    .with_metadata("sources", Value::list(described))
}

/// The requested history is known to have expired (§34 E1303, §12.3).
#[must_use]
pub fn out_of_retention(at: Timestamp, earliest: Timestamp) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalOutOfRetention,
        format!("{at} is older than the retained history, which starts at {earliest}"),
    )
    .with_help(format!(
        "retention holds from {earliest}; `get recorder` shows the policy in force"
    ))
    .with_metadata("at", Value::Timestamp(at))
    .with_metadata("earliest", Value::Timestamp(earliest))
}

/// A mutation attempted while historical context is active (§34 E1304, §4.7).
#[must_use]
pub fn read_only(command: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalReadOnly,
        format!("`{command}` changes the system, and the session is in the past"),
    )
    .with_help("`now` returns to the present, where mutations run".to_owned())
    .with_metadata("command", Value::string(command))
}

/// A current-only operation attempted in historical context (§34 E1305, §4.8).
#[must_use]
pub fn present_only(command: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalPresentOnly,
        format!("`{command}` runs against the live system, and the session is in the past"),
    )
    .with_help(
        "`present <command>` runs it in the present without leaving historical context".to_owned(),
    )
    .with_metadata("command", Value::string(command))
}

/// A reference that resolves to several equally valid events (§34 E1306, §11.6).
#[must_use]
pub fn ambiguous_event(candidates: &[EventId]) -> ErrorValue {
    let listed: Vec<Value> = candidates
        .iter()
        .map(|id| Value::string(&id.to_string()))
        .collect();
    let rendered: Vec<String> = candidates.iter().map(EventId::to_string).collect();
    ErrorValue::new(
        ErrorCode::TemporalAmbiguousEvent,
        format!("that reference names {} events", candidates.len()),
    )
    .with_help(format!("name one of {}", rendered.join(", ")))
    .with_metadata("candidates", Value::list(listed))
}

/// The persistent temporal store cannot be reached (§34 E1307).
#[must_use]
pub fn store_unavailable(detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalStoreUnavailable,
        "the temporal store cannot be read",
    )
    .with_help(detail.to_owned())
    .with_retryable(true)
}

/// The ledger failed an integrity check (§34 E1308, §31.7).
#[must_use]
pub fn store_corrupt(segment: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalStoreCorrupt,
        format!("the temporal ledger segment `{segment}` did not read back"),
    )
    .with_help(detail.to_owned())
    .with_metadata("segment", Value::string(segment))
}

/// History exists and this user may not read it (§34 E1309).
#[must_use]
pub fn permission_denied(scope: &SpatialScope, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalPermissionDenied,
        format!("the history of {scope} is not readable by this user"),
    )
    .with_help(detail.to_owned())
    .with_metadata("scope", Value::string(&scope.to_string()))
}

/// A source cannot provide the requested temporal capability (§34 E1310, §21.1).
#[must_use]
pub fn unsupported_source(source: &EvidenceSource, capability: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalUnsupportedSource,
        format!("`{}` does not answer `{capability}`", source.as_str()),
    )
    .with_help("`get temporal-source` lists what each source can answer".to_owned())
    .with_metadata("source", Value::string(source.as_str()))
    .with_metadata("capability", Value::string(capability))
}

/// An operation that requires completeness meets an interval that has none (§34 E1311).
#[must_use]
pub fn coverage_gap(gap: &TemporalGap) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalCoverageGap,
        format!(
            "`{}` is not covered from {} to {}",
            gap.capability, gap.from, gap.until
        ),
    )
    .with_help(format!(
        "{} ({}) covered nothing there",
        gap.source.as_str(),
        gap.reason.as_str()
    ))
    .with_metadata("capability", Value::string(&gap.capability))
    .with_metadata("from", Value::Timestamp(gap.from))
    .with_metadata("until", Value::Timestamp(gap.until))
    .with_metadata("reason", Value::string(gap.reason.as_str()))
    .with_metadata("source", Value::string(gap.source.as_str()))
}

/// Strict ordering between two events cannot be established (§34 E1312, §26.3).
#[must_use]
pub fn clock_uncertain(a: &EventId, b: &EventId) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalClockUncertain,
        format!("{a} and {b} cannot be ordered"),
    )
    .with_help(
        "no shared sequence, monotonic clock or transaction links them, and wall time is not evidence (§26.1)"
            .to_owned(),
    )
    .with_metadata("first", Value::string(&a.to_string()))
    .with_metadata("second", Value::string(&b.to_string()))
}

/// The operation requires a running recorder (§34 E1313, §10.8).
#[must_use]
pub fn recorder_not_running() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalRecorderNotRunning,
        "the recorder is not running",
    )
    .with_help("`start recorder` begins retaining local history".to_owned())
}

/// A start was requested where idempotency cannot absorb it (§34 E1314, §10.8).
#[must_use]
pub fn recorder_already_running() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalRecorderAlreadyRunning,
        "the recorder is already running",
    )
    .with_help("`get recorder` shows what it is retaining".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_carry_the_selector_when_a_time_is_refused() {
        let refused = invalid_time("yesterday", "no such form");
        let Some(Value::Map(metadata)) = refused.field("metadata") else {
            panic!("a structured refusal carries metadata");
        };
        assert_eq!(
            metadata.get("selector"),
            Some(&Value::string("yesterday")),
            "a refusal a script reads must carry what was refused"
        );
    }
}
