//! Repository automation, as a library so its rules can be tested.
//!
//! `cargo xtask <task>` is the single entry point an agent uses to verify its work. The checks
//! it performs decide whether a green tree means anything, so they are unit-tested against
//! fixtures rather than trusted to be right.

#![forbid(unsafe_code)]

pub mod bindings;
pub mod conformance;
pub mod contracts;
pub mod narrative;
pub mod perf;
pub mod provenance;
pub mod reference;
pub mod scan;
pub mod supply_chain;
pub mod terminology;
pub mod verification;
