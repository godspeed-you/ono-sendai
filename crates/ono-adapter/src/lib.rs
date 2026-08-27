//! The External Command Adaptation Layer (spec v0.3).
//!
//! Adapters give familiar Unix commands structured output without taking away their Unix
//! identity. Everything here is planning vocabulary: what a consumer demands of a child's stdout,
//! how an adapter answers, and what it promises about the values it produces. Spawning stays in
//! `ono-process`; adapters describe semantics (spec v0.3 §1.7).

mod demand;

pub use demand::{Consumer, OutputDemand, Stdout};
