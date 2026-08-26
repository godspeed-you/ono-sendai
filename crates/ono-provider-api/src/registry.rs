//! Which provider answers which target.

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_value::{ErrorValue, Schema};

use crate::{
    Action, ActionOutcome, Availability, EventStream, ObjectRef, Provider, Query, Selector,
};

/// The providers this shell knows about.
///
/// Registration is explicit and ordered: the first provider claiming a target answers for it, and
/// a later one claiming the same target extends rather than replaces — which is what lets a
/// KUANG/11 package add a container runtime without displacing the one already there
/// (spec §31.23).
#[derive(Clone, Debug, Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn Provider>>,
}

impl ProviderRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Adds a provider.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
    }

    /// Every registered provider.
    #[must_use]
    pub fn providers(&self) -> &[Arc<dyn Provider>] {
        &self.providers
    }

    /// Every schema any registered provider produces.
    #[must_use]
    pub fn schemas(&self) -> Vec<Arc<Schema>> {
        self.providers
            .iter()
            .flat_map(|provider| provider.schemas())
            .collect()
    }

    /// The providers claiming `target`, in registration order.
    #[must_use]
    pub fn for_target(&self, target: &str) -> Vec<&Arc<dyn Provider>> {
        self.providers
            .iter()
            .filter(|provider| provider.targets().contains(&target))
            .collect()
    }

    /// The first available provider for `target`.
    ///
    /// # Errors
    ///
    /// `resolve.target_not_found` when nothing claims the target, and `provider.unavailable` —
    /// carrying the provider's own reason — when something claims it but cannot answer here. The
    /// two are different questions and a user needs to be told which one they are looking at.
    pub fn provider_for(&self, target: &str) -> Result<&Arc<dyn Provider>, ErrorValue> {
        let claiming = self.for_target(target);
        if claiming.is_empty() {
            return Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("no provider answers `{target}`"),
            )
            .with_help("`help targets` lists what this shell can be asked about"));
        }

        let mut reasons = Vec::new();
        for provider in &claiming {
            match provider.availability() {
                Availability::Available => return Ok(provider),
                Availability::Unavailable(reason) => {
                    reasons.push(format!("{}: {reason}", provider.id()));
                }
            }
        }
        Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`{target}` cannot be answered here — {}",
                reasons.join("; ")
            ),
        )
        .with_help(
            "the provider exists but the system it reads is not present. This is not the same as \
             there being none of the thing you asked for.",
        ))
    }

    /// The objects matching `query`, from the first available provider for its target.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::provider_for`], plus whatever the provider itself reports.
    pub fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        self.provider_for(query.target_name())?.snapshot(query)
    }

    /// Changes to the objects matching `query`.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::provider_for`], plus `provider.unsupported` when the provider
    /// cannot watch.
    pub fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        self.provider_for(query.target_name())?.subscribe(query)
    }

    /// The objects a selector names within `target`.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::provider_for`].
    pub async fn resolve(
        &self,
        target: &str,
        selector: &Selector,
    ) -> Result<Vec<ObjectRef>, ErrorValue> {
        self.provider_for(target)?.resolve(selector).await
    }

    /// Performs `action` through the provider that owns its target.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::provider_for`], plus `provider.unsupported` when the provider does
    /// not implement the operation.
    pub async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        self.provider_for(action.target_name())?.act(action).await
    }
}
