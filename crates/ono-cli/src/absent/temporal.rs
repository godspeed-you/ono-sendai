//! The temporal tier of spec v0.5, as a core build has it: not at all (#127, ADR-0910).
//!
//! Mounted at `crate::temporal` when the `temporal` feature is off. A core session is always in
//! the present: every stage evaluates there, no prompt carries a time marker, no action is
//! recorded in a ledger that does not exist, and a command that names a moment with `--at` is
//! refused rather than answered from the present, because answering today's state for a past
//! instant is the one thing §55.2 forbids outright. `at`, `now` and `present` never reach this
//! module: [`crate::absent::claims`] refuses them first.

use std::sync::Arc;

use jiff::Timestamp;
use ono_parser::Stage;
use ono_temporal_core::TemporalContext;
use ono_value::{ErrorValue, Value};

use crate::absent::{Tier, not_in_build};
use crate::eval::Eval;
use crate::session::Session;

/// The stages the temporal tier would answer; none, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {}

/// No stage is the temporal tier's in this build.
#[must_use]
pub const fn claims(_stage: &Stage) -> Option<Request> {
    None
}

/// Unreachable, since nothing is claimed; it exists so the evaluator reads the same either way.
///
/// # Errors
///
/// None: a [`Request`] cannot be constructed.
pub fn answer(
    _session: &mut Session,
    _stage: &Stage,
    _source: &str,
    request: Request,
) -> Eval<Vec<Value>> {
    match request {}
}

/// Nothing is refused for running outside the present: the session never leaves it.
#[must_use]
pub const fn present_only_refusal(
    _list: &ono_parser::StageList,
    _source: &str,
) -> Option<ErrorValue> {
    None
}

/// Nothing to configure: no temporal setting has a reader in this build.
pub const fn configure_from(_settings: &crate::settings::Settings) {}

/// The present, for every stage — or the refusal of a stage that asks for another instant.
///
/// # Errors
///
/// `resolve.not_in_build` when the stage carries `--at`: the answer from the present would be a
/// claim about the past this build has no evidence for.
#[allow(clippy::unused_async)]
pub async fn invocation_context(
    arguments: &ono_command::BoundArguments,
) -> Result<Arc<TemporalContext>, ErrorValue> {
    match arguments.option("at") {
        Some(Value::Null) | None => Ok(Arc::new(TemporalContext::Present)),
        Some(_) => Err(not_in_build("--at", Tier::Temporal)),
    }
}

/// What happened to an action, for a ledger this build does not keep.
#[derive(Debug, Clone, Copy)]
pub enum Outcome<'a> {
    /// The command ran and answered with one result row per target.
    Acted(&'a [ono_provider_api::ActionOutcome]),
    /// The command refused before any target was touched.
    Refused(&'a ErrorValue),
}

/// Nothing records the action: there is no ledger.
#[allow(clippy::unused_async)]
pub async fn record_action(
    _spelling: &str,
    _operation: &str,
    _arguments: &[String],
    _requested_at: Timestamp,
    _outcome: Outcome<'_>,
) {
}

/// The prompt carries no time marker.
#[must_use]
pub const fn marker_segments() -> Option<(String, String)> {
    None
}
