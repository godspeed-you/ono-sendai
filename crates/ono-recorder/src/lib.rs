//! The Ono-Sendai temporal recorder (spec v0.5 §10, §22, §31.9, §43, §44).
//!
//! §10.1 states what this is for in one sentence, and the sentence is a boundary as much as a
//! purpose: the recorder "exists to retain enough local system history for Ono's temporal
//! interface to remain useful across shell sessions. It is **not** a metrics platform, log
//! archive or general monitoring product."
//!
//! # The asymmetry this component exists to keep
//!
//! Nothing else in the temporal system reads the clock or touches the store. §39.2 keeps
//! `Timestamp::now()` out of `ono-temporal-core`, `-reconstruct` and `-query` so a historical
//! answer is reproducible; §39.4 keeps SQL out of them so they compile against a contract rather
//! than a database. The recorder is where both of those become somebody's job, which is why every
//! instant here is a parameter of the call that needed it and every timer is a caller's turn of
//! [`Recorder::maintenance`] rather than a thread this crate started.
//!
//! # The four things a recorder owes the record
//!
//! - **Bounded ingestion, and an honest gap when it overflows.** §43.1 prohibits an unbounded
//!   channel and §43.2 prefers "an explicit coverage gap over pretending continuity". An
//!   [`Intake`] drops rather than grows and remembers the interval it dropped over, which becomes
//!   a [`TemporalGap`](ono_temporal_core::TemporalGap) reading `dropped events`.
//! - **Coverage nobody may overstate.** A [`SourceProfile`] that is polled cannot claim
//!   `exhaustive_events` (§21.5) and its coverage carries the sampling interval §22.1 requires. A
//!   missing object becomes a disappearance only where §6.3 supports one.
//! - **Downtime that is a gap rather than a join.** §44.1's five steps run in order at every
//!   start, and the interval between the last event and the restart is filed under the
//!   `<type>.existence` capability a reconstruction gates object presence on.
//! - **A privacy floor that is structural.** §10.6's second list is enforced by
//!   [`Redaction`] before a record can become an event at all, so there is no path by which an
//!   unredacted argv, environment or body reaches the ledger.
//!
//! # What the command layer calls
//!
//! ```no_run
//! use ono_recorder::{Recorder, RecorderOptions, RecorderSettings};
//! # fn main() -> Result<(), ono_value::ErrorValue> {
//! # let scope = ono_spatial_core::SpatialScope::host(
//! #     "workstation",
//! #     ono_spatial_core::BootIdentity::new("workstation", "boot-a"),
//! # );
//! # let domain = ono_temporal_core::ClockDomain::new("workstation", Some("boot-a"));
//! # let now = jiff::Timestamp::UNIX_EPOCH;
//! let recorder = Recorder::new(
//!     RecorderOptions::new(scope, domain).with_store("/home/case/ledger.sqlite3".into()),
//! );
//!
//! let started = recorder.start(&RecorderSettings::default(), now)?;   // `start recorder`
//! let status = recorder.status(now).to_record()?;                     // `get recorder`
//! let stopped = recorder.stop(now)?;                                  // `stop recorder`
//! # let _ = (started, status, stopped);
//! # Ok(())
//! # }
//! ```
//!
//! Decisions: ADR-0640 (idempotency and E1314), ADR-0641 (what the recorder drives),
//! ADR-0642 (the privacy floor), ADR-0643 (the overflow gap), ADR-0644 (the user service).

#![forbid(unsafe_code)]

mod aggregate;
mod checkpoint;
mod controller;
mod ingest;
mod normalize;
mod redact;
mod settings;
mod snapshot;
mod source;
mod status;

pub mod downtime;
pub mod privilege;
pub mod service;

pub use aggregate::{
    AggregationRules, Aggregator, DEFAULT_AGGREGATED_FIELDS, DEFAULT_AGGREGATION_WINDOW,
};
pub use checkpoint::{CheckpointReason, CheckpointSchedule, is_significant};
pub use controller::{
    CheckpointCapture, CheckpointOutcome, Maintenance, PendingCheckpoint, Recorder,
    RecorderOptions, STALL_INTERVALS, StartOutcome,
};
pub use downtime::RestartPlan;
pub use ingest::{Admission, DROPPED_EVENTS, Intake, IntakeQueue};
pub use normalize::{
    Normalizer, ResolvedSubject, SNAPSHOT_DIFF, SubjectResolver, UnresolvedSubjects, gap_of,
    gap_payload, kind_of,
};
pub use privilege::{PROHIBITIONS, PrivilegeReport};
pub use redact::{ARGV_FIELDS, Redaction, WITHHELD_FIELDS};
pub use settings::{
    DEFAULT_CHECKPOINT_INTERVAL, DEFAULT_FLUSH_INTERVAL, DEFAULT_INTAKE_CAPACITY, DEFAULT_MAX_AGE,
    DEFAULT_MAX_SIZE, DEFAULT_MAX_SWEEP_PASSES, DEFAULT_SESSION_MAX_EVENTS, RecorderSettings,
    SETTING_CHECKPOINT_INTERVAL, SETTING_ENABLED, SETTING_FLUSH_INTERVAL, SETTING_MAX_AGE,
    SETTING_MAX_SIZE, SETTING_PROCESS_ARGV, SETTING_SESSION_MAX_EVENTS, SettingDifference,
};
pub use snapshot::{MISSED_ROUNDS_BEFORE_GAP, SnapshotDiff, SnapshotRound};
pub use source::{Delivery, SourceProfile};
pub use status::{RecorderHealth, RecorderStatus};
