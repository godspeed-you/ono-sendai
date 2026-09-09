//! Historical state reconstruction for Ono-Sendai (spec v0.5 §9, §42).
//!
//! Given a ledger and an instant, this crate answers what was there — and, at least as often,
//! refuses to. §1.3 is the sentence the whole crate is written against: "history is not
//! omniscience". A reconstruction that filled its gaps would be more useful and less true, so
//! every unsupported answer here is a typed refusal rather than a plausible value.
//!
//! # The algorithm
//!
//! [`Reconstructor::reconstruct`] runs §9.1's six steps in the order it gives them: the nearest
//! trusted checkpoint at or before `T`, the ordered events from there through `T`, the
//! provider-owned answers that speak to fields at `T`, identity and relations reconciled through
//! v0.4's rules, coverage and gaps, and typed objects carrying their provenance.
//!
//! # The three refusals
//!
//! - **A value is not carried across an unobserved interval** (§9.2). Between a reading at 12:00
//!   and one at 12:10, the state at 12:05 is [`FieldKnowledge::UnknownInInterval`] with both
//!   readings named. Only a source's own complete coverage over the stretch turns it into a
//!   value, and §9.3's `valid_from`/`valid_until` then come from that coverage or from a
//!   provider's declared interval — never from a midpoint.
//! - **An absence is a claim** (§7.4). [`ReconstructedWorld::can_prove_absence`] gates every one,
//!   and [`Presence`] has a third variant so "nothing observed it" cannot be read as "it was not
//!   there" (§9.5).
//! - **Historical filesystem structure is refused by default** (§14.5). Only a checkpoint, a
//!   filesystem snapshot provider, exhaustive audit evidence or a KUANG/11 provider carries it;
//!   see [`SourceMatrix::structure_support`]. Current directory contents are never the past.
//!
//! # What it does not do
//!
//! It reads no clock: `T` and every window bound is a parameter (§39.2). It reaches no provider
//! and no store: it holds a `&dyn `[`ono_temporal_core::LedgerRead`] and whatever historical
//! answers a caller already obtained (§39.3), which is why the whole engine is testable against
//! [`ono_temporal_core::SessionLedger`] with no file and no database.
//!
//! Decisions: ADR-0650 (unknown intervals), ADR-0651 (unknown against absent), ADR-0652
//! (checkpoint bounds), ADR-0653 (historical path structure), ADR-0654 (replay order).

#![forbid(unsafe_code)]

pub mod capability;

mod answers;
mod checkpoint;
mod collection;
mod engine;
mod field;
mod object;
mod relation;
mod replay;
mod structure;

pub use answers::{HistoricalAnswer, HistoricalAnswers};
pub use checkpoint::{
    CheckpointPolicy, CheckpointProjection, CheckpointRequest, ExcludedClass, is_trusted,
    nearest_trusted, project_checkpoint,
};
pub use collection::ReconstructedCollection;
pub use engine::{ReconstructedWorld, ReconstructionRequest, Reconstructor};
pub use field::{FieldKnowledge, Observation, Presence, ReconstructedField};
pub use object::{
    ReconstructedObject, TEMPORAL_EXTENSION, TEMPORAL_FIELD, UnresolvedSubject, attach_temporal,
    temporal_of,
};
pub use relation::ReconstructedRelation;
pub use replay::{ReplayStep, replay_order, replay_plan};
pub use structure::{SourceMatrix, StructureSupport, is_path_structure};
