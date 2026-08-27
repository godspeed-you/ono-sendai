//! Loading configuration, in the layers ADR-0010 fixes.
//!
//! The file is Ono source evaluated in a restricted mode: it may set values and define bindings,
//! and it may not run a command, reach the network or load a plugin. A file that cannot be parsed
//! or that asks for something it may not have is reported, and the shell starts anyway — a shell
//! that refuses to start has taken away the tool needed to repair its own configuration.

use std::path::PathBuf;

use ono_value::ErrorValue;

use crate::invocation::Options;
use crate::report::Reporter;
use crate::session::{Mode, Session};

/// Where a setting came from, so `get config` can say (spec §30).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// The system-wide file.
    System,
    /// The user's file.
    User,
    /// A file named by `ONO_CONFIG`, replacing the two above.
    Explicit,
}

/// The files this invocation should read, in order.
#[must_use]
pub fn layers(session: &Session, options: &Options) -> Vec<(Layer, PathBuf)> {
    if options.no_config {
        return Vec::new();
    }
    if let Some(path) = options.config.clone() {
        return vec![(Layer::Explicit, path)];
    }
    if let Some(path) = session.env_var("ONO_CONFIG") {
        return vec![(Layer::Explicit, PathBuf::from(path))];
    }

    let mut found = vec![(Layer::System, PathBuf::from("/etc/ono/config.ono"))];
    if let Some(directory) = user_config_dir(session) {
        found.push((Layer::User, directory.join("config.ono")));
    }
    found
}

/// The user's configuration directory, honouring `ONO_CONFIG_DIR` then XDG (ADR-0010).
#[must_use]
pub fn user_config_dir(session: &Session) -> Option<PathBuf> {
    if let Some(directory) = session.env_var("ONO_CONFIG_DIR") {
        return Some(PathBuf::from(directory));
    }
    if let Some(base) = session.env_var("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(base).join(ono_core::SHORT_NAME));
    }
    session
        .home()
        .map(|home| home.join(".config").join(ono_core::SHORT_NAME))
}

/// The directory for history and other state the user does not edit (ADR-0010).
#[must_use]
pub fn state_dir(session: &Session) -> Option<PathBuf> {
    if let Some(base) = session.env_var("XDG_STATE_HOME") {
        return Some(PathBuf::from(base).join(ono_core::SHORT_NAME));
    }
    session
        .home()
        .map(|home| home.join(".local").join("state").join(ono_core::SHORT_NAME))
}

/// Reads every configuration layer into `session`.
///
/// Problems are reported and never fatal.
pub fn load(session: &mut Session, options: &Options, reporter: &Reporter) {
    for (_, path) in layers(session, options) {
        let source = match std::fs::read_to_string(&path) {
            Ok(source) => source,
            // A file that is simply not there is the ordinary case, not a problem.
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                reporter.error(&crate::builtin::io_error(&path, &error));
                continue;
            }
        };

        let parsed = ono_parser::parse(&source);
        for diagnostic in parsed.diagnostics() {
            reporter.diagnostic(&source, diagnostic);
        }
        if parsed.has_errors() {
            continue;
        }

        session.in_mode(Mode::Config, |session| {
            let mut report = |error: &ErrorValue| reporter.error(error);
            crate::eval::run_program(session, parsed.program(), &source, &mut report);
        });

        // A configuration file cannot end the session. `exit` in one is already refused as a
        // policy violation, but a request to leave must not survive the load under any
        // circumstance: it would replace the status of every command the shell later runs and
        // short-circuit every statement after the first. ADR-0010 promises that a bad setting
        // never stops the shell from starting, and that promise has to hold for a *hostile*
        // setting too.
        session.stay();
    }
}
