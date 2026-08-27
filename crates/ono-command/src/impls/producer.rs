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

/// The implicit selector one context frame contributes to a provider query, for a command that
/// builds its query itself (spec §14.3, ADR-0023).
///
/// The command table already narrows the arguments of every command (ADR-0076); this is the
/// query-level form `watch` still composes. A query for the entered target narrows to that one
/// object by its identity fields; a query for another target narrows on the field named after
/// the frame's target, and a target whose schema cannot carry it is refused with the reason,
/// because falling back to the whole machine would mean a command acting on state the user
/// cannot see — exactly what spec §14.3 forbids.
pub(super) fn ambient_selector(
    contract: &crate::CommandContract,
    providers: &ono_provider_api::ProviderRegistry,
    frame: &crate::ContextFrame,
) -> Result<ono_provider_api::Selector, ErrorValue> {
    use ono_provider_api::Selector;

    if contract.target() == Some(frame.target()) {
        let identity = contract.target().and_then(|target| {
            providers
                .for_target(target)
                .iter()
                .flat_map(|provider| provider.schemas())
                .flat_map(|schema| schema.identity().to_vec())
                .find_map(|field| {
                    frame
                        .handle(&field)
                        .map(|value| Selector::field(field.to_string(), value.clone()))
                })
        });
        return Ok(identity.unwrap_or_else(|| Selector::field("name", frame.identity().clone())));
    }

    // The schema that decides is the one the answering provider advertises, because that is the
    // schema the selector will be matched against — a KUANG/11 provider may extend a target with
    // fields the built-in registry has never heard of (spec §31.23).
    let narrows = contract.target().is_some_and(|target| {
        providers
            .for_target(target)
            .iter()
            .flat_map(|provider| provider.schemas())
            .any(|schema| schema.field(frame.target()).is_some())
    });
    if narrows {
        return Ok(Selector::field(frame.target(), frame.identity().clone()));
    }

    Err(ErrorValue::new(
        ono_core::ErrorCode::ResolveTargetNotFound,
        format!(
            "`{}` has no meaning inside the context `{}`",
            contract.spelling(),
            frame.spelling(),
        ),
    )
    .with_help(format!(
        "the {} of `{}` carries no `{}` field to narrow by. `leave` the context, or write the \
         query explicitly (spec §14.5)",
        contract.output().text(),
        contract.spelling(),
        frame.target(),
    )))
}
