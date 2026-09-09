//! The recovery providers a plan may ask, and the ones it had to skip (spec §12.2, §55.6).
//!
//! Two rules shape this module, and both are about what an empty answer means.
//!
//! §12.2 fixes five capabilities every recovery provider declares, and
//! [`ProviderCapabilities::missing_required`] names the ones a candidate provider left out.
//! A provider that can discover and prepare but cannot restore is snapshot theatre (§62.1), so
//! [`ProviderRegistry::register`] refuses it with a structured error instead of keeping it and
//! finding out during recovery.
//!
//! §55.6 case 29 — *"Unknown provider recovery semantics remain UNKNOWN"* — is the other. A
//! provider whose tool is missing is skipped, and the skip is **reported**: a
//! [`DiscoveryOutcome`] carries the refusals beside the candidates, so an empty candidate list
//! can be told apart from "there is nothing here to protect". The difference is the whole
//! distance between "this plan needs no protection" and "Ono could not find out".

use std::sync::Arc;

use ono_change_core::error;
use ono_change_core::{
    PersistenceDomain, ProviderCapabilities, RecoveryCandidate, RecoveryObjective, RecoveryProvider,
};
use ono_value::ErrorValue;

/// Why one provider contributed nothing to a discovery (§12.2, §55.6 case 29).
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderRefusal {
    provider: Arc<str>,
    reason: Arc<str>,
    error: ErrorValue,
}

impl ProviderRefusal {
    /// Records that `provider` was skipped, with the refusal a caller can render or raise.
    #[must_use]
    pub fn new(
        provider: impl Into<Arc<str>>,
        reason: impl Into<Arc<str>>,
        error: ErrorValue,
    ) -> Self {
        Self {
            provider: provider.into(),
            reason: reason.into(),
            error,
        }
    }

    /// The provider that was skipped.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Why, in a sentence a person can act on.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// The structured refusal (§45).
    #[must_use]
    pub const fn error(&self) -> &ErrorValue {
        &self.error
    }
}

/// What a discovery found, and what it could not ask (Appendix A.3, §55.6 case 29).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiscoveryOutcome {
    candidates: Vec<RecoveryCandidate>,
    refusals: Vec<ProviderRefusal>,
}

impl DiscoveryOutcome {
    /// The candidates every available provider offered.
    #[must_use]
    pub fn candidates(&self) -> &[RecoveryCandidate] {
        &self.candidates
    }

    /// The providers that were skipped, and why.
    #[must_use]
    pub fn refusals(&self) -> &[ProviderRefusal] {
        &self.refusals
    }

    /// Whether this outcome establishes that nothing can protect the domain.
    ///
    /// An empty candidate list from a complete set of providers is an answer; an empty list from
    /// a set that could not all be asked is not, and §55.6 case 29 keeps the two apart.
    #[must_use]
    pub fn is_conclusive(&self) -> bool {
        self.refusals.is_empty()
    }

    /// Merges another outcome into this one, keeping both halves of both.
    #[must_use]
    pub fn merged_with(mut self, other: Self) -> Self {
        self.candidates.extend(other.candidates);
        for refusal in other.refusals {
            if !self
                .refusals
                .iter()
                .any(|kept| kept.provider() == refusal.provider())
            {
                self.refusals.push(refusal);
            }
        }
        self
    }
}

/// The recovery providers this shell may ask (§12.1, §12.2).
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn RecoveryProvider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderRegistry")
            .field("providers", &self.ids())
            .finish()
    }
}

impl ProviderRegistry {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Adds `provider`, or refuses it (§12.2).
    ///
    /// # Errors
    ///
    /// `recovery.provider_unavailable` when the provider does not declare all five of §12.2's
    /// required capabilities, or when its id is already registered. A provider that cannot
    /// restore is not a recovery provider, and admitting it would let a plan show a protection
    /// path that no recovery could ever walk.
    pub fn register(&mut self, provider: Arc<dyn RecoveryProvider>) -> Result<(), ErrorValue> {
        let id = provider.id().to_owned();
        let missing = provider.capabilities().missing_required();
        if !missing.is_empty() {
            let names: Vec<&str> = missing
                .iter()
                .map(|capability| capability.as_str())
                .collect();
            return Err(error::provider_unavailable(
                &id,
                &format!(
                    "v0.6 §12.2 requires every recovery provider to declare discover, prepare, \
                     restore, cleanup and estimate-cost. This one declares neither {}. A provider \
                     that cannot restore is not a recovery provider, and it was not registered",
                    names.join(" nor ")
                ),
            ));
        }
        if self.providers.iter().any(|kept| kept.id() == id) {
            return Err(error::provider_unavailable(
                &id,
                "a provider with this id is already registered, and two mechanisms answering to \
                 one name would make a recovery asset's provenance ambiguous (§11.1)",
            ));
        }
        self.providers.push(provider);
        Ok(())
    }

    /// The registered ids, in registration order.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.providers
            .iter()
            .map(|provider| provider.id())
            .collect()
    }

    /// The provider with this id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Arc<dyn RecoveryProvider>> {
        self.providers.iter().find(|provider| provider.id() == id)
    }

    /// Every registered provider, available or not.
    #[must_use]
    pub fn providers(&self) -> &[Arc<dyn RecoveryProvider>] {
        &self.providers
    }

    /// The providers that can run here (§12.2).
    #[must_use]
    pub fn available(&self) -> Vec<&Arc<dyn RecoveryProvider>> {
        self.providers
            .iter()
            .filter(|provider| provider.availability().is_available())
            .collect()
    }

    /// The refusal for every provider that cannot run here (§12.2, Appendix G.4).
    #[must_use]
    pub fn unavailable(&self) -> Vec<ProviderRefusal> {
        self.providers
            .iter()
            .filter_map(|provider| {
                let availability = provider.availability();
                let reason = availability.reason()?.to_owned();
                Some(ProviderRefusal::new(
                    provider.id(),
                    reason.clone(),
                    error::provider_unavailable(provider.id(), &reason),
                ))
            })
            .collect()
    }

    /// Asks every available provider what it could offer for `domain` (Appendix A.3).
    ///
    /// A provider that is unavailable, and a provider whose discovery failed, both leave a
    /// [`ProviderRefusal`] behind rather than a silent gap (§55.6 case 29).
    #[must_use]
    pub fn discover(
        &self,
        domain: &PersistenceDomain,
        objective: RecoveryObjective,
    ) -> DiscoveryOutcome {
        let mut outcome = DiscoveryOutcome {
            candidates: Vec::new(),
            refusals: self.unavailable(),
        };
        if !domain.is_protectable() {
            return outcome;
        }
        for provider in self.available() {
            match provider.discover(domain, objective) {
                Ok(candidates) => outcome.candidates.extend(candidates),
                Err(error) => {
                    let reason = error.message().to_owned();
                    outcome
                        .refusals
                        .push(ProviderRefusal::new(provider.id(), reason, error));
                }
            }
        }
        outcome
    }

    /// The §12.2 capabilities `capabilities` would have to add before registration succeeds.
    #[must_use]
    pub fn registration_shortfall(capabilities: &ProviderCapabilities) -> Vec<&'static str> {
        capabilities
            .missing_required()
            .iter()
            .map(|capability| capability.as_str())
            .collect()
    }
}
