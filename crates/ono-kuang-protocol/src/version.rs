//! The host API version and the ranges a manifest declares against it (spec §31.62, §31.63).
//!
//! The `11` is KUANG/11's major version and does not move; the minor is what negotiation
//! resolves at load. A manifest writes a range — spec §31.5's `">=11.1 <12"` — and the host
//! answers with the single version both sides then speak (`docs/spec/kuang/protocol.v1.yaml`).

use std::fmt;
use std::str::FromStr;

use crate::{KuangError, KuangErrorCode};

/// The host API version this build implements: `kuang-host/11.1` (ADR-0022 §9).
pub const HOST_API: ApiVersion = ApiVersion {
    major: 11,
    minor: 1,
};

/// The value protocol values cross the boundary in (spec §31.62).
pub const VALUE_PROTOCOL: &str = "ono-value/1";

/// The package format this build reads (spec §31.7).
pub const PACKAGE_FORMAT: &str = "kuang-package/1";

/// A `major.minor` API version, e.g. `11.1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ApiVersion {
    /// The major version. For KUANG/11 it is always 11.
    pub major: u32,
    /// The minor version, resolved by negotiation at load (spec §31.63).
    pub minor: u32,
}

impl ApiVersion {
    /// The version rendered as the protocol id, e.g. `kuang-host/11.1`.
    #[must_use]
    pub fn protocol_id(&self) -> String {
        format!("kuang-host/{self}")
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

impl FromStr for ApiVersion {
    type Err = KuangError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = || {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("`{text}` is not a `major.minor` API version"),
            )
        };
        match text.split_once('.') {
            Some((major, minor)) => Ok(Self {
                major: major.parse().map_err(|_| error())?,
                minor: minor.parse().map_err(|_| error())?,
            }),
            None => Ok(Self {
                major: text.parse().map_err(|_| error())?,
                minor: 0,
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparator {
    GreaterEq,
    Greater,
    LessEq,
    Less,
    Exact,
}

/// A version range as a manifest writes it, e.g. `">=11.1 <12"` (spec §31.5, §31.7).
///
/// A range is a space-separated conjunction of comparators; every comparator must hold.
///
/// ```
/// use ono_kuang_protocol::{ApiVersion, VersionRange};
/// let range: VersionRange = ">=11.1 <12".parse()?;
/// assert!(range.contains(ApiVersion { major: 11, minor: 1 }));
/// assert!(!range.contains(ApiVersion { major: 12, minor: 0 }));
/// # Ok::<(), ono_kuang_protocol::KuangError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    source: String,
    clauses: Vec<(Comparator, ApiVersion)>,
}

impl VersionRange {
    /// Whether `version` satisfies every comparator of the range.
    #[must_use]
    pub fn contains(&self, version: ApiVersion) -> bool {
        self.clauses.iter().all(|(comparator, bound)| {
            let key = (version.major, version.minor);
            let bound = (bound.major, bound.minor);
            match comparator {
                Comparator::GreaterEq => key >= bound,
                Comparator::Greater => key > bound,
                Comparator::LessEq => key <= bound,
                Comparator::Less => key < bound,
                Comparator::Exact => key == bound,
            }
        })
    }

    /// The range exactly as the manifest wrote it.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl FromStr for VersionRange {
    type Err = KuangError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let error = |detail: &str| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("`{text}` is not a version range: {detail}"),
            )
        };
        let mut clauses = Vec::new();
        for part in text.split_whitespace() {
            let (comparator, rest) = if let Some(rest) = part.strip_prefix(">=") {
                (Comparator::GreaterEq, rest)
            } else if let Some(rest) = part.strip_prefix("<=") {
                (Comparator::LessEq, rest)
            } else if let Some(rest) = part.strip_prefix('>') {
                (Comparator::Greater, rest)
            } else if let Some(rest) = part.strip_prefix('<') {
                (Comparator::Less, rest)
            } else if let Some(rest) = part.strip_prefix('=') {
                (Comparator::Exact, rest)
            } else {
                (Comparator::Exact, part)
            };
            clauses.push((comparator, rest.parse()?));
        }
        if clauses.is_empty() {
            return Err(error("it names no version at all"));
        }
        Ok(Self {
            source: text.to_owned(),
            clauses,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_accept_the_host_version_when_the_manifest_range_covers_it() {
        let range: VersionRange = ">=11.1 <12".parse().expect("range parses");
        assert!(range.contains(HOST_API));
    }

    #[test]
    fn should_refuse_a_version_outside_the_range_when_checked() {
        let range: VersionRange = ">=11.2 <12".parse().expect("range parses");
        assert!(!range.contains(HOST_API), "11.1 is below >=11.2");
        let range: VersionRange = "<11".parse().expect("range parses");
        assert!(!range.contains(HOST_API));
    }

    #[test]
    fn should_treat_a_bare_version_as_exact_when_parsing() {
        let range: VersionRange = "11.1".parse().expect("range parses");
        assert!(range.contains(ApiVersion {
            major: 11,
            minor: 1
        }));
        assert!(!range.contains(ApiVersion {
            major: 11,
            minor: 2
        }));
    }

    #[test]
    fn should_fail_with_package_invalid_when_the_range_is_gibberish() {
        let error = "not-a-range".parse::<VersionRange>().unwrap_err();
        assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
        let error = "".parse::<VersionRange>().unwrap_err();
        assert_eq!(error.code(), KuangErrorCode::PackageInvalid);
    }
}
