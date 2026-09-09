//! The real [`ToolRunner`]: a program, an argument vector and no shell (§12.3, §2.17).
//!
//! §2.17 requires provider operations to be structured execution plans rather than interpolated
//! command strings, and §12.3 says the same thing about the seam a provider runs a tool through.
//! [`ProcessRunner`] is that seam made real with [`std::process::Command`], which calls `execvp`
//! directly: there is no `sh -c`, so a subvolume path containing a semicolon, a backtick or a
//! newline is a path containing a semicolon, a backtick or a newline.
//!
//! Two smaller decisions are deliberate. The environment is trimmed to a fixed `LC_ALL=C` and
//! `PATH`, because every parser in this crate reads English field names and a localised
//! `btrfs-progs` would otherwise change what "Subvolume ID" is called underneath a provider that
//! never asked. And a failed program is a [`ToolOutput`] with a non-zero status rather than an
//! error, exactly as [`ToolRunner`] specifies: `ERROR: Not a Btrfs subvolume` is an answer to
//! Appendix B.9's question, and swallowing it as a failure would lose it.

use std::process::Command;
use std::sync::Arc;

use ono_change_core::{ToolOutput, ToolRunner, error};
use ono_value::ErrorValue;

/// The `PATH` a provider-run tool is given.
///
/// A tool is looked up along a fixed list rather than along the caller's environment: §43 treats
/// the search path a privileged operation resolves a program on as part of the operation.
pub const TOOL_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

/// Runs programs for real (§12.3).
#[derive(Debug, Clone)]
pub struct ProcessRunner {
    path: Arc<str>,
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self {
            path: Arc::from(TOOL_PATH),
        }
    }
}

impl ProcessRunner {
    /// A runner that looks programs up along [`TOOL_PATH`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A runner that looks programs up along `path`.
    #[must_use]
    pub fn searching(path: impl Into<Arc<str>>) -> Self {
        Self { path: path.into() }
    }

    /// The absolute path of `program`, searching [`ProcessRunner::searching`]'s list.
    ///
    /// A program named with a `/` in it is taken as given; anything else is resolved against the
    /// fixed list, so `btrfs` cannot be answered by whatever happens to be earlier on an inherited
    /// `PATH`.
    #[must_use]
    pub fn resolve(&self, program: &str) -> Option<std::path::PathBuf> {
        if program.contains('/') {
            let candidate = std::path::PathBuf::from(program);
            return candidate.is_file().then_some(candidate);
        }
        self.path
            .split(':')
            .map(|directory| std::path::Path::new(directory).join(program))
            .find(|candidate| candidate.is_file())
    }
}

impl ToolRunner for ProcessRunner {
    fn run(&self, program: &str, argv: &[&str]) -> Result<ToolOutput, ErrorValue> {
        let Some(resolved) = self.resolve(program) else {
            return Err(error::tool_failed(
                program,
                &format!("no such program on {}", self.path),
            ));
        };
        let output = Command::new(&resolved)
            .args(argv)
            .env_clear()
            .env("PATH", self.path.as_ref())
            .env("LC_ALL", "C")
            .output()
            .map_err(|failure| {
                error::tool_failed(program, &format!("the program could not be run: {failure}"))
            })?;
        Ok(ToolOutput::new(
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    fn is_available(&self, program: &str) -> bool {
        self.resolve(program).is_some()
    }
}
