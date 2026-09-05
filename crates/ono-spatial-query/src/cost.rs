//! The spatial query cost model (v0.4.1 §34.1, §34.2, §33.3).
//!
//! §34.1 asks for one thing and sets one bar:
//!
//! > The spatial planner SHOULD compute a coarse cost estimate before expanding expensive
//! > relationships. … It need not be mathematically exact. It MUST be conservative enough to
//! > avoid obviously explosive work.
//!
//! So this is not a performance model and it is not a predictor of milliseconds. It is a number
//! large enough to notice when a query is about to do something nobody meant, and §33.3 says what
//! to do then:
//!
//! > If the planner predicts cost beyond the supported interactive budget, Ono MUST refuse or
//! > switch to a bounded lower-detail strategy rather than silently appear hung.
//!
//! The unit is deliberately abstract — a *candidate acquisition*, weighted by
//! [`AcquisitionCost`]. Comparing it to a wall clock would make the budget a property of the
//! machine, which is the mistake §32.4 exists to prevent. Decisions: ADR-0494.

use ono_core::ErrorCode;
use ono_spatial_core::AcquisitionCost;
use ono_value::ErrorValue;

/// The most a query may estimate before an interactive path refuses it (§33.3, §34.1).
///
/// It is a ceiling on obviously explosive work rather than a latency target: a thousand
/// candidates at moderate cost with a fan-out of four is 20 000 units and is answered, and two
/// hundred thousand is not. §33.2's latency targets are measured on the reference environment and
/// recorded in `docs/contracts/hardening/performance_baseline.json`; this number is what stops a query
/// nobody could have wanted, on any machine.
pub const INTERACTIVE_BUDGET: u64 = 250_000;

/// v0.4.1 §34.1's coarse estimate.
///
/// The inputs are four of the six §34.1 lists — candidate node count, expected edge fan-out, the
/// relationship acquisition cost class, and the requested depth. The two it leaves out, selector
/// selectivity and cache state, only ever make a query *cheaper* than this says, which is the
/// direction §34.1's "conservative" points in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostEstimate {
    candidates: u64,
    fan_out: u64,
    class: AcquisitionCost,
    depth: u32,
    requested: bool,
}

impl CostEstimate {
    /// An estimate for `candidates` nodes with `fan_out` edges each, acquired at `class`, to
    /// `depth` hops.
    #[must_use]
    pub fn new(candidates: usize, fan_out: usize, class: AcquisitionCost, depth: u32) -> Self {
        Self {
            candidates: candidates as u64,
            fan_out: fan_out as u64,
            class,
            depth: depth.max(1),
            requested: false,
        }
    }

    /// The same estimate, for a caller who asked for the expensive relation by name (§34.3).
    ///
    /// §34.3 requires a request path to exist for anything described as "available on request".
    /// This is that path at the planning layer: an estimate the caller has accepted is not
    /// refused, because the refusal exists to stop work nobody asked for.
    #[must_use]
    pub fn requested(self) -> Self {
        Self {
            requested: true,
            ..self
        }
    }

    /// How many candidate acquisitions the query is estimated to make.
    #[must_use]
    pub fn units(&self) -> u64 {
        self.candidates
            .saturating_mul(self.fan_out.max(1))
            .saturating_mul(u64::from(self.depth))
            .saturating_mul(self.class.weight())
    }

    /// How many nodes the query would consider.
    #[must_use]
    pub fn candidates(&self) -> u64 {
        self.candidates
    }

    /// The dominating acquisition class (§34.2).
    #[must_use]
    pub fn class(&self) -> AcquisitionCost {
        self.class
    }

    /// Whether the estimate is beyond `budget` and the caller did not ask for it anyway.
    #[must_use]
    pub fn exceeds(&self, budget: u64) -> bool {
        !self.requested && self.units() > budget
    }
}

/// The refusal §34.1 and §33.3 require, naming the estimate rather than saying "too expensive".
///
/// §53.1 makes a stable error part of automation, so the code is `Ono-Sendai-E1401` and the
/// message carries both figures a caller would act on: how much was estimated, and how many
/// candidates it was estimated over.
#[must_use]
pub fn refusal(estimate: &CostEstimate, operation: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::SpatialCostRefused,
        format!(
            "`{operation}` would acquire about {} {} relations over {} candidates, which is \
             beyond the interactive budget of {INTERACTIVE_BUDGET}",
            estimate.units(),
            estimate.class.as_str(),
            estimate.candidates
        ),
    )
    .with_help(
        "v0.4.1 §33.3: a query the planner predicts beyond the interactive budget is refused \
         rather than left to look hung. Narrow the place, lower the depth, or ask for the \
         expensive relation explicitly with `--all`, which says the cost is acceptable (§34.3).",
    )
}
