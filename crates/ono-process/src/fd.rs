//! File-descriptor numbers as they appear in shell redirection syntax.

use std::fmt;

/// A descriptor number as written in a redirection such as `2>&1`.
///
/// This is a number from the command line, not a handle: no open descriptor crosses this
/// crate's boundary (ADR-0007).
///
/// ```
/// use ono_process::Fd;
/// assert_eq!(Fd::STDERR.number(), 2);
/// assert_eq!(Fd::new(3).to_string(), "3");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fd(u16);

impl Fd {
    /// Standard input, descriptor 0.
    pub const STDIN: Self = Self(0);
    /// Standard output, descriptor 1.
    pub const STDOUT: Self = Self(1);
    /// Standard error, descriptor 2.
    pub const STDERR: Self = Self(2);

    /// The descriptor with this number.
    #[must_use]
    pub const fn new(number: u16) -> Self {
        Self(number)
    }

    /// The descriptor's number.
    #[must_use]
    pub const fn number(self) -> u16 {
        self.0
    }

    /// The number as the C API spells it.
    pub(crate) const fn raw(self) -> i32 {
        self.0 as i32
    }
}

impl fmt::Display for Fd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
