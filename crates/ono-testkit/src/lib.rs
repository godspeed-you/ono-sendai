//! Test fixtures and harness helpers shared by every crate in the workspace.
//!
//! Tests in this project assert observable outcomes, never internal structure (AGENTS.md §11).
//! The helpers here exist to make that easy: run the real binary the way a user would, capture
//! exactly what a user would see, and never hang the suite while doing it.
//!
//! ```no_run
//! use ono_testkit::Shell;
//! let run = Shell::new().args(["--version"]).run();
//! run.assert_success();
//! assert!(run.stdout().starts_with("ono "));
//! ```

#![forbid(unsafe_code)]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "this crate is only ever linked into tests, where a failed precondition should abort loudly"
)]

mod rng;
mod run;
mod scratch;

pub use rng::Rng;
pub use run::{Run, RunError, Shell};
pub use scratch::{Scratch, scratch};

use std::path::PathBuf;
use std::time::Duration;

/// Runs `script` through the real binary the way `ono -c <script>` would, and captures what a
/// user would see.
///
/// This is the shape almost every integration test wants, and before it lived here two dozen
/// suites declared it themselves — which is two dozen chances to declare it slightly differently.
/// Use [`Shell`] directly when a test needs an environment, a working directory or standard
/// input; use this when it only needs the answer.
#[must_use]
pub fn ono(script: &str) -> Run {
    Shell::new().args(["-c", script]).run()
}

/// As [`ono`], with an explicit budget for a script that outlives the default one.
///
/// A suite that spawns real children, waits on a live stream or drives a provider that talks to
/// the system needs longer than [`Shell`]'s default. Naming the budget keeps it in the call
/// rather than in a per-file copy of this function.
#[must_use]
pub fn ono_within(script: &str, budget: Duration) -> Run {
    Shell::new().args(["-c", script]).timeout(budget).run()
}

/// Absolute path of the `ono` binary belonging to the current build profile.
///
/// # Panics
///
/// Panics if the binary is missing, which means the test was run without building it.
#[must_use]
pub fn ono_binary() -> PathBuf {
    let mut path = target_dir();
    path.push("ono");
    assert!(
        path.is_file(),
        "the ono binary is not built at {}: run `cargo build` first",
        path.display()
    );
    path
}

/// Directory the current build profile writes its binaries to.
fn target_dir() -> PathBuf {
    // `CARGO_BIN_EXE_*` is only available to the crate that declares the binary, so the path is
    // derived from this crate's manifest instead, which keeps the helper usable everywhere.
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("target");
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path
}
