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
    /// Whether the query asks the provider to keep producing as the world changes — `tail
    /// journal` rather than `get journal` (spec §7.1).
    follow: bool,
}

impl ProviderProducer {
    pub(crate) fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            follow: false,
        }
    }

    pub(crate) fn following(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            follow: true,
        }
    }
}

impl CommandImpl for ProviderProducer {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        // The query is built from the contract, so a new target is a new registry entry and no
        // new code (ADR-0012, ADR-0021). What a context frame narrows is already in the
        // arguments: the command table filled it in before this ran (spec §14.3, ADR-0076).
        let mut query = ctx.contract().query(ctx.arguments())?;
        // A parenthesised value — `--since (now() - 1h)` — is bound as an expression and has no
        // value until it is evaluated here, against the invocation's scope; the contract's
        // `query` carries only what already is a value (ADR-0085 §2).
        let arguments = ctx.arguments();
        for (name, binding) in arguments.selectors().iter().chain(arguments.options()) {
            if let crate::Binding::Expressions(expressions) = binding {
                for expression in expressions {
                    let value =
                        crate::expr::evaluate(expression, &ono_value::Value::Null, ctx.scope())?;
                    query = if arguments.selector_binding(name).is_some() {
                        query.with(ono_provider_api::Selector::field(name, value))
                    } else {
                        query.option(name, value)
                    };
                }
            }
        }
        if self.follow {
            query = query.option("follow", ono_value::Value::Bool(true));
        }
        let stream = ctx.providers().snapshot(&query)?;
        Ok(Outcome::Values(stream))
    }
}
