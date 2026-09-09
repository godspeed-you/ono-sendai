//! Recovery, as the v0.6 pipeline that decides what putting the state back would cost (spec §24,
//! §25, §26, §27.4, §29.4, Appendix C, Appendix F).
//!
//! §24.1 makes `recover` a planner: it produces a [`ono_change_core::RecoveryPlan`] and changes
//! nothing. §24.2 says why. Since the original plan ran, new files, new writes, later package
//! updates, newer snapshots, new users and unrelated application data may exist, and a naive
//! rollback destroys them — which is why §62.8 names "recovery without drift analysis" as a
//! failure mode by name. The central output of this crate is therefore not *how* to restore but
//! *what restoring would cost*.
//!
//! `ono-change-core` holds the vocabulary; this crate holds the machinery that fills it in, and
//! §50.1's division is why there are two: nothing in the core reads the world, and nothing here
//! does either. Every function takes `now` and its observations as parameters, so Appendix C.4's
//! worked example and Appendix I.5's acceptance scenario can be written down rather than staged on
//! a live filesystem.
//!
//! The modules run in the order a recovery is decided:
//!
//! 1. [`method`] asks the providers that hold the assets what they could do, then applies
//!    Appendix C.1's least-destructive order through [`ono_change_core::choose_method`], dropping
//!    to a lower item only where an upper one cannot preserve the metadata or the application
//!    semantics the goal requires (Appendix C.7).
//! 2. [`conflict`] classifies every object that changed since the asset was captured, inside the
//!    candidate restore scope, as preserved, discarded, conflicting or unknown (Appendix C.3).
//! 3. [`builder`] assembles both into a sealed recovery [`ono_change_core::ChangePlan`] with
//!    RECOVER-role actions, verification contracts and its own risk — §2.12 puts a recovery
//!    through the same lifecycle as any other change.
//! 4. [`gate`] turns the analysis into §24.5's explicit acceptance, or into the refusal that names
//!    every object the recovery would destroy.
//! 5. [`verify`] answers §25's question per equivalence domain, and [`auto`] answers §26.3's.
//! 6. [`remote`] plans §29.4's per-host recovery, and says which hosts it cannot proceed on.
//!
//! # The invariants this crate keeps
//!
//! - **Planning recovery mutates nothing.** No function here calls
//!   [`ono_change_core::RecoveryProvider::restore`], `create` or `cleanup`; the only provider
//!   method it reaches for is `plan_recovery`, which §12.1 defines as the planning half (§24.1,
//!   §55.8 case 35).
//! - **A restore never claims to reverse what left the machine.** An `EffectKind::Emit` is an
//!   unrecoverable effect of the recovery plan even when a provider offers a compensating action,
//!   because §35.3 makes compensation `COMPENSATABLE` and §27.4 forbids the other word.
//! - **An unestablished fact blocks rather than resolves.** An object whose current state could
//!   not be observed is `NewerStateClass::Unknown`, and [`gate::check`] refuses on it before it
//!   ever consults the operator's acceptance (§56.3).
//! - **No sentence here claims the world came back.** §25.3 forbids the global claim, and
//!   `tests/language.rs` greps this crate's own sources to keep it forbidden.

#![forbid(unsafe_code)]

pub mod auto;
pub mod builder;
pub mod conflict;
pub mod gate;
pub mod method;
pub mod remote;
pub mod verify;

pub use auto::admits_auto_recovery;
pub use builder::{RecoveryRequest, plan_recovery};
pub use conflict::{ConflictRequest, ObjectObservation, ObservedState, analyse};
pub use gate::{conflicting_objects, destroyed_objects, unestablished_objects};
pub use method::{
    MethodOffer, MethodOffers, MethodRejection, MethodRequest, MethodSelection, RejectedMethod,
    offers, select,
};
pub use remote::{HostObservation, HostRecovery, HostState, RemoteRecovery, plan_hosts};
pub use verify::{outcome_of, recovery_failed, refusal_for, results, verify};
