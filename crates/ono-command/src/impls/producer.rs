//! `get <target>` and its siblings: one implementation, parameterised by the contract.
//!
//! Spec §27.1 puts a provider capability on every command that asks the system something, and the
//! contract already carries the target, the selectors and their types. So there is one producer
//! here rather than one per target: the selectors become the [`Query`](ono_provider_api::Query)
//! the provider may push down, and the provider's stream is the stage's output — nothing is
//! collected, and the boundedness the provider declared is the boundedness the next stage sees
//! (spec §11.1).

use ono_value::ErrorValue;

use crate::invoke::{CommandImpl, Invocation, Outcome};

/// A producer over whichever provider answers the contract's target.
#[derive(Debug)]
pub(crate) struct ProviderProducer {
    id: String,
}

impl ProviderProducer {
    pub(crate) fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

impl CommandImpl for ProviderProducer {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        // The query is built from the contract, so a new target is a new registry entry and no
        // new code (ADR-0012, ADR-0021).
        let query = ctx.contract().query(ctx.arguments())?;
        let stream = ctx.providers().snapshot(&query)?;
        Ok(Outcome::Values(stream))
    }
}
