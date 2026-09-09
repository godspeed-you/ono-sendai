//! The canonical temporal vocabulary of Ono-Sendai (spec v0.5 §3, §39).
//!
//! §39 splits the temporal system across six crates and gives this one the words the other five
//! speak in: "event IDs and event model, evidence, coverage, causal link types, time selectors,
//! temporal context value types". Everything here is a value or a rule over values, and the two
//! prohibitions §39 attaches to that are what shape the crate:
//!
//! - **No clock.** §39.2 keeps the system clock out of pure logic, so [`TimeSelector::resolve`]
//!   takes `now` and the zone as parameters and nothing in this crate calls
//!   `Timestamp::now()`. A historical answer is then reproducible, which is what makes §47's
//!   temporal tests possible at all.
//! - **No store.** §39.4 forbids SQLite types and SQL semantics in the core types, so
//!   [`LedgerRead`] and [`LedgerWrite`] name events, evidence, coverage and checkpoints and
//!   never a connection, a statement or a file. `ono-temporal-reconstruct` and
//!   `ono-temporal-query` therefore compile against the contract rather than against a database,
//!   and [`SessionLedger`] — §10.7's bounded in-memory ledger — satisfies the same contract.
//!
//! Three further rules of the specification are enforced by the shape of the types rather than
//! by review:
//!
//! - **Three timestamps stay three.** §3.3: `source_time`, `observed_at` and `ingested_at`
//!   "MUST NOT be silently collapsed into one field". [`EventTimes`] keeps them apart and
//!   [`EventTimes::presentation_instant`] is the one place a single instant is chosen.
//! - **Wall time never orders.** §26.1 supports happens-before with "stronger evidence than wall
//!   time", so [`happens_before`] answers from a source sequence or a monotonic clock inside one
//!   [`ClockDomain`] and returns [`Ordering::Concurrent`] otherwise.
//! - **Nothing raises evidence.** §7.2: strength "MUST NOT be automatically upgraded by
//!   renderers, AI assistants or plugins". [`EvidenceStrength`] therefore has
//!   [`EvidenceStrength::weakest_of`] and no counterpart.
//!
//! # Where a temporal value becomes an Ono value
//!
//! [`value`] is the single bridge from these types to the schemas of §35, and [`error`] the
//! single source of the fourteen refusals of §34. No other crate spells a temporal field name or
//! an error code by hand.

#![forbid(unsafe_code)]

mod action;
mod causal;
mod clock;
mod context;
mod coverage;
mod digest;
mod event;
mod evidence;
mod id;
mod ledger;
mod order;
mod session;
mod source;
mod time;

pub mod error;
pub mod value;

pub use action::{
    ActionEvent, ActionOutcome, ActionResultSummary, AuthorizationDecision, AuthorizationSummary,
    REDACTED, Redactable, RedactedCommandSummary,
};
pub use causal::{CausalLink, CausalRelation, CausalRuleId};
pub use clock::{ClockDomain, EventTimes};
pub use context::TemporalContext;
pub use coverage::{
    CoverageSummary, GapReason, HeadlineCoverage, TemporalCompleteness, TemporalCoverage,
    TemporalGap, gap_detail,
};
pub use event::{
    CONFIDENCE_KEY, ChangeCertainty, ChangeClass, EventKind, EventSeed, FieldChange, RELATION_KEY,
    SpatialRef, TemporalEvent, relation_of, relation_payload,
};
pub use evidence::{Evidence, EvidenceClaim, EvidenceStrength, OpaqueReference};
pub use id::{ActionId, CausalLinkId, CheckpointId, EventId, EvidenceId};
pub use ledger::{
    Appended, Checkpoint, CoverageQuery, EventQuery, LedgerRead, LedgerWrite, ObjectState,
    QueryOrder, RelationState, RetentionState, SourceAvailability, TimeRange,
};
pub use order::{OrderEvidence, Ordering, happens_before, presentation_order};
pub use session::{DEFAULT_SESSION_MAX_EVENTS, SessionLedger};
pub use source::{EvidenceSource, TemporalCapabilities, TemporalSourceDescription};
pub use time::{EventAnchors, TimeResolution, TimeSelector};
