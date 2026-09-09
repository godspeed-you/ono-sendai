//! Temporal query planning for Ono-Sendai (spec v0.5 §11, §13, §16, §20, §27, §39).
//!
//! §39 gives this crate four responsibilities — "timeline planning, changes, event search,
//! why/causal graph planning, relevance ranking" — and the rest of the specification fixes what
//! they may and may not do:
//!
//! - **Nothing here reads a clock, a provider, a database or a network.** A caller hands in a
//!   [`ono_temporal_core::LedgerRead`], a reconstruction engine and the instant the question is
//!   asked at (§39.2, §39.3). That is what makes a timeline testable against an in-memory ledger
//!   with no file and no machine.
//! - **The answer is typed values, and the renderer is somewhere else.** §11.4 requires
//!   `timeline` to be a stream a pipeline filters, and §19 requires the full-screen view to be a
//!   presentation over that stream rather than the data contract itself.
//! - **Causation is stricter than correlation, and the code cannot blur them.** §15.2 permits
//!   `caused_by` only under a registered rule with the evidence that rule requires; §16.6 forbids
//!   moving a correlated event into a known cause because it looks plausible; §55.3 names the
//!   renderer that does it anyway as the failure this whole crate exists against.
//!
//! The module boundary is the same one the specification draws. [`timeline`], [`changes`],
//! [`search`], [`relevance`], [`landmark`] and [`merge`] answer *what happened*; [`causal`]
//! answers *why*, and it may answer `unknown` — §16.7 makes that a successful result rather than
//! an error.

pub mod causal;
pub mod changes;
pub mod landmark;
pub mod merge;
pub mod relevance;
pub mod search;
pub mod timeline;
