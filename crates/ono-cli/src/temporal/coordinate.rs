//! Entering historical context and returning from it (spec v0.5 §4.2, §4.3, §12).
//!
//! §12.1 is the rule the whole module is arranged around: `at` resolves the selector **and** its
//! coverage before the session moves, so a selector that cannot be resolved leaves the coordinate
//! exactly where it was. Nothing here mutates the state until every refusal has been raised.
//!
//! The second rule is §4.2's: `at` changes only the temporal coordinate. There is no call into
//! `crate::spatial::session` in this file, which is how that is kept rather than remembered.

use std::sync::Arc;

use jiff::Timestamp;
use ono_temporal_core::{
    CoverageQuery, CoverageSummary, EventAnchors, EventId, EventQuery, LedgerRead, QueryOrder,
    SourceAvailability, TemporalContext, TimeRange, TimeResolution, TimeSelector, error,
};
use ono_value::{ErrorValue, Value};

use super::session::TemporalState;

/// The instants the ledger can anchor `at event @e42` to (§12.2).
///
/// A thin adapter rather than an implementation: resolving a prefix to one event and refusing an
/// ambiguous one is the ledger's job, and this only turns the event it found into the instant a
/// person navigates by (`EventTimes::presentation_instant`).
struct LedgerAnchors<'a>(&'a dyn LedgerRead);

impl EventAnchors for LedgerAnchors<'_> {
    fn instant_of(&self, id: &EventId) -> Option<Timestamp> {
        self.0
            .event(id)
            .ok()
            .flatten()
            .map(|event| event.times.presentation_instant())
    }
}

/// The historical context `text` names, resolved but not yet committed (§12.1).
///
/// # Errors
///
/// `temporal.invalid_time` for a selector §4.4 does not define, for a relative selector naming
/// the future, and for a local wall time a daylight-saving fold covers — with both instants named
/// so the reader can disambiguate by offset (§4.4, §25.6). `temporal.out_of_retention` where the
/// instant is older than anything retained, and `temporal.not_recorded` where no source can reach
/// it at all (§12.3).
pub fn resolve(
    state: &TemporalState,
    text: &str,
    now: Timestamp,
) -> Result<TemporalContext, ErrorValue> {
    let selector = TimeSelector::parse(text)?;
    let ledger = state.ledger();
    let anchors = LedgerAnchors(ledger);
    let resolved_at = match selector.resolve(state.zone(), now, &anchors)? {
        TimeResolution::Resolved(instant) => instant,
        TimeResolution::Ambiguous { earlier, later } => {
            return Err(error::ambiguous_local_time(text, earlier, later));
        }
        TimeResolution::Skipped {
            gap_from,
            gap_until,
        } => {
            return Err(error::skipped_local_time(text, gap_from, gap_until));
        }
    };
    // §4.4 again, this time about an absolute selector: standing in the future is not historical
    // context, it is a claim about evidence that cannot exist.
    if resolved_at > now {
        return Err(error::invalid_time(
            text,
            "§4.4: a historical coordinate is at or before now",
        ));
    }

    let anchor_event = match &selector {
        TimeSelector::Event(id) => Some(id.clone()),
        _ => None,
    };
    let scope = crate::spatial::local_scope();
    let window = TimeRange::between(resolved_at, now);
    let mut intervals = ledger.coverage(&CoverageQuery {
        scope: Some(scope.clone()),
        capabilities: Vec::new(),
        range: window,
    })?;
    // §10.7: the session's own bounded ledger is an evidence source for the stretch the session
    // has been alive, and §8.3 is the honest word for what it covers — it observes every action
    // taken through the shell and only what a provider happened to report besides. Declaring it
    // `partial` is what puts `[PAST?]` on the prompt rather than `[PAST]` (§8.6).
    if state.started_at() <= now {
        intervals.push(super::session::session_coverage(
            &scope,
            state.started_at(),
            now,
        ));
    }
    let coverage = CoverageSummary::compose(&intervals, window);

    // §12.3: a coordinate no source can reach is a refusal that lists what each source *can*
    // reach, and the session stays where it was. §34 keeps "never recorded" and "expired" apart,
    // and the ledger's own retention boundary is what tells them apart.
    if !reachable(state, resolved_at)? {
        return Err(unreachable(state, resolved_at, now));
    }

    Ok(TemporalContext::Historical {
        requested: selector,
        requested_text: Arc::from(text),
        resolved_at,
        coverage,
        anchor_event,
    })
}

