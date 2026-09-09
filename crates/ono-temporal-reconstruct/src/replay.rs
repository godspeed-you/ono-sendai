//! Deterministic event replay (v0.5 §9.1 step 2, §26, §47.2).
//!
//! §47.2 asks for the property this module exists to make true: "applying an event sequence to a
//! checkpoint is deterministic". §26.1 says what the order may rest on — "an internal ordering
//! relation supported by stronger evidence than wall time" — and §26.3 says what it may not
//! claim: a renderer "MAY still choose stable display order, but `inspect` MUST not claim
//! semantic ordering".
//!
//! Both hold at once by splitting the events into the groups
//! [`ono_temporal_core::happens_before`] can actually order — one clock domain with monotonic
//! readings, one source's own sequence within one domain — sorting each group by its own
//! evidence, and then merging the groups by presentation instant. Order inside a group is
//! evidence; order between groups is presentation, and [`replay_plan`] says which is which for
//! every step, so nothing downstream has to guess.

use std::collections::BTreeMap;
use std::sync::Arc;

use ono_temporal_core::{ClockDomain, EventId, OrderEvidence, TemporalEvent};

/// Where one event sits in the replay, and what put it there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayStep {
    /// The event.
    pub event: EventId,
    /// What orders it against the step before it. `None` means presentation order placed it and
    /// no ordering is claimed (§26.3).
    pub evidence: Option<OrderEvidence>,
}

/// The group an event can be ordered within, and by what.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Group {
    /// One host, one boot: monotonic readings are comparable (§25.1, §25.5).
    Monotonic(ClockDomain),
    /// One source's own stream inside one clock domain (§25.4).
    Sequence(ClockDomain, Arc<str>),
    /// Nothing orders this event against anything.
    Alone(Arc<str>),
}

impl Group {
    fn evidence(&self) -> Option<OrderEvidence> {
        match self {
            Group::Monotonic(_) => Some(OrderEvidence::Monotonic),
            Group::Sequence(..) => Some(OrderEvidence::SourceSequence),
            Group::Alone(_) => None,
        }
    }
}

/// Which group an event belongs to, mirroring what `happens_before` will answer for it.
fn group_of(event: &TemporalEvent) -> Group {
    if event.times.domain.boot_id.is_some() && event.times.monotonic_nanos.is_some() {
        return Group::Monotonic(event.times.domain.clone());
    }
    if event.times.source_sequence.is_some() {
        return Group::Sequence(
            event.times.domain.clone(),
            Arc::from(event.provenance.provider()),
        );
    }
    Group::Alone(Arc::from(event.event_id.as_str()))
}

/// The rank of an event inside its own group.
fn rank_in_group(event: &TemporalEvent) -> (u64, Arc<str>) {
    let rank = event
        .times
        .monotonic_nanos
        .or(event.times.source_sequence)
        .unwrap_or_default();
    (rank, Arc::from(event.event_id.as_str()))
}

/// Orders `events` for replay: evidence inside a group, presentation between groups.
///
/// The answer is a function of the set alone, so the same events produce the same replay
/// whatever order they arrived in — which is §47.2's determinism property and what lets a
/// reconstruction after a restart equal the one before it.
#[must_use]
pub fn replay_order(events: Vec<TemporalEvent>) -> Vec<TemporalEvent> {
    let mut groups: BTreeMap<Group, Vec<TemporalEvent>> = BTreeMap::new();
    for event in events {
        groups.entry(group_of(&event)).or_default().push(event);
    }
    for members in groups.values_mut() {
        members.sort_by_key(rank_in_group);
    }

    let mut queues: Vec<(Group, std::vec::IntoIter<TemporalEvent>)> = groups
        .into_iter()
        .map(|(group, members)| (group, members.into_iter()))
        .collect();
    let mut heads: Vec<Option<TemporalEvent>> = queues
        .iter_mut()
        .map(|(_, members)| members.next())
        .collect();

    let mut ordered = Vec::new();
    loop {
        let Some(next) = heads
            .iter()
            .enumerate()
            .filter_map(|(index, head)| head.as_ref().map(|event| (index, event)))
            .min_by(|(left_index, left), (right_index, right)| {
                left.times
                    .presentation_instant()
                    .cmp(&right.times.presentation_instant())
                    .then_with(|| left.event_id.as_str().cmp(right.event_id.as_str()))
                    .then(left_index.cmp(right_index))
            })
            .map(|(index, _)| index)
        else {
            break;
        };
        if let Some(event) = heads.get_mut(next).and_then(Option::take) {
            ordered.push(event);
        }
        if let (Some(slot), Some((_, members))) = (heads.get_mut(next), queues.get_mut(next)) {
            *slot = members.next();
        }
    }
    ordered
}

/// The replay order, with what placed every step (§26.3).
///
/// A step whose evidence is `None` was placed by presentation order alone: it is a position in a
/// list and not a claim that anything happened before anything else.
#[must_use]
pub fn replay_plan(events: &[TemporalEvent]) -> Vec<ReplayStep> {
    let mut previous: Option<Group> = None;
    let mut plan = Vec::with_capacity(events.len());
    for event in events {
        let group = group_of(event);
        let evidence = (previous.as_ref() == Some(&group))
            .then(|| group.evidence())
            .flatten();
        plan.push(ReplayStep {
            event: event.event_id.clone(),
            evidence,
        });
        previous = Some(group);
    }
    plan
}
