//! The one place a v0.6 value becomes an Ono value (spec §46).
//!
//! Filled in by the schema increment.

/// The schema ids §46 names, in the order it lists them.
pub const SCHEMAS: &[&str] = &[
    "ono.change-plan/1",
    "ono.plan-action/1",
    "ono.proposed-effect/1",
    "ono.recovery-asset/1",
    "ono.recovery-plan/1",
    "ono.verification-result/1",
    "ono.impact-graph/1",
];
