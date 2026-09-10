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
    AssetState, PersistenceDomain, ProviderAvailability, ProviderCapabilities, RecoveryAsset,
    RecoveryCandidate, RecoveryObjective, RecoveryProvider,
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
    resolutions: Vec<(Arc<str>, PersistenceDomain)>,
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

    /// The domain `provider` resolved for itself, where it answered (ADR-0807).
    #[must_use]
    pub fn resolved_by(&self, provider: &str) -> Option<&PersistenceDomain> {
        self.resolutions
            .iter()
            .find(|(id, _)| id.as_ref() == provider)
            .map(|(_, domain)| domain)
    }

    /// Every domain a provider resolved for itself, with the provider that resolved it.
    #[must_use]
    pub fn resolutions(&self) -> Vec<(&str, &PersistenceDomain)> {
        self.resolutions
            .iter()
            .map(|(id, domain)| (id.as_ref(), domain))
            .collect()
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
        self.resolutions.extend(other.resolutions);
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
            resolutions: Vec::new(),
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

    /// Asks every available provider what it could offer for `path`, each over the domain it
    /// resolved itself (Appendix A.3, Appendix B, ADR-0807).
    ///
    /// `fallback` is Appendix B's resolution from the mount table. It decides first: a path the
    /// core refused — a network filesystem, a pseudo or volatile one, an overlay whose writable
    /// layer is elsewhere — is not offered to any provider, whatever that provider would say
    /// (Appendix B.6, B.7, §32.4). Otherwise each provider is asked `resolve_domain(path)`:
    ///
    /// - `Some(domain)` is the provider's own identity for the object — the Btrfs provider's
    ///   filesystem-and-subvolume-id reference rather than the `subvol=` option — and that is
    ///   the domain its `discover` is handed;
    /// - `None` means "not mine to map", and the provider is handed `fallback`;
    /// - an error is §56.3's "could not establish this", and it becomes that provider's
    ///   [`ProviderRefusal`] rather than an empty answer, so the outcome is not conclusive;
    /// - a domain resolved through a different mount than `fallback`'s is a refusal too: the
    ///   kernel's table decides which filesystem serves the path, and a provider that reads only
    ///   its own filesystem's mounts can mistake an ext4 disk beneath its root for its root.
    ///
    /// Only an absolute path is offered to `resolve_domain`: a unit name or an endpoint is not a
    /// path any provider maps.
    #[must_use]
    pub fn discover_at(
        &self,
        path: &str,
        fallback: Option<&PersistenceDomain>,
        objective: RecoveryObjective,
    ) -> DiscoveryOutcome {
        let mut outcome = DiscoveryOutcome {
            candidates: Vec::new(),
            refusals: self.unavailable(),
            resolutions: Vec::new(),
        };
        if fallback.is_some_and(|domain| !domain.is_protectable()) {
            return outcome;
        }
        for provider in self.available() {
            let own = if path.starts_with('/') {
                match provider.resolve_domain(path) {
                    Ok(own) => own,
                    Err(error) => {
                        let reason = format!(
                            "the persistence domain of {path} could not be established: {}",
                            error.message()
                        );
                        outcome
                            .refusals
                            .push(ProviderRefusal::new(provider.id(), reason, error));
                        continue;
                    }
                }
            } else {
                None
            };
            if let (Some(own), Some(fallback)) = (&own, fallback)
                && own.is_protectable()
                && !same_mount(own, fallback)
            {
                // §56.3 and Appendix B.1: the kernel's table says which filesystem serves the
                // path. A provider that reached it through another mount is describing another
                // filesystem, and a claim is not made over an object two readings disagree about.
                let reason = format!(
                    "the provider resolved {path} through the mount at {} while the kernel mount \
                     table serves it from {}, so its answer describes another filesystem",
                    display_point(own.mount().mount_point()),
                    display_point(fallback.mount().mount_point())
                );
                let error = error::provider_unavailable(provider.id(), &reason);
                outcome
                    .refusals
                    .push(ProviderRefusal::new(provider.id(), reason, error));
                continue;
            }
            let domain = match (own, fallback) {
                (Some(own), _) => {
                    outcome
                        .resolutions
                        .push((Arc::from(provider.id()), own.clone()));
                    own
                }
                (None, Some(fallback)) => fallback.clone(),
                (None, None) => continue,
            };
            if !domain.is_protectable() {
                continue;
            }
            match provider.discover(&domain, objective) {
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

    /// The persistence domain of `path`: what the first available provider that maps it to an
    /// object of its own resolves — the Btrfs subvolume that holds a file rather than the mount's
    /// `subvol=` option (Appendix B.9) — and `fallback`, Appendix B's reading of the mount table,
    /// where none does.
    ///
    /// The rules are [`Self::discover_at`]'s, without the discovery: a path the core refused is
    /// offered to no provider, a provider that answers "not mine" or could not establish the
    /// domain changes nothing (§56.3), and a domain resolved through another mount than
    /// `fallback`'s describes another filesystem and is set aside. A provider that names the path
    /// itself as its object — a copy provider, which protects a file by copying it — says how it
    /// would protect the path and not what holds its state (Appendix B.1), so it changes nothing
    /// either.
    #[must_use]
    pub fn resolve_at(&self, path: &str, fallback: &PersistenceDomain) -> PersistenceDomain {
        if !fallback.is_protectable() || !path.starts_with('/') {
            return fallback.clone();
        }
        self.available()
            .into_iter()
            .find_map(|provider| {
                provider.resolve_domain(path).ok().flatten().filter(|own| {
                    own.is_protectable()
                        && same_mount(own, fallback)
                        && own.object().is_some_and(|object| object != path)
                })
            })
            .unwrap_or_else(|| fallback.clone())
    }

    /// Asks an asset's own provider whether a READY asset still is (§11.4, §37.5).
    ///
    /// A record says what was true when the asset was validated. The bytes behind it can be
    /// deleted afterwards — a store emptied by hand, a snapshot destroyed outside Ono — and a
    /// listing that repeats the record would show a recovery point that no longer exists. This is
    /// the check a caller makes before it shows, plans from or relies on an asset: a READY asset is
    /// validated again and comes back READY or INVALID on the strength of that validation. Any
    /// other state is returned as it is, because only READY makes a claim a validation can
    /// withdraw.
    ///
    /// # Errors
    ///
    /// `recovery.provider_unavailable` when the owning provider is not registered or cannot run
    /// here, and the provider's own error when validation itself fails. §56.3: an asset nobody
    /// could check is not thereby confirmed READY, and the caller decides how to show that.
    pub fn revalidate(&self, asset: &RecoveryAsset) -> Result<RecoveryAsset, ErrorValue> {
        if asset.state() != AssetState::Ready {
            return Ok(asset.clone());
        }
        let Some(provider) = self.get(asset.provider()) else {
            return Err(error::provider_unavailable(
                asset.provider(),
                "it is not registered here, so nothing can confirm the asset still exists",
            ));
        };
        if let ProviderAvailability::Unavailable { reason } = provider.availability() {
            return Err(error::provider_unavailable(asset.provider(), &reason));
        }
        let validation = provider.validate(asset)?;
        Ok(asset.clone().validated(validation))
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

/// Whether two resolutions of one path went through the same mount (Appendix B.1).
fn same_mount(left: &PersistenceDomain, right: &PersistenceDomain) -> bool {
    display_point(left.mount().mount_point()) == display_point(right.mount().mount_point())
}

/// A mount point as a person reads it, with `/` for the root rather than the empty string.
fn display_point(point: &str) -> &str {
    let trimmed = point.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }
}
