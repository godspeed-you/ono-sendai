//! Plan construction, target freezing, drift revalidation and the persistent plan store
//! (spec v0.6 §4, §5, §6, §7, §36, §41, §42).
//!
//! `ono-change-core` says what a plan *is*. This crate says how one comes into being, how it stays
//! the plan it was, and where it lives between two runs of the shell. Four rules shape every
//! module here, and each of them is a sentence of the specification rather than a convention:
//!
//! - **A plan freezes identities, never selectors** (§4.3, §2.6). [`freeze`] composes the identity
//!   §7.1 asks for — canonical path plus persistence domain plus inode for a file, provider
//!   namespace plus unit for a service, host for a remote target — and keeps the selector only as
//!   provenance. Nothing re-runs it, so a fifth service that starts failing after resolution does
//!   not join the plan.
//! - **Unknown blocks** (§2.4, §7.3). [`drift`] treats a precondition nobody could observe as a
//!   reason to stop, and an [`ono_change_core::ActionStatus::Unknown`] read back out of the store
//!   comes back unknown rather than as failure or success (Appendix F.2).
//! - **A sealed plan is immutable** (§4.4, §7.5). [`rebase`] takes the original by reference and
//!   returns a new revision, so "rebase MUST NOT mutate the sealed original" is a property of the
//!   signature.
//! - **Two sessions do not apply one plan** (§42.4). [`store`] hands out an exclusive, leased
//!   [`store::Claim`]; §42.3 bounds it, so a session that crashed holding one does not block the
//!   plan forever.
//!
//! # Determinism
//!
//! Every entry point that needs the time takes it as a parameter. Nothing here calls
//! `Timestamp::now`, which is what lets §55's cases be written as tests rather than as
//! observations.
//!
//! # What this crate does not do
//!
//! It resolves no providers. Actions arrive as [`ono_change_core::PlanFragment`]s that a change
//! provider produced, so [`builder::PlanBuilder`] is testable without a world to plan against, and
//! §2.1's "planning is side-effect free" is enforced by a crate that cannot perform I/O outside
//! its own store.

#![forbid(unsafe_code)]

pub mod builder;
pub mod drift;
pub mod freeze;
pub mod historical;
mod migrate;
pub mod path;
pub mod rebase;
pub mod references;
pub mod secrets;
pub mod store;

pub use builder::{
    BlockPlan, BlockStatement, MAX_BLOCK_ACTIONS, OpaqueEscape, PlanBuilder, PlanGranularity,
    external_command,
};
pub use drift::{DriftReport, DriftVerdictSummary, revalidate};
pub use freeze::{FileTarget, RemoteTarget, ServiceTarget};
pub use historical::refuse_in_past;
pub use migrate::STORE_VERSION;
pub use path::{DATABASE_NAME, plan_store_directory, plan_store_path};
pub use rebase::rebase;
pub use references::{Reference, ReferenceKind};
pub use secrets::{HANDLE_PREFIX, SENSITIVE_ARGUMENTS, SecretRedaction};
pub use store::{CLAIM_LEASE, Claim, PlanFilter, PlanStore, PlanSummary, StoreOptions};
