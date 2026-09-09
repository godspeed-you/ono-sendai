//! The map view's temporal cursor: pause, rewind, step and return to now (spec v0.5 §18).
//!
//! §18.1 requires one live/historical model rather than "live diff model A" beside "historical
//! event model B", and this module is where the full-screen map of v0.4 §23.3 acquires the
//! second half of that one model. It decides *which instant the view is showing*; everything
//! that decides what the world looked like at that instant is
//! [`crate::spatial::historical::HistoricalWorld`], and everything that decides which events are
//! worth stepping to is `ono-temporal-query`. Nothing is re-derived here.
//!
//! Four rules from §18 shape it:
//!
//! - **Pausing stops the view and nothing else** (§18.2). The providers, the recorder and the
//!   machine keep running; [`TemporalCursor::toggle_pause`] writes one field.
//! - **`[` and `]` step significant events, not provider samples** (§18.4). The significance
//!   order is `ono_temporal_query::relevance::RelevanceClass`, which is §18.4's own list.
//! - **A gap is shown as a gap** (§18.6). [`TemporalCursor::gap_in`] finds the coverage gap the
//!   cursor is standing in, and the view draws it instead of the last supported state.
//! - **Returning to now summarises through the canonical `changes` engine** (§18.7).
//!   [`TemporalCursor::return_to_now`] calls `ono_temporal_query::changes::changes`, which is
//!   the same call §13.5 makes for `look` — one implementation, asked twice.

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialScope};
use ono_temporal_core::{LedgerRead, TemporalContext, TemporalEvent, TemporalGap};
use ono_temporal_query::changes::{ChangesRequest, TemporalChange, changes};
use ono_temporal_query::relevance::{Horizon, RelevanceClass, classify, is_default_scope};
use ono_temporal_query::timeline::{TimelineRequest, plan};
use ono_value::{ErrorValue, RecordValue};

use crate::spatial::historical::{HistoricalWorld, active};

/// How far a significant-event step may reach for the events it chooses between.
///
/// §32.3 budgets a bounded answer and §43.1 forbids an unbounded one, so a step reads a window
/// rather than the ledger. Twelve hours is long enough that stepping crosses a shift and short
/// enough that the query stays a query.
const STEP_WINDOW_HOURS: i64 = 12;

/// Where the view's clock is, and whether it is running (§18.1, §18.2).
#[derive(Debug, Clone)]
pub struct TemporalCursor {
    scope: SpatialScope,
    ledger: Option<Arc<dyn LedgerRead>>,
    /// The instant the view shows. `None` follows the present.
    at: Option<Timestamp>,
    /// Whether the view's own advancement is frozen (§18.2).
    paused: bool,
    /// The instant the cursor first left the present, so §18.7 knows what window to summarise.
    departed: Option<Timestamp>,
    /// The session's own coordinate, which releasing a pause returns the view to (§4.1, §18.2).
    opened_at: Option<Timestamp>,
}

impl TemporalCursor {
    /// A cursor for `scope`, starting wherever the session's temporal coordinate is (§4, §18.1).
    #[must_use]
    pub fn of(scope: SpatialScope) -> Self {
        let evidence = active();
        Self::over(
            scope,
            evidence
                .as_ref()
                .map(super::historical::Active::ledger_handle),
            evidence.as_ref().map(super::historical::Active::at),
        )
    }

    /// A cursor over `ledger`, opened at `at`.
    ///
    /// The session's coordinate is a parameter rather than something read from a global, so the
    /// cursor is a value with an origin: `Some` opens the view in the past and `None` opens it
    /// following the present (§4.1).
    #[must_use]
    pub fn over(
        scope: SpatialScope,
        ledger: Option<Arc<dyn LedgerRead>>,
        at: Option<Timestamp>,
    ) -> Self {
        Self {
            scope,
            ledger,
            at,
            paused: false,
            departed: None,
            opened_at: at,
        }
    }

    /// The instant the view is showing, or `None` where it follows the present.
    #[must_use]
    pub const fn at(&self) -> Option<Timestamp> {
        self.at
    }

    /// Whether the view's advancement is frozen (§18.2).
    #[must_use]
    pub const fn is_paused(&self) -> bool {
        self.paused
    }

