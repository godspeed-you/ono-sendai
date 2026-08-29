//! `trace` (spec §22): the relationships the kernel actually asserts, as a graph value.
//!
//! The command resolves its subject first — a trace of nothing is an error naming what was
//! asked for, never an empty graph that looks like an answer — then walks the relationship
//! providers of `ono-graph`, which read procfs and the object providers and mark every edge
//! with who asserted it and how confidently (spec §22.1).

use std::sync::Arc;

use ono_core::ErrorCode;
use ono_graph::{Node, ProcessUsers, TraceOptions, Tracer, kernel_relationships};
use ono_pipeline::ValueStream;
use ono_value::{ErrorValue, Value};

use crate::invoke::{CommandImpl, Invocation, Outcome, OutcomeFuture};

/// The `trace <target>` implementation, one instance per contract.
#[derive(Debug)]
pub(crate) struct TraceCommand {
    id: String,
}

impl TraceCommand {
    pub(crate) fn new(id: &str) -> Self {
        Self { id: id.to_owned() }
    }
}

impl CommandImpl for TraceCommand {
    fn id(&self) -> &str {
        &self.id
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(crate::invoke::must_be_awaited(&self.id))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let query = ctx.contract().query(ctx.arguments())?;
            let target = ctx.contract().target().unwrap_or_default().to_owned();

            // The subject comes through the pipeline or resolves from the selectors; either
            // way it must exist before anything is traced.
            let candidates = match ctx.take_input() {
                Some(stream) => records(stream).await,
                None => records(ctx.providers().snapshot(&query)?).await,
            };
            if candidates.is_empty() {
                return Err({
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        format!(
                            "nothing to trace: no {target} answers to `{}`",
                            query
                                .selectors()
                                .first()
                                .map_or_else(String::new, |selector| {
                                    match selector {
                                        ono_provider_api::Selector::Field { name, value } => {
                                            format!("{name} {value}")
                                        }
                                        other => format!("{other:?}"),
                                    }
                                }),
                        ),
                    )
                    .with_help(format!("`get {target}` shows what exists"))
                });
            }

            // A record whose identity field is null is a value, not an object (spec §27.3,
            // §35.3): a `time-wait` socket whose inode the kernel has already released is a real
            // connection that nothing can be related to. It is skipped rather than fatal, because
            // the connections behind it in the same answer are objects and are what was asked
            // for. Which of several matched objects is traced was already the provider's choice
            // (spec §16.5, §22.1). Only when nothing in the answer is an object is there nothing
            // to build a graph from, and that is a different refusal from "nothing answered".
            let root = candidates
                .iter()
                .find_map(|record| Node::of(record))
                .ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "this record declares no identity, so nothing can relate to it",
                    )
                })?;

            let mut options = TraceOptions::from_query(&query);
            if let Some(depth) = ctx
                .arguments()
                .option("depth")
                .and_then(|value| value.as_int().ok())
                .and_then(|depth| usize::try_from(depth).ok())
            {
                options = options.depth(depth);
            }
            if let Some(relations) = ctx.arguments().option("relations")
                && let Ok(list) = relations.as_list()
            {
                options = options.only_relations(
                    list.iter()
                        .filter_map(|relation| relation.as_str().ok().map(str::to_owned)),
                );
            }

            let providers = Arc::new(ctx.providers().clone());
            let mut tracer = Tracer::new()
                .with_options(options)
                .with_all(kernel_relationships(Arc::clone(&providers)));
            // Spec §22.3: `--users` adds the people behind the processes the trace reaches.
            if ctx
                .arguments()
                .option("users")
                .is_some_and(|value| matches!(value, Value::Bool(true)))
            {
                tracer = tracer.with(Arc::new(ProcessUsers::new(providers)));
            }
            let graph = tracer.trace([root]).await;
            Ok(Outcome::Values(ValueStream::from_values([
                graph.to_value()?
            ])))
        })
    }
}

/// Every record a stream yields, in order. Non-records are not subjects and are dropped.
///
/// The whole answer is kept rather than its first row, because which record a selector that
/// matched several yields first is the provider's business: the caller roots the graph at the
/// first one that can be related to, and tells "nothing answered" apart from "nothing that
/// answered is an object" (spec §16.5, §22.1).
async fn records(stream: ValueStream) -> Vec<std::sync::Arc<ono_value::RecordValue>> {
    let collected = stream.collect().await;
    collected
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(record),
            _ => None,
        })
        .collect()
}