/// Whether any evidence at all reaches `at` (§12.3).
///
/// Three things count, and all three are facts rather than judgements: the instant falls inside
/// the stretch this session has been observing, the ledger retains an event at or before it, or
/// the ledger holds a coverage interval that spans it. A coverage interval alone is enough —
/// §8.2's "complete coverage" of an interval in which nothing happened is exactly the case where
/// an empty answer is the true one.
/// Whether any source can answer about `at`.
///
/// §12.3 is about the coordinate a session moves to. `changes` asks a different question and
/// §13.4 answers it differently — a side without evidence is reported unknown, with the coverage
/// that made it unknown — so this stays `at`'s rule rather than becoming a shared one.
fn reachable(state: &TemporalState, at: Timestamp) -> Result<bool, ErrorValue> {
    if at >= state.started_at() {
        return Ok(true);
    }
    let ledger = state.ledger();
    let before = ledger.events(&EventQuery {
        scope: None,
        subjects: Vec::new(),
        kinds: Vec::new(),
        range: TimeRange::until(at),
        limit: Some(1),
        order: QueryOrder::Descending,
    })?;
    if !before.is_empty() {
        return Ok(true);
    }
    let covering = ledger.coverage(&CoverageQuery {
        scope: None,
        capabilities: Vec::new(),
        range: TimeRange::at(at),
    })?;
    Ok(!covering.is_empty())
}

/// §34's refusal for an instant no source can reach, of the two kinds §34 keeps apart.
///
/// §34 words `temporal.out_of_retention` as "requested history is **known to have expired**", and
/// that is a stronger claim than "older than the policy window". A store that began recording five
/// seconds ago has a `max_age` of 24h and an `earliest` of five seconds ago; three days back is
/// outside its window and was never inside it, so nothing expired and §12.3's
/// `temporal.not_recorded` — with the list of what each source *can* reach — is the honest answer.
/// §12.3's own worked example is exactly that shell asking `at -3d`.
///
/// So retention is the reason only once the policy has demonstrably swept: the retained record has
/// to begin at or before the horizon, which is another way of saying the store has been keeping
/// history for longer than it keeps history. Then an instant below the horizon is one this store
/// held and discarded, and "the earliest instant still retained" is a boundary it can name.
///
/// Shared with `changes`, so the two commands give the same reason for the same instant (§12.3,
/// §13.4, §55.5).
pub(crate) fn unreachable(state: &TemporalState, at: Timestamp, now: Timestamp) -> ErrorValue {
    let retention = state.ledger().retention();
    if let (Some(max_age), Some(earliest)) = (retention.max_age, retention.earliest) {
        let horizon = now.as_nanosecond().saturating_sub(max_age.nanoseconds());
        if at.as_nanosecond() < horizon && earliest.as_nanosecond() <= horizon {
            return error::out_of_retention(at, earliest);
        }
    }
    error::not_recorded(
        &crate::spatial::local_scope(),
        at,
        &availability(state, now),
    )
}

/// What each source can reach, as §12.3's refusal lists it.
///
/// Only sources the shell can honestly speak for: the session's own bounded ledger, and the
/// recorder. A provider's historical reach is the provider's to state (§21.1), and inventing a
/// row for one that was never asked would be the fabrication §35.3 forbids.
pub fn availability(state: &TemporalState, now: Timestamp) -> Vec<SourceAvailability> {
    let retention = state.ledger().retention();
    let persistent = state.ledger().is_persistent();
    let mut rows = vec![SourceAvailability {
        source: ono_temporal_core::EvidenceSource::session(),
        earliest: retention.earliest,
        available: retention.earliest.is_some(),
        detail: retention.earliest.map(|earliest| -> Arc<str> {
            Arc::from(
                ono_value::Duration::from_nanoseconds(
                    now.as_nanosecond().saturating_sub(earliest.as_nanosecond()),
                )
                .to_string()
                .as_str(),
            )
        }),
    }];
    rows.push(SourceAvailability {
        source: ono_temporal_core::EvidenceSource::recorder(),
        earliest: if persistent { retention.earliest } else { None },
        available: persistent,
        detail: (!persistent).then(|| -> Arc<str> { Arc::from("disabled") }),
    });
    rows
}

/// The `ono.temporal-context/1` record `at` and `now` answer with (§35.2).
///
/// # Errors
///
/// Returns whatever the value bridge reports when the contract is not in this build.
pub fn context_record(context: &TemporalContext) -> Result<Value, ErrorValue> {
    ono_temporal_core::value::context_record(context).map(|record| Value::Record(Arc::new(record)))
}

/// One instant from one of §4.4's five selector forms, without moving the session.
///
/// This is what `--since`, `--until` and `--at` read, and it is deliberately the same function
/// `at` resolves through: §4.5 requires `--at` to use the same engine as `at` context and forbids
/// a second historical code path.
///
/// # Errors
///
/// `temporal.invalid_time` for anything §4.4 does not define, and for a local wall time a
/// daylight-saving fold covers.
pub fn resolve_instant(
    state: &TemporalState,
    text: &str,
    now: Timestamp,
) -> Result<Timestamp, ErrorValue> {
    let selector = TimeSelector::parse(text)?;
    let ledger = state.ledger();
    let anchors = LedgerAnchors(ledger);
    match selector.resolve(state.zone(), now, &anchors)? {
        TimeResolution::Resolved(instant) => Ok(instant),
        TimeResolution::Ambiguous { earlier, later } => {
            Err(error::ambiguous_local_time(text, earlier, later))
        }
        TimeResolution::Skipped {
            gap_from,
            gap_until,
        } => Err(error::skipped_local_time(text, gap_from, gap_until)),
    }
}