    /// Whether the view is showing a reconstructed instant rather than the present.
    #[must_use]
    pub const fn is_historical(&self) -> bool {
        self.at.is_some()
    }

    /// Whether a ledger is behind the cursor at all.
    ///
    /// Without one there is nothing to rewind through, and §2.17 wants that said rather than
    /// answered with an empty past.
    #[must_use]
    pub const fn has_evidence(&self) -> bool {
        self.ledger.is_some()
    }

    /// Freezes or releases the view's temporal cursor (§18.2).
    ///
    /// Pausing pins the instant the view is showing. It stops the providers, the recorder and
    /// the machine not at all: this writes two fields and reaches nothing.
    pub fn toggle_pause(&mut self, now: Timestamp) {
        if self.paused {
            self.paused = false;
            // Releasing returns the view to whatever the session's own coordinate is, which is
            // the present unless `at` put the session in the past (§4.1, §18.1).
            self.at = self.opened_at;
            return;
        }
        self.paused = true;
        let at = self.at.unwrap_or(now);
        self.at = Some(at);
        self.departed.get_or_insert(at);
    }

    /// Moves the cursor `seconds` — §18.3's `Shift-[` and `Shift-]`.
    ///
    /// A cursor moved forward past the present lands on the present, because §18.5 forbids a
    /// state nothing supports and the future is the largest of those.
    pub fn nudge(&mut self, seconds: i64, now: Timestamp) {
        let from = self.at.unwrap_or(now);
        let moved = from
            .checked_add(jiff::Span::new().seconds(seconds))
            .unwrap_or(from)
            .min(now);
        self.set(moved, now);
    }

    /// Moves the cursor to the previous or next significant event around `center` (§18.4).
    ///
    /// Returns the event stepped to, or `None` where the window holds none in that direction —
    /// which the view says rather than moving to an instant nothing happened at.
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn step(
        &mut self,
        center: &SpatialId,
        neighbours: Vec<SpatialId>,
        forward: bool,
        now: Timestamp,
    ) -> Result<Option<TemporalEvent>, ErrorValue> {
        let Some(ledger) = &self.ledger else {
            return Ok(None);
        };
        let from = self.at.unwrap_or(now);
        let horizon = Horizon::at_place(self.scope.clone(), center.clone(), neighbours);
        let events = significant(ledger.as_ref(), &horizon, from, now)?;
        let found = if forward {
            events
                .into_iter()
                .find(|event| event.times.presentation_instant() > from)
        } else {
            events
                .into_iter()
                .rfind(|event| event.times.presentation_instant() < from)
        };
        if let Some(event) = &found {
            self.set(event.times.presentation_instant(), now);
        }
        Ok(found)
    }

