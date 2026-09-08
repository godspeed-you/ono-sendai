//! Live spatial state (spec v0.4 §25, §45.5).
//!
//! §45.5 gives this crate five responsibilities — "provider event merge, snapshot diff, change
//! state, landmark recalculation triggers, live map update messages" — and §25 fixes what they
//! must and must not do:
//!
//! - **Change comes from the machine, never from a timer.** §25.2 forbids artificial delay or
//!   activity and §2.12 makes it an invariant: "Motion and visual updates MUST correspond to
//!   actual topology or metric changes." So a comparison of two identical projections yields
//!   nothing at all, and there is no code path here that produces a change without a difference.
//! - **The source is part of the answer.** §25.3 fixes five words for how fresh a live view is —
//!   `event-driven`, `polled`, `cached`, `stale`, `partial` — and §25.4 requires a change built
//!   by comparing snapshots to say so. Both travel with every [`ChangeSet`].
//! - **The events are the ones the shell already has.** §2.16 forbids the spatial layer from
//!   becoming a second source of system truth, so [`EventMerge`] reads the envelope of the v0.2
//!   watch runtime (v0.2 §18.2, ADR-0024) rather than defining a spatial event of its own.
//!
//! - **A change says when it was seen, and how precisely that is known.** v0.5 §18.1 forbids a
//!   live diff model and a historical event model with incompatible semantics, and §39.1 keeps
//!   this crate's job while requiring its canonical output to be ingestible into the temporal
//!   event path. So every [`SpatialChange`] carries an [`ObservedAt`]: the instant a provider
//!   stated, or — for a difference found by comparing two observations — the interval between
//!   them, because v0.5 §9.2 forbids claiming a moment nothing was observed at.
//!
//! Nothing in here reaches a provider, a terminal or a clock: a caller hands in two projections
//! or a stream of events, each carrying the instant it was observed at, and gets back what
//! differs and when it was seen. That is what makes a live view testable against a real change
//! instead of against a frame counter (§43.6), and it is v0.5 §39.2's rule that pure comparison
//! logic takes time as a parameter rather than reading a clock.

mod change;
mod merge;
mod snapshot;

pub use change::{
    ChangeKind, ChangeSet, ChangeSource, Freshness, ObservationWindow, ObservedAt, SpatialChange,
};
pub use merge::{EventKind, EventMerge, EventSource, ObservedEvent};
pub use snapshot::{MapSnapshot, PlaceSnapshot, compare, compare_places};
