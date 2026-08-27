//! The `Regex` semantic scalar of spec §10.2, used by the `~=` operator of spec §6.3.

use std::fmt;

use ono_core::ErrorCode;
use regex::Regex;

use crate::ErrorValue;

/// A compiled regular expression together with the pattern it was written as.
///
/// The pattern text is kept so that the value can be rendered, serialized and compared: two
/// regular expressions are equal when they were written the same way, because a compiled
/// automaton has no meaningful equality.
///
/// ```
/// use ono_value::RegexValue;
/// let pattern = RegexValue::new("^ono-[0-9]+$")?;
/// assert!(pattern.is_match("ono-42"));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Clone)]
pub struct RegexValue {
    source: Box<str>,
    regex: Regex,
}

impl RegexValue {
    /// Compiles a pattern.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::ParseSyntax`] if the pattern does not compile, carrying the
    /// underlying explanation as help text.
    pub fn new(pattern: &str) -> Result<Self, ErrorValue> {
        match Regex::new(pattern) {
            Ok(regex) => Ok(Self {
                source: pattern.into(),
                regex,
            }),
            Err(error) => Err(ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("`{pattern}` is not a valid regular expression"),
            )
            .with_help(error.to_string())),
        }
    }

    /// The pattern as it was written.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The compiled pattern.
    #[must_use]
    pub const fn regex(&self) -> &Regex {
        &self.regex
    }

    /// Whether the pattern matches anywhere in `text`.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        self.regex.is_match(text)
    }
}

impl fmt::Debug for RegexValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("RegexValue").field(&self.source).finish()
    }
}

impl fmt::Display for RegexValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

impl PartialEq for RegexValue {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}
