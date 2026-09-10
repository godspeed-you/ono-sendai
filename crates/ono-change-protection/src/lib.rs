//! Protection, as the v0.6 pipeline that decides what a plan is actually covered by (spec §10,
//! §17, §18, §37, §38, Appendix A, Appendix B).
//!
//! `ono-change-core` holds the vocabulary; this crate holds the machinery that fills it in, and
//! §50.1's division is the reason there are two: nothing in the core reads a file, and everything
//! here reads the world through values a caller supplies. [`domain`] takes a mount table and a
//! path; [`coverage`] takes mutation domains, resolved persistence domains and a registry. Both
//! are pure functions of their inputs, so Appendix G.2's deliberately misleading layouts can be
//! written down rather than mounted.
//!
//! The order the modules run in is Appendix A's:
//!
//! 1. [`domain::resolve`] maps each target path to the storage object that holds its state
//!    (Appendix B), refusing where a local provider must not claim protection.
//! 2. [`registry::ProviderRegistry`] asks every available recovery provider what it could offer,
//!    and reports the providers it had to skip rather than returning a silently short list
//!    (§12.2, §55.6 case 29).
//! 3. [`coverage::analyse`] derives each domain's recovery objective, ranks the candidates by
//!    Appendix A.4's preference order, and composes the coverage matrix whose plan-level word
//!    `ProtectionSummary::level` computes.
//! 4. [`policy`] decides what that answer means: `require` refuses a shortfall, `maximize` adds
//!    non-conflicting extras inside the cost limits, `off` still shows what was available.
//! 5. [`freshness`], [`retention`] and [`cost`] answer the questions that outlive the plan —
//!    whether an early asset still describes the state about to change (§18.3), when an asset may
//!    be removed and what that removal takes away (§37), and what protection costs (§38).
//!
//! Two invariants are properties of this crate rather than rules a reviewer has to remember:
//!
//! - **A candidate that cannot be restored from never wins.** Appendix A.4's first key is the
//!   required objective, and [`coverage::ranked`] treats a candidate of unknown consistency as
//!   satisfying nothing (§2.4, §39.1).
//! - **Cleanup never silently invalidates a recovery guarantee.**
//!   [`retention::cleanup_preview`] names the plans an asset's removal would strand, and
//!   [`retention::CleanupPreview::refusal_for`] turns that into §2.15's refusal.

#![forbid(unsafe_code)]

pub mod cost;
pub mod coverage;
pub mod domain;
pub mod freshness;
pub mod hosts;
pub mod policy;
pub mod registry;
pub mod retention;
pub mod settings;

pub use coverage::{CoverageAnalysis, CoverageRequest, MutationDomain, analyse};
pub use domain::{DomainReach, MountBoundary, MountInfo, MountTable, recorded_domain, resolve};
pub use freshness::{Freshness, FreshnessVerdict, assess};
pub use policy::{CostLimits, FreeSpaceFloor, Profile, ProtectionPolicy};
pub use registry::{DiscoveryOutcome, ProviderRefusal, ProviderRegistry};
pub use retention::{CleanupPreview, CleanupVerdict, PlanRetention, cleanup_preview};
pub use settings::ChangeSettings;
