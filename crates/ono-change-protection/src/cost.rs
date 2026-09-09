//! What protection costs, and the one word §38.2 forbids (spec §38).
//!
//! §38.1 lists the dimensions — initial latency, retained storage growth, I/O overhead, quiesce
//! duration, reboot or downtime requirement, cleanup cost — and `RecoveryCost` in the core holds
//! them. This module does the two things a plan needs on top: composing the cost of a whole
//! candidate set, and turning one cost into the four lines §38.2 permits.
//!
//! §38.2's rule is short: **Ono MUST NOT display "free" for CoW snapshots.** A copy-on-write
//! snapshot starts at nearly no space and grows with every block the original changes afterwards,
//! so "free" is not an approximation of its cost, it is the wrong shape of statement.
//! [`render`] therefore answers `minimal` where the measured size is zero, and
//! [`CostRendering::claims_free`] exists so a test can assert that no input produces the word.

use std::sync::Arc;
use std::time::Duration;

use ono_change_core::{RecoveryAssetType, RecoveryCost};
use ono_value::ByteSize;

/// The composed cost of a set of candidates or assets (§38.1).
///
/// Space adds up, because two snapshots occupy two lots of space. Creation latency adds up too:
/// PREPARE creates assets one after another, and §18.1's just-in-time protection is the window
/// this number measures. Quiesce takes the longest rather than the sum, because §18.4 bounds one
/// application's pause and two applications pause at once. Anything unmeasured makes the whole
/// composition estimated (§37.5).
#[must_use]
pub fn compose(costs: &[RecoveryCost]) -> RecoveryCost {
    if costs.is_empty() {
        return RecoveryCost::unknown();
    }
    let mut initial: Option<u128> = Some(0);
    let mut retained: Option<u128> = Some(0);
    let mut latency = Duration::ZERO;
    let mut quiesce: Option<Duration> = None;
    let mut estimated = false;
    let mut reboot = false;
    let mut offline = false;
    for cost in costs {
        initial = add(initial, cost.initial_bytes());
        retained = add(retained, cost.retained_bytes());
        latency = latency.saturating_add(cost.creation_latency().unwrap_or(Duration::ZERO));
        if let Some(window) = cost.quiesce() {
            quiesce = Some(quiesce.map_or(window, |longest: Duration| longest.max(window)));
        }
        estimated = estimated
            || cost.is_estimated()
            || cost.initial_bytes().is_none()
            || cost.retained_bytes().is_none();
        reboot = reboot || cost.requires_reboot();
        offline = offline || cost.requires_offline();
    }
    let mut composed = RecoveryCost::unknown()
        .with_space(
            initial.map(ByteSize::from_bytes),
            retained.map(ByteSize::from_bytes),
            estimated,
        )
        .with_latency(latency);
    if let Some(window) = quiesce {
        composed = composed.with_quiesce(window);
    }
    if reboot {
        composed = composed.needing_reboot();
    }
    if offline {
        composed = composed.needing_offline();
    }
    composed
}

fn add(total: Option<u128>, addend: Option<ByteSize>) -> Option<u128> {
    match (total, addend) {
        (Some(total), Some(addend)) => Some(total.saturating_add(addend.bytes())),
        _ => None,
    }
}

/// The four lines §38.2 permits for a recovery asset's cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostRendering {
    initial_creation: Arc<str>,
    initial_space: Arc<str>,
    future_growth: Arc<str>,
    retention_risk: Arc<str>,
}

impl CostRendering {
    /// `initial creation`.
    #[must_use]
    pub fn initial_creation(&self) -> &str {
        &self.initial_creation
    }

    /// `initial space`.
    #[must_use]
    pub fn initial_space(&self) -> &str {
        &self.initial_space
    }

    /// `future growth`.
    #[must_use]
    pub fn future_growth(&self) -> &str {
        &self.future_growth
    }

    /// `retention risk`.
    #[must_use]
    pub fn retention_risk(&self) -> &str {
        &self.retention_risk
    }

    /// The four labelled lines, in §38.2's order.
    #[must_use]
    pub fn lines(&self) -> [(&'static str, &str); 4] {
        [
            ("initial creation", self.initial_creation()),
            ("initial space", self.initial_space()),
            ("future growth", self.future_growth()),
            ("retention risk", self.retention_risk()),
        ]
    }

    /// Whether any line claims the asset is free or costs nothing (§38.2).
    ///
    /// The check is on the rendered text rather than on the numbers, because §38.2's prohibition
    /// is about what a person reads. A test asserts this is false for every asset type and every
    /// cost, including a measured zero.
    #[must_use]
    pub fn claims_free(&self) -> bool {
        self.lines().iter().any(|(_, text)| {
            let text = text.to_ascii_lowercase();
            text.contains("free")
                || text.contains("no cost")
                || text.contains("costs nothing")
                || text == "0 b"
                || text == "zero"
        })
    }
}

/// Renders `cost` for an asset of `asset_type` (§38.2).
///
/// A copy-on-write snapshot gets §38.2's own four phrases, because they are the honest shape of
/// its cost: instant to make, almost nothing at first, growing with every block that changes
/// afterwards, and a moderate risk if it is kept. An independent copy gets a different four: it
/// costs its size up front, it does not grow, and keeping it is cheap. §11.5's distinction is the
/// reason the two are not rendered the same way.
#[must_use]
pub fn render(asset_type: RecoveryAssetType, cost: &RecoveryCost) -> CostRendering {
    let estimated = if cost.is_estimated() {
        ", estimated"
    } else {
        ""
    };
    if asset_type.is_independent_copy() {
        return CostRendering {
            initial_creation: latency_text(cost, "proportional to the data copied"),
            initial_space: Arc::from(match cost.initial_bytes() {
                Some(size) if size.bytes() > 0 => format!("{size}{estimated}"),
                _ => "the size of the captured objects".to_owned(),
            }),
            future_growth: Arc::from("none: the copy is fixed at what it captured"),
            retention_risk: Arc::from("low: the copy is independent of the source (§11.5)"),
        };
    }
    let shares_failure_domain = asset_type.shares_failure_domain();
    CostRendering {
        initial_creation: latency_text(cost, "near-instant"),
        // §38.2: "minimal" is the word for a copy-on-write snapshot's initial space. A measured
        // zero is still minimal rather than free — the space arrives as the original changes.
        initial_space: Arc::from(format!("minimal{estimated}")),
        future_growth: Arc::from("depends on changed blocks"),
        retention_risk: Arc::from(if shares_failure_domain {
            "moderate: the recovery point shares a failure domain with what it protects (§11.5)"
        } else {
            "moderate"
        }),
    }
}

fn latency_text(cost: &RecoveryCost, fallback: &str) -> Arc<str> {
    match cost.creation_latency() {
        Some(latency) if latency >= Duration::from_secs(1) => {
            Arc::from(format!("{} s", latency.as_secs()))
        }
        Some(_) | None => Arc::from(fallback),
    }
}
