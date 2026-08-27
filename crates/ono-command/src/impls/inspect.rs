//! `inspect <target>` (spec §9.1, §33.1): one object, read closer, with its provenance.
//!
//! The provider that enumerates a target is the provider that can look at one of its objects in
//! detail, so the command is the producer's query with the `detail` option set — the provider
//! answers with the target's detail schema instead of its list schema (ADR-0091). The object
//! comes through the pipeline or from the selectors, and either way it has to exist: inspecting
//! nothing is the provider's failure naming what was asked for, never an empty answer.

use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_provider_api::Selector;
use ono_value::{ErrorValue, Value};

use crate::invoke::{CommandImpl, Invocation, Outcome, OutcomeFuture};

/// The `inspect <target>` implementation, one instance per contract.
#[derive(Debug)]
pub(crate) struct InspectCommand {
    id: String,
}

impl InspectCommand {
    pub(crate) fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

impl CommandImpl for InspectCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(crate::invoke::must_be_awaited(&self.id))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let spelling = ctx.contract().spelling();
            let mut query = ctx
                .contract()
                .query(ctx.arguments())?
                .option("detail", Value::Bool(true));

            // An object that arrived carries its identity; the provider is asked for exactly
            // that object — every identity field it knows, so a recycled pid is not inspected
            // in the place of the process that was selected.
            if let Some(mut input) = ctx.take_input() {
                let mut subject: Option<Vec<Selector>> = None;
                while let Some(event) = input.recv().await {
                    match event {
                        ono_pipeline::StreamEvent::Value(Value::Record(record)) => {
                            let known: Vec<Selector> = record
                                .identity()
                                .iter()
                                .filter(|(_, value)| !matches!(value, Value::Null))
                                .map(|(name, value)| Selector::field(name, value.clone()))
                                .collect();
                            if !known.is_empty() {
                                subject = Some(known);
                                break;
                            }
                        }
                        ono_pipeline::StreamEvent::Value(other) => {
                            return Err(ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                format!(
                                    "`{spelling}` inspects an object, and a {} is not one",
                                    other.type_name()
                                ),
                            ));
                        }
                        ono_pipeline::StreamEvent::Failure(error) => return Err(error),
                    }
                }
                let Some(selectors) = subject else {
                    return Err(ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        format!("nothing arrived for `{spelling}` to inspect"),
                    )
                    .with_help("pipe one object in, or name it: `inspect process <pid>`"));
                };
                for selector in selectors {
                    query = query.with(selector);
                }
            } else if query.selectors().is_empty() {
                return Err(ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("`{spelling}` needs an object to inspect, and none was given"),
                )
                .with_help(format!(
                    "name it, as in `{spelling} <selector>`, or pipe it in"
                )));
            }

            let collected = ctx.providers().snapshot(&query)?.collect().await;
            if let Some(error) = collected.errors().first() {
                return Err(error.clone());
            }
            let mut values = collected.into_values();
            if values.is_empty() {
                let target = ctx.contract().target().unwrap_or_default();
                return Err(ErrorValue::new(
                    ErrorCode::IoNotFound,
                    format!("no {target} answers to `{spelling}`"),
                )
                .with_help(format!("`get {target}` lists what is there")));
            }
            Ok(Outcome::Values(ValueStream::from_values(
                [values.remove(0)],
            )))
        })
    }
}
