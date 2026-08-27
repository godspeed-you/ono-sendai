//! Parse diagnostics: what went wrong, where, and what would fix it.

use ono_core::{ErrorCode, Span};

/// One problem the parser found, pointing at the text that caused it.
///
/// The code separates the two states an editor must distinguish (ADR-0009): input that is
/// well-formed but unfinished carries [`ErrorCode::ParseIncomplete`], input that no further
/// typing can rescue carries [`ErrorCode::ParseSyntax`].
///
/// ```
/// use ono_core::ErrorCode;
/// let parsed = ono_parser::parse("echo (get process");
/// assert_eq!(parsed.diagnostics()[0].code(), ErrorCode::ParseIncomplete);
/// assert!(!parsed.is_complete());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    code: ErrorCode,
    span: Span,
    message: String,
    help: Option<String>,
}

impl Diagnostic {
    /// A diagnostic for input that cannot become valid however much more is typed.
    #[must_use]
    pub fn syntax(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::ParseSyntax,
            span,
            message: message.into(),
            help: None,
        }
    }

    /// A diagnostic for input that is well-formed so far but ends inside a construct.
    #[must_use]
    pub fn incomplete(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::ParseIncomplete,
            span,
            message: message.into(),
            help: None,
        }
    }

    /// Attaches the sentence that tells the user what to write instead.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// The stable error identity, one of `parse.syntax` or `parse.incomplete` (spec §43).
    #[must_use]
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The source range the diagnostic points at.
    #[must_use]
    pub fn span(&self) -> Span {
        self.span
    }

    /// The one-line description of the problem.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The suggestion attached to the diagnostic, if there is one.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// Whether this diagnostic only means "not finished yet".
    #[must_use]
    pub fn is_incomplete(&self) -> bool {
        self.code == ErrorCode::ParseIncomplete
    }
}
