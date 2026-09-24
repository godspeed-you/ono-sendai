//! The spatial tier of spec v0.4, as a core build has it: not at all (#127, ADR-0910).
//!
//! Mounted at `crate::spatial` when the `spatial` feature is off. It holds what the rest of the
//! shell asks of the tier on its own path — whether a place is being observed, what the prompt
//! shows, whether `cd` moves a place — and answers as a session in which no place exists. The
//! spatial commands themselves never reach it: [`crate::absent::claims`] refuses them first.

use std::path::Path;

use ono_value::{ErrorValue, RecordValue, Value};

use crate::absent::{Tier, not_in_build};
use crate::session::Session;

/// Nothing to configure: no spatial setting has a reader in this build.
pub const fn configure_from(_settings: &crate::settings::Settings) {}

/// The spatial layer is off, because it is not here.
#[must_use]
pub const fn disabled(_session: &Session) -> bool {
    true
}

/// The refusal a spatial verb answers with: the tier is not in this build.
#[must_use]
pub fn switched_off(command: &str) -> ErrorValue {
    not_in_build(command, Tier::Spatial)
}

/// `help here` describes the current place, and there is none.
///
/// # Errors
///
/// Always `resolve.not_in_build`.
pub fn here_help(_session: &mut Session) -> crate::eval::Eval<ono_command::TopicHelp> {
    Err(crate::eval::Flow::Failed(not_in_build(
        "help here",
        Tier::Spatial,
    )))
}

/// Nothing observes the entered object.
#[allow(clippy::unused_async)]
pub async fn enter_observed(_record: &RecordValue) {}

/// Nothing observes what an adapter decoded.
#[allow(clippy::unused_async)]
pub async fn observe_adapted(_values: &[Value]) {}

/// No full-screen spatial view exists to be allowed onto the terminal.
pub const fn mark_interactive() {}

/// The prompt has no place segment.
#[must_use]
pub const fn place_segment() -> Option<String> {
    None
}

/// The completion offers of the spatial verbs, which this build does not have.
pub mod complete {
    /// A completion offer; none is ever made.
    #[derive(Debug, Clone)]
    pub struct Offer {
        /// The text that replaces the word under the cursor.
        pub insert: String,
        /// The line the listing shows.
        pub line: String,
    }

    /// No place is visible from here.
    #[must_use]
    pub const fn places_here() -> Vec<Offer> {
        Vec::new()
    }

    /// No relation leaves from here.
    #[must_use]
    pub const fn relations_here() -> Vec<Offer> {
        Vec::new()
    }
}

/// The filesystem side of the spatial layer.
pub mod storage {
    use super::{Path, Session};

    /// `cd` moves the working directory and nothing else.
    pub const fn follow_cwd(_session: &mut Session, _destination: &Path) {}

    /// No word is entered as a path place: `enter <path>` is a spatial command.
    #[must_use]
    pub const fn looks_like_a_path(_word: &str) -> bool {
        false
    }
}
