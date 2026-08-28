//! The live map (spec v0.4 §25.1, §25.2, §25.3, §25.4, §6.9, §43.6).
//!
//! §25.1: "`map --live` MUST subscribe to available provider events and/or explicit polling
//! sources." Both halves are here, and neither of them is a new runtime: the events are the ones
//! the v0.2 watch runtime emits ([`ono_command::watch_events`], ADR-0024), because §2.16 forbids
//! the spatial layer from becoming a second source of system truth.
//!
//! The loop is three steps and one rule.
//!
//! 1. **Wait for an event.** Every provider target the current horizon reads is watched. An event
//!    says an object was added, changed or removed; a removal is what makes a place a tombstone
//!    (§10.3) and drops the relationships nobody asserts any more (§33.2).
//! 2. **Re-project.** The same observation the still `map` makes, so the live view and the still
//!    view cannot disagree about what the system looks like (§45.4).
//! 3. **Compare, and emit only a difference.** `ono-spatial-events` reduces both projections to
//!    what a change can be about and says what moved. A projection identical to the last one is
//!    not emitted at all.
//!
//! The rule is §25.2 and §2.12: motion means change. Nothing here emits on a timer, and an event
//! about an object outside the horizon changes no picture and produces no value. §43.6 is the
//! test-side statement of the same thing — "no test may pass based only on timer animation" — and
//! the acceptance case for this is `docker/acceptance/cases/108-spatial-live.case`.

use std::collections::BTreeSet;
use std::time::Duration;

use jiff::Timestamp;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{ProviderRegistry, Query};
use ono_spatial_core::SpatialId;
use ono_spatial_events::{ChangeSet, EventKind, EventMerge, MapSnapshot, compare};
use ono_spatial_query::MapRequest;
use ono_value::{ErrorValue, Value};

use crate::spatial::session::SpatialSessionState;

/// The provider targets a live view of `center` has to watch (§25.1).
///
/// They are the targets the still projection already reads — a listener's connections come from
/// the socket target, a domain's contents from the targets its collections are served by — so a
/// change anywhere the map draws is a change the view sees, and nothing else is subscribed to.
#[must_use]
pub fn targets_of(session: &SpatialSessionState, center: &SpatialId) -> BTreeSet<&'static str> {
    let mut targets = BTreeSet::new();
    if let Some(space) = ono_spatial_query::resolve::space_of(center) {
        let mut sourced: Vec<&'static ono_spatial_core::CanonicalSpace> =
            ono_spatial_core::space::children(space.id)
                .filter(|child| child.is_served())
                .collect();
        if space.member_type.is_some() {
            sourced.push(space);
        }
        for source in sourced
            .iter()
            .filter_map(|child| ono_spatial_query::source_of_space(child.id))
        {
            targets.extend(source.targets.iter().copied());
        }
    } else if let Some(entry) = session.index().get(center) {
        let object_type = entry.object().object_type();
        if let Some((target, _)) = crate::spatial::relations::target_of(object_type) {
            targets.insert(target);
        }
        targets.extend(crate::spatial::relations::adjacent_targets(object_type));
    }
    targets
}

/// Whether a place can be watched at all — §22's `live_capable`, answered rather than assumed.
#[must_use]
pub fn capable(
    providers: &ProviderRegistry,
    session: &SpatialSessionState,
    center: &SpatialId,
) -> bool {
    targets_of(session, center)
        .iter()
        .any(|target| ono_command::is_watchable(target) && !providers.for_target(target).is_empty())
}

/// What one turn of the live loop produced: a projection, and what moved to produce it.
pub struct LiveUpdate {
    /// The projection itself, already the `ono.spatial-map/1` record of §22.
    pub map: ono_value::RecordValue,
    /// What differs from the value before it — empty for the opening snapshot (§25.1).
    pub changes: ChangeSet,
}

