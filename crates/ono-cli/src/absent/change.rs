//! The change tier of spec v0.6, as a core build has it: not at all (#127, ADR-0910).
//!
//! Mounted at `crate::change` when the `change` feature is off. A mutation in a core build runs
//! the way spec v0.2 describes it — through its provider, with the confirmation the command's
//! privilege asks for — and there is no plan, no protection and no recovery around it. `plan`,
//! `apply`, `recover` and the rest never reach this module: [`crate::absent::claims`] refuses
//! them first.

use ono_parser::Stage;
use ono_value::{ErrorValue, Value};

use crate::absent::{Tier, not_in_build};
use crate::eval::{Eval, Flow};
use crate::session::Session;

/// Nothing to configure, and so nothing wrong with the configuration either.
#[must_use]
pub const fn configure_from(_settings: &crate::settings::Settings) -> Vec<ErrorValue> {
    Vec::new()
}

/// No stage is a `plan` in this build.
#[must_use]
pub const fn claims(_stage: &Stage) -> bool {
    false
}

/// `plan` is not in this build.
///
/// # Errors
///
/// Always `resolve.not_in_build`.
pub fn answer(
    _session: &mut Session,
    _stage: &Stage,
    _source: &str,
    _input: &[Value],
) -> Eval<Vec<Value>> {
    Err(Flow::Failed(not_in_build("plan", Tier::Change)))
}

/// No sealed plan exists to be explained.
///
/// # Errors
///
/// None; the signature matches the tier's.
#[allow(clippy::unnecessary_wraps)]
pub const fn explanation(
    _session: &mut Session,
    _subject: &str,
) -> Result<Option<Vec<String>>, ErrorValue> {
    Ok(None)
}
