//! Remote links, as a build without the remote tier has them: none (#127, ADR-0910).
//!
//! Mounted at `crate::remote` when the `remote` feature is off. No link is defined, so no stage
//! is the link table's to answer. `link host`, `connect host`, `get link` and the key commands
//! never get this far: [`crate::absent::claims`] refuses them first.

use ono_parser::Stage;
use ono_value::{ErrorValue, Value};

use crate::absent::{Tier, not_in_build};
use crate::eval::Eval;
use crate::session::Session;

/// The link-table commands the remote tier answers in the shell; none, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {}

/// No stage is the link table's in this build.
#[must_use]
pub const fn claims(_stage: &Stage) -> Option<Request> {
    None
}

/// Unreachable, since nothing is claimed.
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

/// Unreachable, since nothing is claimed.
///
/// # Errors
///
/// None: a [`Request`] cannot be constructed.
pub fn answer_piped(
    _session: &mut Session,
    _stage: &Stage,
    request: Request,
    _targets: &[Value],
) -> Eval<Vec<Value>> {
    match request {}
}

/// The refusal of a remote command reached through a pipe: the tier is not in this build.
#[must_use]
pub fn no_stream_input(spelling: &str, _target: &str) -> ErrorValue {
    not_in_build(spelling, Tier::Remote)
}
