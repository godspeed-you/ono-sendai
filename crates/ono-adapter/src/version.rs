//! Executable versions and the ranges a contract constrains them to (spec v0.3 §1.46).

use std::fmt;

/// A dotted numeric version, `2.41.3` or `6.15`, compared component-wise.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(Vec<u64>);

impl Version {
    /// Parses `2.41.3`; anything that is not dot-separated integers is `None`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Option<Vec<u64>> = text
            .trim()
            .split('.')
            .map(|part| part.parse::<u64>().ok())
            .collect();
        parts.filter(|parts| !parts.is_empty()).map(Self)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&text.join("."))
    }
}

/// A version constraint as a contract writes it: `any`, `>=2.37`, `>=6.1 <7`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    minimum: Option<Version>,
    below: Option<Version>,
}

impl VersionRange {
    /// Every version at all — a machine protocol that is genuinely version-independent.
    pub const ANY: Self = Self {
        minimum: None,
        below: None,
    };

    /// Parses a range; `None` when the text is not one of the accepted forms.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text == "any" {
            return Some(Self::ANY);
        }
        let mut range = Self::ANY;
        for clause in text.split_whitespace() {
            if let Some(rest) = clause.strip_prefix(">=") {
                range.minimum = Some(Version::parse(rest)?);
            } else if let Some(rest) = clause.strip_prefix('<') {
                range.below = Some(Version::parse(rest)?);
            } else {
                return None;
            }
        }
        (range != Self::ANY).then_some(range)
    }

    /// Whether `version` satisfies the range.
    #[must_use]
    pub fn contains(&self, version: &Version) -> bool {
        self.minimum
            .as_ref()
            .is_none_or(|minimum| version >= minimum)
            && self.below.as_ref().is_none_or(|below| version < below)
    }

    /// Whether the range admits every version.
    #[must_use]
    pub fn is_any(&self) -> bool {
        *self == Self::ANY
    }
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.minimum, &self.below) {
            (None, None) => f.write_str("any"),
            (Some(minimum), None) => write!(f, ">={minimum}"),
            (None, Some(below)) => write!(f, "<{below}"),
            (Some(minimum), Some(below)) => write!(f, ">={minimum} <{below}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_compare_versions_component_wise() {
        let older = Version::parse("2.9").unwrap();
        let newer = Version::parse("2.41.3").unwrap();
        assert!(
            older < newer,
            "2.9 is older than 2.41.3, not lexically newer"
        );
    }

    #[test]
    fn should_bound_a_range_below_and_above() {
        let range = VersionRange::parse(">=6.1 <7").unwrap();
        assert!(range.contains(&Version::parse("6.15").unwrap()));
        assert!(!range.contains(&Version::parse("7.0").unwrap()));
        assert!(!range.contains(&Version::parse("6.0.9").unwrap()));
        assert!(VersionRange::parse("any").unwrap().is_any());
        assert!(VersionRange::parse("~1.2").is_none());
    }
}
