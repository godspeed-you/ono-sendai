//! Vocabulary shared by every crate in the Ono-Sendai workspace.
//!
//! `ono-core` is deliberately dependency-free (ADR-0005): the parser, the value model and the
//! process layer are developed against it in parallel, so it holds only what all of them need
//! and nothing that any of them owns.
//!
//! - [`Span`] — byte ranges into a source line, carried by every diagnostic (spec §16.3);
//! - [`ErrorCode`] and [`ErrorKind`] — the stable error taxonomy of spec §43 and §16.1;
//! - [`ExitStatus`] — the exit-status contract of ADR-0008;
//! - [`diagnostic!`] — a line of commentary on standard error that cannot kill the writer.

#![forbid(unsafe_code)]

mod diagnostic;
mod error;
mod exit;
mod span;

pub use error::{ErrorCode, ErrorKind};
pub use exit::ExitStatus;
pub use span::Span;

/// Version of the running shell, taken from the workspace manifest at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Product name in its full form.
pub const PRODUCT_NAME: &str = "Ono-Sendai";

/// Short name, and the name of the binary.
pub const SHORT_NAME: &str = "ono";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_report_a_semver_version_when_asked() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "the shell must report a three-part semantic version, got {VERSION:?}"
        );
        for part in parts {
            assert!(
                part.chars().all(|c| c.is_ascii_digit()),
                "every version component must be numeric, got {VERSION:?}"
            );
        }
    }
}