    /// The coverage gap the cursor is standing in, where it is standing in one (§18.6).
    ///
    /// Both ends count. A gap that runs up to the cursor's own instant is a gap the cursor is
    /// standing in: the instant itself is the one nothing covered, which is exactly the case
    /// §18.6 forbids showing the last supported state for.
    #[must_use]
    pub fn gap_in<'a>(&self, world: &'a HistoricalWorld) -> Option<&'a TemporalGap> {
        let at = self.at?;
        world
            .gaps()
            .iter()
            .find(|gap| gap.from <= at && at <= gap.until)
    }

    /// Returns the cursor to the present, and reports what accumulated while it was away (§18.7).
    ///
    /// The summary is the canonical `changes` engine over the window the cursor spent in the
    /// past, so §13.5's "one change implementation" holds across `look` and the map view alike.
    /// An empty answer is an empty summary; a cursor that never left the present has nothing to
    /// summarise and says so with an empty list.
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn return_to_now(&mut self, now: Timestamp) -> Result<Vec<TemporalChange>, ErrorValue> {
        let departed = self.departed.take();
        self.at = None;
        self.paused = false;
        let (Some(ledger), Some(since)) = (&self.ledger, departed) else {
            return Ok(Vec::new());
        };
        changes(
            ledger.as_ref(),
            &ChangesRequest::new(self.scope.clone(), since),
            now,
        )
    }

    /// What changed between the cursor and now, without moving the cursor — §18.3's `D`.
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn changes_to_now(&self, now: Timestamp) -> Result<Vec<TemporalChange>, ErrorValue> {
        let (Some(ledger), Some(since)) = (&self.ledger, self.at) else {
            return Ok(Vec::new());
        };
        changes(
            ledger.as_ref(),
            &ChangesRequest::new(self.scope.clone(), since),
            now,
        )
    }

    /// The world the view draws while the cursor is in the past.
    ///
    /// # Errors
    ///
    /// Whatever §34 refusal the ledger raises.
    pub fn world(&self) -> Result<Option<HistoricalWorld>, ErrorValue> {
        let (Some(ledger), Some(at)) = (&self.ledger, self.at) else {
            return Ok(None);
        };
        HistoricalWorld::reconstruct(ledger.as_ref(), &self.scope, at).map(Some)
    }

    /// The HUD segment the view header carries (§18.2, §4.6).
    ///
    /// `PAUSED @14:03:12.410` while the cursor is frozen — §18.2's own words — and the plain
    /// coordinate with `[PAST]` while it is somewhere in the past without being frozen. `None`
    /// in the present, where §4.6 wants no marker at all.
    #[must_use]
    pub fn marker(&self) -> Option<String> {
        let at = self.at?;
        let options = ono_temporal_render::RenderOptions::default();
        if self.paused {
            return Some(ono_temporal_render::paused_marker(
                &ono_value::Value::Timestamp(at),
                &options,
            ));
        }
        Some(format!("@{} [PAST]", at.strftime("%H:%M:%S")))
    }

    /// §18.6's gap frame, as lines the view draws instead of the map.
    #[must_use]
    pub fn gap_frame(gap: &TemporalGap, width: usize) -> Vec<String> {
        match ono_temporal_core::value::gap_record(gap) {
            Ok(record) => ono_temporal_render::gap_frame(
                &record,
                width,
                &ono_temporal_render::RenderOptions::default(),
            ),
            // The contract is embedded in the binary, so this is unreachable in a built shell;
            // the interval is still worth stating, because §18.6 forbids showing the stale map.
            Err(_) => vec![
                "HISTORY GAP".to_owned(),
                format!("{} - {}", gap.from, gap.until),
            ],
        }
    }

    /// §18.7's return-to-now summary, as lines.
    #[must_use]
    pub fn summary_lines(changed: &[TemporalChange], width: usize) -> Vec<String> {
        let records: Vec<RecordValue> = changed
            .iter()
            .filter_map(|change| change.to_record().ok())
            .collect();
        ono_temporal_render::return_to_now(&records, width)
    }

    /// Moves the cursor to `at`, remembering that it left the present.
    fn set(&mut self, at: Timestamp, now: Timestamp) {
        self.at = Some(at);
        self.departed.get_or_insert(at.min(now));
    }
}

/// The events worth stepping to around `horizon`, in time order (§18.4).
///
/// Background bookkeeping is left out — an observation that changed nothing is exactly the raw
/// provider sample §18.4 says `[` and `]` must not step through — and so is anything the horizon
/// does not reach. The order is the presentation instant, then the event identity, so the same
/// window steps the same way twice (§26.3).
fn significant(
    ledger: &dyn LedgerRead,
    horizon: &Horizon,
    from: Timestamp,
    now: Timestamp,
) -> Result<Vec<TemporalEvent>, ErrorValue> {
    let span = jiff::Span::new().hours(STEP_WINDOW_HOURS);
    let mut request = TimelineRequest::new(horizon.clone());
    request.since = Some(from.checked_sub(span).unwrap_or(from));
    request.until = Some(from.checked_add(span).unwrap_or(now).min(now));
    let query = plan(&request, &TemporalContext::Present, now);
    let mut events: Vec<TemporalEvent> = ledger
        .events(&query)?
        .into_iter()
        .filter(|event| {
            classify(event, horizon).class != RelevanceClass::Background
                && is_default_scope(event, horizon)
        })
        .collect();
    events.sort_by(|a, b| {
        a.times
            .presentation_instant()
            .cmp(&b.times.presentation_instant())
            .then_with(|| a.event_id.as_str().cmp(b.event_id.as_str()))
    });
    Ok(events)
}
