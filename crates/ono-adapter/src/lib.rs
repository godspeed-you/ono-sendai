//! The External Command Adaptation Layer (spec v0.3).
//!
//! Adapters give familiar Unix commands structured output without taking away their Unix
//! identity. Everything here is planning vocabulary: what a consumer demands of a child's stdout,
//! how an adapter answers, and what it promises about the values it produces. Spawning stays in
//! `ono-process`; adapters describe semantics (spec v0.3 §1.7).

mod demand;

pub use demand::{Consumer, OutputDemand, Stdout};

/// The stage keyword that bypasses adaptation: `raw <program> [arguments]` (spec v0.3 §1.17,
/// ADR-0054). The program runs with no argv rewrite, no decoder, no renderer and its own exit
/// status.
pub const RAW: &str = "raw";

/// The stage keyword that forces adaptation: `adapt <program> [arguments]` (spec v0.3 §1.18,
/// ADR-0054). The stage fails rather than downgrade to text when no adapter can answer.
pub const ADAPT: &str = "adapt";
