//! The exit-status contract of ADR-0008.

use std::fmt;

/// The status `ono` reports to whatever started it, and the status a command reports inside it.
///
/// Ono uses the Bourne-family conventions because every tool that consumes a shell's status
/// already assumes them (ADR-0008). An external program's own status passes through unchanged,
/// as spec §16.4 requires, including values that collide with these conventions.
///
/// ```
/// use ono_core::ExitStatus;
/// assert_eq!(ExitStatus::from_signal(9).code(), 137);
/// assert!(ExitStatus::SUCCESS.is_success());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExitStatus(u8);

impl ExitStatus {
    /// The command succeeded.
    pub const SUCCESS: Self = Self(0);
    /// The command ran and failed: a structured error, or a test that was false.
    pub const FAILURE: Self = Self(1);
    /// The command line could not be understood: a usage or parse error.
    pub const USAGE: Self = Self(2);
    /// The command was found but could not be executed.
    pub const NOT_EXECUTABLE: Self = Self(126);
    /// The command could not be resolved at all.
    pub const NOT_FOUND: Self = Self(127);
    /// The foreground command was interrupted, the `128 + SIGINT` case.
    pub const INTERRUPTED: Self = Self(130);

    /// Wraps a raw status a program chose for itself, unchanged (spec §16.4).
    #[must_use]
    pub const fn from_code(code: u8) -> Self {
        Self(code)
    }

    /// The status for a process terminated by `signal`, as `128 + signal`.
    ///
    /// Signals above 127 cannot be represented in a wait status and saturate rather than wrap,
    /// so a nonsensical signal number can never be mistaken for success.
    #[must_use]
    pub const fn from_signal(signal: u8) -> Self {
        Self(128u8.saturating_add(signal))
    }

    /// The raw status value.
    #[must_use]
    pub const fn code(self) -> u8 {
        self.0
    }

    /// Whether the command succeeded, which is true only for zero.
    #[must_use]
    pub const fn is_success(self) -> bool {
        self.0 == 0
    }

    /// The signal that terminated the process, if this status describes one.
    #[must_use]
    pub const fn signal(self) -> Option<u8> {
        if self.0 > 128 {
            Some(self.0 - 128)
        } else {
            None
        }
    }
}

impl From<ExitStatus> for std::process::ExitCode {
    fn from(status: ExitStatus) -> Self {
        std::process::ExitCode::from(status.0)
    }
}

impl fmt::Display for ExitStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
