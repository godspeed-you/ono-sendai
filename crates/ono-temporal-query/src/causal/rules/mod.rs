//! The ten built-in rules of `docs/contracts/temporal/causality.yaml`.
//!
//! Seven emit a causal class and three emit `correlated_with`. The division is the one §15.5
//! draws and it is not a spectrum: a correlation rule cannot be promoted into a causal one by
//! finding more instances of itself, and no rule in either list joins on two events being close
//! together in time.

mod action;
mod correlation;
mod process;
mod provider;
mod systemd;

use ono_temporal_core::EvidenceSource;

use crate::causal::rule::{CausalRule, SourceConstraint};

pub use action::{ActionLaunchedProcess, ActionToTransaction};
pub use correlation::{ChangeBeforeFailure, RemoteEndpointRetrySpike, ResourcePressureOverlap};
pub use process::{ProcessParent, ServiceControlsProcess};
pub use provider::ProviderCausalToken;
pub use systemd::{SystemdJobResult, SystemdJobToUnitState};

/// The seven causal rules, in the order the registry declares them.
#[must_use]
pub fn causal() -> Vec<Box<dyn CausalRule>> {
    vec![
        Box::new(ActionToTransaction),
        Box::new(SystemdJobResult),
        Box::new(SystemdJobToUnitState),
        Box::new(ProcessParent),
        Box::new(ServiceControlsProcess),
        Box::new(ActionLaunchedProcess),
        Box::new(ProviderCausalToken),
    ]
}

/// The three correlation rules, in the order the registry declares them.
#[must_use]
pub fn correlation() -> Vec<Box<dyn CausalRule>> {
    vec![
        Box::new(ChangeBeforeFailure),
        Box::new(ResourcePressureOverlap),
        Box::new(RemoteEndpointRetrySpike),
    ]
}

/// The source constraints of a registry row, parsed.
pub(crate) fn constraints(patterns: &[&str]) -> Vec<SourceConstraint> {
    patterns.iter().map(|p| SourceConstraint::new(p)).collect()
}

/// Whether any of `constraints` admits `source`.
pub(crate) fn accepts(constraints: &[SourceConstraint], source: &EvidenceSource) -> bool {
    constraints
        .iter()
        .any(|constraint| constraint.matches(source))
}
