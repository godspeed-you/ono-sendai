//! The materialization contract of spec v0.4.1 §22: the one place a global operation turns a
//! finite stream into memory.
//!
//! The budget itself lives in `ono-value`, beside the estimator it spends and beside the values
//! it counts, so a component that retains values without owning a stream — the result history of
//! §24 — can share the abstraction §21.1 asks for without depending on the streaming engine
//! (ADR-0453).

use ono_value::{Budget, ErrorValue, Exceeded};

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
