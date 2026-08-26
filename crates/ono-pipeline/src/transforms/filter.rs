//! `where`: keep the values whose predicate is exactly `true` (spec §53, ADR-0014).

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

use crate::{Predicate, SinkClosed, StreamSink, Transform, ValueStream};

/// Filters a stream by a predicate.
///
/// ADR-0014 fixes the four outcomes, and this transform implements exactly them:
///
/// | predicate | outcome |
/// |---|---|
/// | `true` | the value is kept |
/// | `false` | the value is dropped |
/// | `null` | the value is dropped and counted as excluded-unknown |
/// | an `Error` | the value is dropped and the failure is reported with the value's identity |
///
/// So `get process | where cpu > 20` does not report a process whose CPU is unknown, and it
/// never silently reads "I am not allowed to see this" as "this is below 20".
pub struct Where {
    predicate: Box<dyn Predicate>,
}

impl Where {
    /// Filters by `predicate`.
    ///
    /// ```
    /// use ono_pipeline::{ValueStream, Where};
    /// use ono_value::Value;
    ///
    /// let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
    /// runtime.block_on(async {
    ///     let collected = ValueStream::from_values([Value::Int(1), Value::Int(2)])
    ///         .transform(Where::new(|value: &Value| Value::Bool(value == &Value::Int(2))))
    ///         .unwrap()
    ///         .collect()
    ///         .await;
    ///     assert_eq!(collected.values(), [Value::Int(2)]);
    /// });
    /// ```
    #[must_use]
    pub fn new(predicate: impl Predicate) -> Self {
        Self {
            predicate: Box::new(predicate),
        }
    }
}

impl Transform for Where {
    fn name(&self) -> &'static str {
        "where"
    }

    fn apply(self: Box<Self>, input: ValueStream) -> ValueStream {
        // `where` yields as it reads, so its output ends exactly when its input does (spec §11.1).
        let boundedness = input.boundedness();
        input.stage(boundedness, move |mut input, sink| async move {
            while let Some(value) = input.next_value(&sink).await {
                match self.predicate.test(&value) {
                    Value::Bool(true) => {
                        if sink.send(value).await.is_err() {
                            return;
                        }
                    }
                    Value::Bool(false) => {}
                    Value::Null => sink.diagnostics().record_excluded_unknown(),
                    Value::Error(error) => {
                        if sink
                            .fail(with_identity((*error).clone(), &value))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    other => {
                        if report_non_boolean(&sink, &other, &value).await.is_err() {
                            return;
                        }
                    }
                }
            }
        })
    }
}

/// Attaches the identity of the object a failure is about, when the failure does not name one.
///
/// Spec §16.5 requires a partial failure to say *which* item failed; without the target, "one of
/// your 412 processes could not be read" is not actionable.
pub(crate) fn with_identity(error: ErrorValue, value: &Value) -> ErrorValue {
    match value {
        Value::Record(record) if error.target().is_none() => error.with_target(record.to_ref()),
        _ => error,
    }
}

async fn report_non_boolean(
    sink: &StreamSink,
    outcome: &Value,
    value: &Value,
) -> Result<(), SinkClosed> {
    let error = ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!(
            "a `where` predicate must be true, false or null, but it produced {}",
            outcome.type_name()
        ),
    )
    .with_help("compare a field against a value, or use `== null` to ask whether it is unknown");
    sink.fail(with_identity(error, value)).await
}

crate::transform::debug_as_name!(Where);
