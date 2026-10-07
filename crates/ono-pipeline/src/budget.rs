//! The materialization contract of spec v0.4.1 §22: the one place a global operation turns a
//! finite stream into memory.
//!
//! The budget itself lives in `ono-value`, beside the estimator it spends and beside the values
//! it counts, so a component that retains values without owning a stream — the result history of
//! §24 — can share the abstraction §21.1 asks for without depending on the streaming engine
//! (ADR-0453).

use ono_core::ErrorCode;
use ono_value::{Budget, ErrorValue, Exceeded, MaterializationLimits, Value};

use crate::stream::{Collected, ValueStream, unbounded_error};

/// Collects a finite stream into memory, refusing when it does not fit `budget` (§22.1, §22.2).
///
/// This is the one place a global operation turns a stream into a `Vec`. §30.2 puts the helper in
/// the evaluator's materialize module so no caller recreates it, and §6.2 puts byte-budget
/// enforcement in the materialization primitive rather than in each caller: both are satisfied by
/// there being exactly one of these.
///
/// # Errors
///
/// - [`ono_core::ErrorCode::StreamUnboundedOperation`] when the upstream declares itself
///   [`crate::Boundedness::Unbounded`], **before consuming a value** (§22.3: "It MUST NOT wait forever to
///   discover that an unbounded stream never ends").
/// - [`ono_core::ErrorCode::ResourceItemLimit`] or [`ono_core::ErrorCode::ResourceByteLimit`]
///   when a ceiling is
///   reached. The stream is dropped at that point, which cancels the stages above it.
pub async fn materialize(stream: ValueStream, budget: Budget) -> Result<Collected, ErrorValue> {
    materialize_with(stream, budget)
        .await
        .map(|(collected, _)| collected)
}

/// [`materialize`], handing the spent budget back so a parent can absorb it (§23.4).
///
/// # Errors
///
/// As [`materialize`].
pub async fn materialize_with(
    mut stream: ValueStream,
    mut budget: Budget,
) -> Result<(Collected, Budget), ErrorValue> {
    if !stream.boundedness().is_bounded() {
        return Err(unbounded_error(budget.stage()));
    }
    if let Some(refusal) = admits_nothing(
        budget.stage(),
        MaterializationLimits::new(budget.max_items(), budget.max_bytes()),
    ) {
        return Err(refusal);
    }
    let mut values = Vec::new();
    let mut errors = Vec::new();
    while let Some(event) = stream.recv().await {
        match event {
            crate::StreamEvent::Value(value) => {
                budget.charge(&value).map_err(Exceeded::into_error)?;
                values.push(value);
            }
            crate::StreamEvent::Failure(error) => errors.push(error),
        }
    }
    let diagnostics = stream.diagnostics().clone();
    Ok((Collected::new(values, errors, diagnostics), budget))
}

/// The refusal of a stage whose budget admits nothing at all, or `None` when it admits something
/// (v0.4.1 §21.4, §22.2, ADR-0934).
///
/// §22.2: *"A value of zero means 'no values permitted', not unlimited."* A materializing stage
/// with such a budget cannot produce anything but a refusal, so it gives that refusal before it
/// reads a value: `resource.materialization_limit` is about the input as a whole, which is what
/// errors.yaml says it is, rather than about a count the first value happened to reach. The item
/// ceiling is named when both are zero, because it is the one a user meets first.
#[must_use]
pub(crate) fn admits_nothing(stage: &str, limits: MaterializationLimits) -> Option<ErrorValue> {
    let (setting, ceiling, written) = if limits.max_items() == 0 {
        ("limits.materialize_items", "values", "0 values")
    } else if limits.max_bytes() == 0 {
        ("limits.materialize_bytes", "bytes", "0 bytes")
    } else {
        return None;
    };
    Some(
        ErrorValue::new(
            ErrorCode::ResourceMaterializationLimit,
            format!(
                "`{stage}` must hold its whole input, and its budget admits nothing: `{setting}` \
                 is {written}"
            ),
        )
        .with_retryable(false)
        .with_help(format!(
            "a budget of zero means no values permitted, not unlimited (v0.4.1 §22.2); raise \
             `{setting}` deliberately, or leave `{stage}` out"
        ))
        .with_metadata("stage", Value::string(stage))
        .with_metadata("ceiling", Value::string(ceiling))
        .with_metadata("limit", Value::Int(0))
        .with_metadata("setting", Value::string(setting)),
    )
}
