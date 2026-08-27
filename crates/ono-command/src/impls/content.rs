//! `read file` and `tail file`: the content behind an object, from the provider that owns it.
//!
//! A producer answers "which objects"; these answer "what does this one hold". The provider is
//! asked with the same [`Query`] a producer builds, marked with the verb (`Query::for_verb`),
//! and what it streams — bytes, text, lines — is the stage's output as it is (ADR-0083).
//!
//! The object is named either by the selectors (`read file ./notes.md`) or by the records that
//! arrive on the pipeline (`get file *.md | read file --encoding utf-8`): a File record carries
//! the path the provider reached it by, so each record becomes one query for its content, in
//! the order the records arrived.

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamEvent, ValueStream};
use ono_provider_api::{Query, Selector};
use ono_value::{ErrorValue, Value};

use crate::invoke::{CommandImpl, Invocation, Outcome};

/// The content command over whichever provider answers the contract's target.
#[derive(Debug)]
pub(crate) struct ContentCommand {
    id: String,
}

impl ContentCommand {
    pub(crate) fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

impl CommandImpl for ContentCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let query = ctx.contract().query(ctx.arguments())?;
        let Some(mut input) = ctx.take_input() else {
            return Ok(Outcome::Values(ctx.providers().snapshot(&query)?));
        };
        let spelling = ctx.contract().spelling();
        let providers = ctx.providers().clone();
        // The selector the piped record fills in: the contract's first, which for every content
        // command is the path or name that identifies the object.
        let selector = ctx
            .contract()
            .selectors()
            .first()
            .map(|spec| spec.name().to_owned())
            .ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{spelling}` declares nothing a piped object could name"),
                )
            })?;
        Ok(Outcome::Values(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                while let Some(event) = input.recv().await {
                    let record = match event {
                        StreamEvent::Value(Value::Record(record)) => record,
                        StreamEvent::Value(other) => {
                            let error = ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                format!(
                                    "`{spelling}` reads objects, and a {} is not one",
                                    other.type_name()
                                ),
                            );
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                            continue;
                        }
                        StreamEvent::Failure(error) => {
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                            continue;
                        }
                    };
                    let Some(handle) = record.get(&selector).cloned() else {
                        let error = ErrorValue::new(
                            ErrorCode::TypeMismatch,
                            format!(
                                "`{spelling}` needs a `{selector}` on each object, and `{}` \
                                 carries none",
                                record.schema_id()
                            ),
                        );
                        if sink.fail(error).await.is_err() {
                            return;
                        }
                        continue;
                    };
                    let mut one = Query::target(query.target_name())
                        .for_verb(query.verb())
                        .with(Selector::field(&selector, handle));
                    for (name, value) in query.options() {
                        one = one.option(name, value.clone());
                    }
                    let mut content = match providers.snapshot(&one) {
                        Ok(stream) => stream,
                        Err(error) => {
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                            continue;
                        }
                    };
                    while let Some(event) = content.recv().await {
                        let delivered = match event {
                            StreamEvent::Value(value) => sink.send(value).await.is_ok(),
                            StreamEvent::Failure(error) => sink.fail(error).await.is_ok(),
                        };
                        if !delivered {
                            return;
                        }
                    }
                }
            },
        )))
    }
}
