//! Core types shared across the Ono-Sendai workspace.
//!
//! This crate is intentionally almost empty. It exists so the workspace, the quality gate and
//! the containerised acceptance harness are in place and green before phase A of the
//! specification starts. Everything else is built test-first on top of it.

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
