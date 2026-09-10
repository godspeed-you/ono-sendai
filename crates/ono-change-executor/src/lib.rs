//! The prepare, apply and verify state machine of Ono-Sendai v0.6 (spec §4, §23, §28, §41, §42,
//! Appendix F).
//!
//! Everything in this crate exists to make one distinction operable: whether the system was
//! changed. §2.3 forbids mutation once a required recovery asset could not be created, §4.1 draws
//! no edge from `prepare-failed` to `applying`, and
//! [`PlanState::has_mutated`](ono_change_core::PlanState::has_mutated) is the predicate an
//! operator's whole reading of a failure hangs on. Appendix F's thirteen rows are that distinction
//! written out per failure point, and [`FailurePoint`] is those rows made addressable, so a test
//! and a renderer both name the same boundary.
//!
//! # The order in `apply`, and why it is fixed
//!
//! [`apply`] runs claim, revalidate, capability, gate, prepare, mutate, verify in that order, and
//! each step's failure means something different (§42.4, §7.3, §43.2, §19.4, §4.5, §4.7, §4.8).
//! Everything before `prepare` refuses with the plan still `SEALED` and nothing created; a prepare
//! failure reaches `PREPARE_FAILED` with assets possibly created and the targets untouched; only
//! from `APPLYING` onwards may the operator be told something happened.
//!
//! [`apply`] answers with an [`ApplyOutcome`] rather than a `Result`, because Appendix F's point is
//! that "it failed" is several different facts: the final state, the per-action statuses, the
//! assets that exist, the targets that were touched, and the structured error where there is one.
//!
//! # Nothing here reaches the outside world
//!
//! `now`, the plan store, the provider registry and the functions that revalidate, execute and
//! observe are all parameters. No module calls `Timestamp::now()`, opens a socket or runs a
//! program, which is what lets §54.5's failure injection be a scripted function rather than a
//! broken machine.
//!
//! # What each module owns
//!
//! - [`execute`] — §4.5 to §4.9, §5.5 to §5.7, §18, §19.4, §23, §40, §42, §43, Appendix F.
//! - [`strategy`] — §28.4's waves and §28.6's canary gate.
//! - [`mod@resume`] — §41.2's reconstruction and §41.3's per-action decision.
//! - [`events`] — §22.1's thirteen ledger events and §22.2's pre-plan checkpoint.

#![forbid(unsafe_code)]

pub mod events;
pub mod execute;
pub mod resume;
pub mod strategy;

pub use events::{PlanLifecycle, checkpoint_before_mutation};
pub use execute::{
    ApplyOutcome, ApplyRequest, Authority, CleanupDecision, CleanupReport, CloseOutcome,
    CloseRequest, ExecutionOutcome, FailurePoint, Observation, PrepareRequest, Prepared,
    ProtectionShortfall, QuiesceReport, Quiescing, VerifyOutcome, VerifyRequest, apply, close,
    prepare, verify,
};
pub use resume::{BlockedAction, ResumeDecision, ResumeOutcome, resume, resume_with};
pub use strategy::{StrategyRun, TargetResult, run_waves};
