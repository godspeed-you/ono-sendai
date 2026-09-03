//! Which provider answers which target.

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_value::{ErrorValue, Schema};

use crate::{
    Action, ActionOutcome, Availability, EventStream, ObjectId, ObjectRef, Provider, Query,
    Selector,
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

    /// Performs `action` through the provider the object it names belongs to.
    ///
    /// # Errors
    ///
    /// See [`ProviderRegistry::provider_for`] and [`ProviderRegistry::provider_of`], plus
    /// `provider.unsupported` when the provider does not implement the operation.
    pub async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        self.provider_of(action.target_name(), action.target())?
            .act(action)
            .await
    }

    /// The provider `object` belongs to, which is not always the first one that answers.
    ///
    /// A schema whose identity names a `provider` says which of several answering systems made
    /// the object: `ono.package/1` is `provider + name`, and on a Debian machine that also has
    /// `rpm` installed a record the rpm side made must not be acted on by dpkg because dpkg
    /// registered first (ADR-0559). Where the identity says so, the record is routed by what it
    /// says; where it does not, this is [`ProviderRegistry::provider_for`].
    ///
    /// # Errors
    ///
    /// `provider.unavailable` naming the token, when the provider a record names is not
    /// registered here or cannot answer. Refusing by name is the answer: acting through another
    /// provider would change a system the record was never about.
    pub fn provider_of(
        &self,
        target: &str,
        object: &ObjectId,
    ) -> Result<&Arc<dyn Provider>, ErrorValue> {
        let Some(token) = self.identity_token_of(object) else {
            return self.provider_for(target);
        };
        let claiming = self.for_target(target);
        // Only a target whose providers say which of them a record names is routed by what the
        // record says. Where none of them does — `service`, which systemd alone answers — the
        // `provider` field is a note on the record rather than a choice between answerers.
        if claiming
            .iter()
            .all(|provider| provider.identity_token().is_none())
        {
            return self.provider_for(target);
        }
        let named: Vec<&&Arc<dyn Provider>> = claiming
            .iter()
            .filter(|provider| provider.identity_token() == Some(token.as_str()))
            .collect();
        if named.is_empty() {
            // Nothing here answers for that token. It may be a record from another machine, or
            // from a build with a provider this one does not have.
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!(
                    "`{object}` was made by `{token}`, and no provider of `{target}` here \
                     answers for `{token}`"
                ),
            )
            .with_help(
                "the object names the system it belongs to, and acting on it through another \
                 one would change something this record was never about",
            ));
        }
        for provider in &named {
            if let Availability::Available = provider.availability() {
                return Ok(provider);
            }
        }
        let reasons: Vec<String> = named
            .iter()
            .filter_map(|provider| {
                provider
                    .availability()
                    .reason()
                    .map(|reason| format!("{}: {reason}", provider.id()))
            })
            .collect();
        Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`{object}` was made by `{token}`, which cannot answer here — {}",
                reasons.join("; ")
            ),
        ))
    }

    /// The value an object's identity gives for a field called `provider`, if it has one.
    ///
    /// The identity carries values in the order the schema declares them and not their names, so
    /// the position is read from the schema — which is one a registered provider emits, because
    /// the object came from one of them.
    fn identity_token_of(&self, object: &ObjectId) -> Option<String> {
        let schema = self
            .schemas()
            .into_iter()
            .find(|schema| schema.id() == object.schema())?;
        // A selector a user typed builds a *partial* identity — `add package curl` is
        // `ono.package/1[curl]` — and a name says nothing about which database it belongs to.
        // Only an identity with a value for every field the schema declares is a record's, and
        // only a record's says which provider made it.
        if object.values().len() != schema.identity().len() {
            return None;
        }
        let position = schema
            .identity()
            .iter()
            .position(|field| &**field == "provider")?;
        object
            .values()
            .get(position)
            .and_then(|value| value.as_str().ok())
            .map(ToOwned::to_owned)
    }
}