/// The stream `map --live` answers with (§25.1, §29.4).
///
/// The first value is the current state, as every subscription in this shell begins (ADR-0024,
/// v0.2 §18.2); every value after it is a real change. The stream is unbounded and ends when the
/// consumer stops listening — `map --live --json | take 3` is three values and then nothing.
pub fn stream(
    providers: ProviderRegistry,
    center: SpatialId,
    request: MapRequest,
    targets: BTreeSet<&'static str>,
    interval: Duration,
    render: impl Fn(&LiveUpdate) -> Result<Value, ErrorValue> + Send + 'static,
) -> ValueStream {
    ValueStream::spawn(
        PipelineConfig::new(),
        Boundedness::Unbounded,
        move |sink| async move {
            let mut watches = Vec::new();
            for target in &targets {
                if providers.for_target(target).is_empty() {
                    continue;
                }
                match ono_command::watch_events(
                    &providers,
                    target,
                    Query::target(*target),
                    interval,
                ) {
                    Ok(stream) => watches.push(stream),
                    // A target with no event contract simply is not watched; §25.4's snapshot
                    // comparison covers what the others report, and saying nothing about it is
                    // better than claiming a liveness it has not got (§2.17).
                    Err(_) => continue,
                }
            }
            if watches.is_empty() {
                let _ = sink
                    .fail(
                        ErrorValue::new(
                            ono_core::ErrorCode::SpatialUnsupported,
                            "nothing that feeds this place can be watched, so there is no live \
                             view of it",
                        )
                        .with_help(
                            "`map` draws it once; `map --json` is the same picture as a value \
                             (spec v0.4 §25.1, §40)",
                        ),
                    )
                    .await;
                return;
            }

            let mut merge = EventMerge::new();
            let mut previous: Option<MapSnapshot> = None;
            let mut removed: Vec<SpatialId> = Vec::new();
            let mut opened = false;

            loop {
                // The opening projection is made before any event is read, so the first value is
                // the state the caller is standing in rather than whatever the first tick found.
                let now = Timestamp::now();
                let projected = reproject(&providers, &center, &request, &removed, now).await;
                removed.clear();
                let map = match projected {
                    Ok(map) => map,
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                };
                let shape = MapSnapshot::of(&map);
                let changes = match &previous {
                    // §24.3: no change is invented where there is nothing to compare to. The
                    // opening value is a snapshot and says so by carrying no changes at all.
                    None => ChangeSet::new(
                        ono_spatial_events::ChangeSource::SnapshotComparison,
                        merge.freshness(),
                    ),
                    Some(before) => compare(before, &shape, merge.freshness()),
                };
                let emit = !opened || !changes.is_empty();
                previous = Some(shape);
                opened = true;

                if emit {
                    // The record is built once the diff is known, so a live value carries both
                    // the picture and what moved to produce it (§45.5).
                    let record = {
                        let session = crate::spatial::spatial_session().await;
                        crate::spatial::map::record_of(&providers, &session, &map, Some(&changes))
                    };
                    let record = match record {
                        Ok(record) => record,
                        Err(error) => {
                            let _ = sink.fail(error).await;
                            return;
                        }
                    };
                    let update = LiveUpdate {
                        map: record,
                        changes,
                    };
                    match render(&update) {
                        Ok(value) => {
                            if sink.send(value).await.is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            let _ = sink.fail(error).await;
                            return;
                        }
                    }
                }

                // Wait for the machine to do something. Nothing is emitted while nothing happens,
                // which is §25.2 and §2.12 in one line.
                if sink.is_cancelled()
                    || !wait_for_change(&mut watches, &mut merge, &mut removed).await
                {
                    return;
                }
            }
        },
    )
}

/// Blocks until at least one watched target reports a change (§25.1, §25.2).
///
/// Nothing is emitted while nothing happens, which is §2.12 and §25.2 in one function: this waits
/// on the events rather than on a clock, so an idle system produces an idle view. An opening
/// `snapshot` is not a change — ADR-0024 makes it the state the stream starts from, and reporting
/// it as movement would be the startup artefact §43.6 forbids.
///
/// Once something has moved, the rest of that moment is drained before the caller redraws. A
/// connection closing is a removal *and* an appearance — the kernel keeps the closing end in the
/// table under a new identity — and a picture that showed one without the other would be of no
/// moment that ever existed.
///
/// Returns `false` when every watch has ended or the consumer stopped listening.
async fn wait_for_change(
    watches: &mut Vec<ValueStream>,
    merge: &mut EventMerge,
    removed: &mut Vec<SpatialId>,
) -> bool {
    if !next_change(watches, merge, removed).await {
        return false;
    }
    // The events of one tick arrive back to back; a quiet moment is what ends it.
    while tokio::time::timeout(SETTLE, next_change(watches, merge, removed))
        .await
        .is_ok_and(|moved| moved)
    {}
    true
}

/// How long a live view waits for the rest of a moment's events before it redraws.
///
/// Short enough that a change is on screen within the §34 budget for a frame, long enough that
/// the events of one poll tick arrive together.
const SETTLE: Duration = Duration::from_millis(60);

/// Reads events until one of them is a change, or every watch has ended.
async fn next_change(
    watches: &mut Vec<ValueStream>,
    merge: &mut EventMerge,
    removed: &mut Vec<SpatialId>,
) -> bool {
    loop {
        if watches.is_empty() {
            return false;
        }
        let (event, index) = next_event(watches).await;
        let Some(event) = event else {
            watches.remove(index);
            continue;
        };
        let ono_pipeline::StreamEvent::Value(value) = event else {
            // A failure on one source is not a change on the map; §35.2 keeps "could not read"
            // and "not there" apart, and only the second is topology.
            continue;
        };
        let Some(observed) = merge.absorb(&value) else {
            continue;
        };
        if observed.kind() == EventKind::Removed
            && let Some(record) = observed.object()
        {
            let spatial = crate::spatial::spatial_session().await;
            if let Ok(id) = spatial.projection_of(record) {
                removed.push(id);
            }
        }
        if observed.kind() != EventKind::Snapshot {
            return true;
        }
    }
}

/// The next event from whichever watch produces one first, and which watch that was.
async fn next_event(watches: &mut [ValueStream]) -> (Option<ono_pipeline::StreamEvent>, usize) {
    let mut pending: Vec<_> = watches
        .iter_mut()
        .map(|watch| Box::pin(watch.recv()))
        .collect();
    let (event, index, _) = futures::future::select_all(pending.iter_mut()).await;
    (event, index)
}

/// Observes the horizon again and re-projects it — the same path the still `map` takes (§45.4).
async fn reproject(
    providers: &ProviderRegistry,
    center: &SpatialId,
    request: &MapRequest,
    removed: &[SpatialId],
    now: Timestamp,
) -> Result<ono_spatial_query::SpatialMap, ErrorValue> {
    let mut session = crate::spatial::spatial_session().await;
    // §10.3 and §33.2: an object a provider announced as removed is gone, and the relationships
    // it was an end of are not asserted any more. Both before the horizon is read again, so the
    // projection is of what is there rather than of what was.
    for id in removed {
        session.record_removed(id, now);
    }
    crate::spatial::map::project_at(providers, &mut session, center, request, now).await
}
