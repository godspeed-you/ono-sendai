//! KUANG/11 package management, as a build without the runtime has it (#127, ADR-0910).
//!
//! Mounted at `crate::plugins` when the `kuang` feature is off. No package is installed, loaded
//! or declared, so nothing here claims a stage or finds a contribution; the entry points the
//! evaluator reaches on its own path refuse with `resolve.not_in_build`. `load plugin`, `get
//! plugin` and `<package>:<command>` never get this far: [`crate::absent::claims`] refuses them
//! first.

use ono_command::CommandContract;
use ono_core::ExitStatus;
use ono_value::{ErrorValue, Value};

use crate::absent::{Tier, not_in_build};
use crate::eval::{Eval, Flow};
use crate::session::Session;

/// The management commands the runtime would run in the shell; none, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {}

/// What a management command produced.
#[derive(Debug)]
pub struct Produced {
    /// The records the rest of the pipeline is seeded with.
    pub values: Vec<Value>,
    /// A failure to report after the values, which fails the run.
    pub failure: Option<ErrorValue>,
}

/// What a contributed command answered.
pub enum Answered {
    /// A finite answer.
    Values(Vec<Value>),
    /// An answer that continues until the operator ends it.
    Live(ono_pipeline::ValueStream),
}

/// What `load plugin` was told besides the package.
#[derive(Debug, Default, Clone)]
pub struct LoadOptions;

impl LoadOptions {
    /// The package a `load plugin` names, which this build cannot load.
    #[must_use]
    pub fn from_words(words: &[String]) -> (Option<String>, Self) {
        let id = words
            .iter()
            .find(|word| *word != "plugin" && !word.starts_with("--"))
            .cloned();
        (id, Self)
    }
}

fn refusal(what: &str) -> Flow {
    Flow::Failed(not_in_build(what, Tier::Kuang))
}

/// No stage is a management command in this build.
#[must_use]
pub const fn claims(_stage: &ono_parser::Stage) -> Option<Request> {
    None
}

/// Unreachable, since nothing is claimed.
///
/// # Errors
///
/// None: a [`Request`] cannot be constructed.
pub fn run(_session: &mut Session, request: Request, _words: &[String]) -> Eval<Produced> {
    match request {}
}

/// Unreachable, since nothing is claimed.
///
/// # Errors
///
/// None: a [`Request`] cannot be constructed.
pub fn run_piped(
    _session: &mut Session,
    request: Request,
    _words: &[String],
    _targets: &[Value],
) -> Eval<Produced> {
    match request {}
}

/// `load plugin` is not in this build.
///
/// # Errors
///
/// Always `resolve.not_in_build`.
pub fn load_piped(
    _session: &mut Session,
    _words: &[String],
    _targets: &[Value],
) -> Eval<ExitStatus> {
    Err(refusal("load plugin"))
}

/// `load plugin` is not in this build.
///
/// # Errors
///
/// Always `resolve.not_in_build`.
pub fn load_plugin_with(
    _session: &mut Session,
    _id: &str,
    _options: &LoadOptions,
) -> Eval<ExitStatus> {
    Err(refusal("load plugin"))
}

/// No package is loaded.
#[must_use]
pub const fn loaded_package(_session: &Session, _namespace: &str) -> Option<String> {
    None
}

/// No package can be invoked.
///
/// # Errors
///
/// Always `resolve.not_in_build`.
pub fn invoke(
    _session: &mut Session,
    namespace: &str,
    command: &str,
    _words: &[std::ffi::OsString],
) -> Eval<Vec<Value>> {
    Err(refusal(&format!("{namespace}:{command}")))
}

/// No package declared a command.
#[must_use]
pub const fn contributed_command(_stage: &ono_parser::Stage) -> Option<&'static CommandContract> {
    None
}

/// No package declared a command.
#[must_use]
pub const fn contributed_by_namespace(
    _namespace: &str,
    _command: &str,
) -> Option<&'static CommandContract> {
    None
}

/// No package declared a command.
///
/// # Errors
///
/// Always `resolve.not_in_build`.
pub fn invoke_contributed(
    _session: &mut Session,
    contract: &CommandContract,
    _words: &[std::ffi::OsString],
) -> Eval<Answered> {
    Err(refusal(&contract.spelling()))
}
