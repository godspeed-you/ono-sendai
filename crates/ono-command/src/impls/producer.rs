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
        let mut query = ctx.contract().query(ctx.arguments())?;
        for frame in ctx.context() {
            // A filesystem frame's whole effect is the working directory it already changed
            // (spec §14.2); only an object frame narrows what a provider is asked (§14.3).
            if frame.kind() == crate::FrameKind::Object {
                query = query.with(ambient_selector(ctx.contract(), ctx.providers(), frame)?);
            }
        }
        let stream = ctx.providers().snapshot(&query)?;
        Ok(Outcome::Values(stream))
    }
}

/// The implicit selector one context frame contributes to this query (spec §14.3, ADR-0023).
///
/// Inside `enter service nginx`, `get process` asks for that service's processes — the frame
/// becomes the selector `--service nginx.service` (spec §14.5). A query for the entered target
/// itself narrows to that one object by name. A target whose schema cannot carry the frame's
/// field is refused with the reason, because falling back to the whole machine would mean a
/// command acting on state the user cannot see — exactly what spec §14.3 forbids.
fn ambient_selector(
    contract: &crate::CommandContract,
    providers: &ono_provider_api::ProviderRegistry,
    frame: &crate::ContextFrame,
) -> Result<ono_provider_api::Selector, ErrorValue> {
    use ono_provider_api::Selector;

    if contract.target() == Some(frame.target()) {
        return Ok(Selector::field("name", frame.identity().clone()));
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
