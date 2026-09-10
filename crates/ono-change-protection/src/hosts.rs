//! A remote plan's protection, composed from the protection of each host (spec §29.1, §29.2).
//!
//! §29.1 makes truth per host, and §29.2 gives the worked example: a twenty-host plan with twelve
//! ZFS-protected hosts, six Btrfs-protected hosts and two unprotected ones is
//! `PARTIALLY_PROTECTED` "unless policy excludes the unprotected targets". Each host is analysed
//! on its own, by its own providers over its own mount table, because a snapshot on one host
//! covers nothing on another. [`compose`] puts every host's rows into one matrix, and
//! `ProtectionSummary::level` computes the plan-level word over them exactly as it does for a
//! single host, so there is one composition rule rather than two that could drift apart.
//!
//! Two decisions belong here. A host the policy excludes keeps its rows, marked irrelevant, and
//! gains an exclusion that names it, so the matrix still shows what the plan does not cover
//! (§10.3). A host whose protection could not be analysed at all — its link down at plan time
//! (§29.3) — contributes a row whose protection is unknown, so its absence caps the plan
//! (Appendix A.7) and the reachable hosts never speak for it.

use std::sync::Arc;

use ono_change_core::{
    CoverageExclusion, DomainCoverage, DomainProtection, EffectDomain, ProtectionLevel,
    ProtectionSummary, RecoveryObjective,
};

use crate::policy::ProtectionPolicy;

/// What the protection analysis of one host of a remote plan established (§29.1).
#[derive(Debug, Clone, PartialEq)]
pub struct HostCoverage {
    host: Arc<str>,
    analysis: Result<ProtectionSummary, Arc<str>>,
}

impl HostCoverage {
    /// The coverage matrix that was analysed on `host`.
    #[must_use]
    pub fn analysed(host: impl Into<Arc<str>>, summary: ProtectionSummary) -> Self {
        Self {
            host: host.into(),
            analysis: Ok(summary),
        }
    }

    /// A host whose protection could not be analysed, and why (§29.3).
    #[must_use]
    pub fn unestablished(host: impl Into<Arc<str>>, reason: impl Into<Arc<str>>) -> Self {
        Self {
            host: host.into(),
            analysis: Err(reason.into()),
        }
    }

    /// The host.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The host's own coverage matrix, where it could be analysed.
    #[must_use]
    pub fn summary(&self) -> Option<&ProtectionSummary> {
        self.analysis.as_ref().ok()
    }

    /// The host's own protection word (§29.1). A host nobody could analyse is unknown.
    #[must_use]
    pub fn level(&self) -> ProtectionLevel {
        self.analysis
            .as_ref()
            .map_or(ProtectionLevel::Unknown, ProtectionSummary::level)
    }

    /// The rows the host contributes to the plan's matrix.
    fn rows(&self) -> Vec<DomainCoverage> {
        match &self.analysis {
            Ok(summary) => summary.rows().to_vec(),
            Err(reason) => vec![DomainCoverage::new(
                EffectDomain::RemoteSystem,
                RecoveryObjective::Unknown,
                DomainProtection::Unknown,
                format!(
                    "{}: its protection could not be analysed, so nothing is claimed for it: \
                     {reason}",
                    self.host
                ),
            )],
        }
    }
}

/// One coverage matrix over every host of a remote plan (§29.2).
///
/// The plan-level word is the matrix's own: twelve ZFS-protected, six Btrfs-protected and two
/// unprotected hosts compose to `PARTIALLY_PROTECTED`, and only a policy that excludes the two
/// ([`ProtectionPolicy::excluding_host`]) lets the rest call the plan protected — with the two
/// still named among the exclusions.
#[must_use]
pub fn compose(hosts: &[HostCoverage], policy: &ProtectionPolicy) -> ProtectionSummary {
    let mut rows = Vec::new();
    let mut exclusions = Vec::new();
    for host in hosts {
        if policy.excludes_host(host.host()) {
            rows.extend(
                host.rows()
                    .into_iter()
                    .map(DomainCoverage::declared_irrelevant),
            );
            exclusions.push(CoverageExclusion::new(
                EffectDomain::RemoteSystem,
                host.host(),
                "the plan's policy excludes this host from its protection claim (§29.2)",
            ));
        } else {
            rows.extend(host.rows());
        }
        if let Some(summary) = host.summary() {
            exclusions.extend(summary.plan_exclusions().iter().cloned());
        }
    }
    exclusions
        .into_iter()
        .fold(ProtectionSummary::of(rows), ProtectionSummary::excluding)
}
