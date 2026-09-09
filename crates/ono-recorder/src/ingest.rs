//! Bounded ingestion, and the gap an overflow leaves (v0.5 §43.1, §43.2, §32.5).
//!
//! §43.1 is absolute: "every event ingestion path MUST be bounded. Unbounded channels are
//! prohibited", and `xtask::scan::check_bounded_channels` refuses one in the tree. So an
//! [`Intake`] is a [`tokio::sync::mpsc`] channel with a capacity, and the interesting decision is
//! what happens when it is full.
//!
//! §43.2 answers it: "Ono MUST prefer an explicit coverage gap over pretending continuity". The
//! recorder therefore never blocks a provider to make room — blocking a netlink reader is how a
//! kernel queue overflows instead, and the loss becomes invisible — and never grows. It drops,
//! counts, and remembers the first and last instant it dropped at, so the interval turns into a
//! [`TemporalGap`] that reads exactly as §43.2's own example does:
//!
//! ```text
//! coverage gap
//!   source    linux.netlink
//!   reason    dropped events
//!   interval  14:03:12.100 .. 14:03:13.411
//! ```
//!
//! `dropped events` is the gap's `detail`; its `reason` is `source_disconnected`, which is the
//! closed §7.5 word for a subscription that stopped delivering mid-interval (ADR-0643).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use jiff::Timestamp;
use ono_spatial_core::SpatialScope;
use ono_temporal_core::{EvidenceSource, GapReason, TemporalEvent, TemporalGap};
use tokio::sync::mpsc;

use crate::source::SourceProfile;

/// What §43.2's overflow gap says beside its reason.
pub const DROPPED_EVENTS: &str = "dropped events";

/// What became of one offered event (§43.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The event is in the bounded queue.
    Accepted,
    /// The queue was full, so the event is gone and the interval is a gap.
    Dropped {
        /// How many this source has now lost, including this one.
        dropped_so_far: u64,
    },
}

impl Admission {
    /// Whether the event reached the queue.
    #[must_use]
    pub const fn is_accepted(self) -> bool {
        matches!(self, Admission::Accepted)
    }
}

/// The interval one source has been losing events over (§43.2).
#[derive(Debug, Default)]
struct Overflow {
    from: Option<Timestamp>,
    until: Option<Timestamp>,
}

/// One source's bounded ingestion path (§43.1).
#[derive(Debug)]
pub struct Intake {
    source: EvidenceSource,
    capability: std::sync::Arc<str>,
    scope: SpatialScope,
    capacity: usize,
    sender: mpsc::Sender<TemporalEvent>,
    dropped: AtomicU64,
    overflow: Mutex<Overflow>,
}

/// The recorder's end of one source's bounded queue.
#[derive(Debug)]
pub struct IntakeQueue {
    receiver: mpsc::Receiver<TemporalEvent>,
}

impl Intake {
    /// A bounded path for `profile`, holding `capacity` events (§43.1).
    ///
    /// A capacity of zero is raised to one: a queue nothing fits in would report every event as
    /// lost, which is a gap covering everything and evidence of nothing.
    #[must_use]
    pub fn bounded(
        profile: &SourceProfile,
        scope: &SpatialScope,
        capacity: usize,
    ) -> (Self, IntakeQueue) {
        let capacity = capacity.max(1);
        let (sender, receiver) = mpsc::channel(capacity);
        (
            Self {
                source: profile.source.clone(),
                capability: profile.existence_capability(),
                scope: scope.clone(),
                capacity,
                sender,
                dropped: AtomicU64::new(0),
                overflow: Mutex::new(Overflow::default()),
            },
            IntakeQueue { receiver },
        )
    }

    /// A bounded path at the capacity the profile declares.
    #[must_use]
    pub fn for_profile(profile: &SourceProfile, scope: &SpatialScope) -> (Self, IntakeQueue) {
        Self::bounded(profile, scope, profile.intake_capacity)
    }

    /// Offers one event, without waiting and without growing (§43.1, §43.2).
    ///
    /// A provider is never held: §32.5 asks for batching within the flush interval and §43.2 for
    /// a gap rather than pretended continuity, and both are better served by losing an event
    /// loudly than by stalling the source that produced it.
    pub fn offer(&self, event: TemporalEvent, at: Timestamp) -> Admission {
        match self.sender.try_send(event) {
            Ok(()) => Admission::Accepted,
            Err(_) => {
                let dropped_so_far = self.dropped.fetch_add(1, Ordering::Relaxed) + 1;
                if let Ok(mut overflow) = self.overflow.lock() {
                    overflow.from.get_or_insert(at);
                    overflow.until = Some(at);
                }
                Admission::Dropped { dropped_so_far }
            }
        }
    }

    /// How many events this path holds at once (§43.1).
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// How many events this source has lost since the recorder started.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// The §7.1 source this path carries.
    #[must_use]
    pub const fn source(&self) -> &EvidenceSource {
        &self.source
    }

    /// §43.2's explicit coverage gap, or `None` where nothing has been lost.
    ///
    /// The interval is the first drop to the last, which is what nothing can be said about. A
    /// gap running to the present would claim loss over an interval the queue has since been
    /// keeping up with.
    #[must_use]
    pub fn overflow_gap(&self) -> Option<TemporalGap> {
        let overflow = self.overflow.lock().ok()?;
        let from = overflow.from?;
        let until = overflow.until.unwrap_or(from);
        Some(TemporalGap {
            scope: self.scope.clone(),
            from,
            until,
            capability: std::sync::Arc::clone(&self.capability),
            reason: GapReason::SourceDisconnected,
            source: self.source.clone(),
            detail: Some(std::sync::Arc::from(DROPPED_EVENTS)),
        })
    }

    /// Closes the current overflow interval, keeping the count of what was lost.
    ///
    /// A queue that has caught up covers the present again, and §43.2's gap is about the interval
    /// that was lost rather than about every interval after it.
    pub fn clear_overflow(&self) {
        if let Ok(mut overflow) = self.overflow.lock() {
            overflow.from = None;
            overflow.until = None;
        }
    }
}

impl IntakeQueue {
    /// The next event, or `None` once every [`Intake`] for this queue has gone.
    pub async fn recv(&mut self) -> Option<TemporalEvent> {
        self.receiver.recv().await
    }

    /// The next event if one is already there, without waiting.
    pub fn try_recv(&mut self) -> Option<TemporalEvent> {
        self.receiver.try_recv().ok()
    }

    /// Takes up to `limit` events that are already there, for §32.5's batched commit.
    pub fn drain(&mut self, limit: usize) -> Vec<TemporalEvent> {
        let mut batch = Vec::new();
        while batch.len() < limit {
            match self.receiver.try_recv() {
                Ok(event) => batch.push(event),
                Err(_) => break,
            }
        }
        batch
    }
}
