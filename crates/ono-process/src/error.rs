//! The structured failures this crate reports, in the taxonomy of spec §43.

use std::fmt;
use std::io;

use nix::errno::Errno;
use ono_core::ErrorCode;

/// The result type used throughout this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// A failure that stopped a command from running, or an operation on a job from succeeding.
///
/// The [`ErrorCode`] is the machine-readable identity a caller matches on; the message carries
/// the operating system's own wording so nothing is lost on the way to the user.
///
/// ```
/// use ono_core::ErrorCode;
/// use ono_process::Error;
///
/// let error = Error::new(ErrorCode::IoNotFound, "no such file: /nope");
/// assert_eq!(error.code(), ErrorCode::IoNotFound);
/// assert_eq!(error.to_string(), "no such file: /nope");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    code: ErrorCode,
    message: String,
}

impl Error {
    /// Builds an error with an explicit code and message.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// The stable identity of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.code
    }

    /// The human-readable description, including the operating system's own wording.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Builds an error from an I/O failure, prefixed with what was being attempted.
    pub(crate) fn from_io(context: impl fmt::Display, error: &io::Error) -> Self {
        Self::new(code_for_io(error), format!("{context}: {error}"))
    }

    /// Builds an error from a raw system call failure.
    pub(crate) fn from_errno(context: impl fmt::Display, errno: Errno) -> Self {
        Self::from_io(context, &io::Error::from_raw_os_error(errno as i32))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Maps an I/O failure onto the closed taxonomy of spec §43.
///
/// The taxonomy has no generic I/O code, so anything the four specific codes do not describe is
/// reported as `io.permission_denied`: the operating system refused the operation. The message
/// always carries the real reason.
fn code_for_io(error: &io::Error) -> ErrorCode {
    match error.kind() {
        io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        io::ErrorKind::AlreadyExists => ErrorCode::IoAlreadyExists,
        io::ErrorKind::NotADirectory => ErrorCode::IoNotDirectory,
        _ => ErrorCode::IoPermissionDenied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_not_found_when_the_system_says_the_path_is_missing() {
        let error = Error::from_io(
            "opening /nope",
            &io::Error::from_raw_os_error(libc_enoent()),
        );
        assert_eq!(error.code(), ErrorCode::IoNotFound);
        assert!(error.message().starts_with("opening /nope: "));
    }

    #[test]
    fn should_report_not_a_directory_when_a_path_component_is_a_file() {
        let error = Error::from_io("opening /etc/hosts/x", &io::Error::from_raw_os_error(20));
        assert_eq!(error.code(), ErrorCode::IoNotDirectory);
    }

    #[test]
    fn should_fall_back_to_permission_denied_for_an_unclassified_system_failure() {
        // EMFILE: too many open files. The taxonomy has no generic I/O code.
        let error = Error::from_io("opening a pipe", &io::Error::from_raw_os_error(24));
        assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
        assert!(error.message().contains("opening a pipe"));
    }

    const fn libc_enoent() -> i32 {
        2
    }
}
