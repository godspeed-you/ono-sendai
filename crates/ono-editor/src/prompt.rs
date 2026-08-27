//! The prompt, as the semantic segments of spec §4.2 rather than a string of escape codes.
//!
//! The prompt is a HUD: link, privilege, context, path. Handing the editor coloured text would
//! put terminal escapes inside the buffer, where they would ruin every column calculation and
//! every non-TTY render. Handing it *segments* lets the theme paint them, and lets the same
//! prompt render to a pipe with no escapes at all (spec §4.6).

use ono_render::{Presentation, Theme, Token, sanitise};
use unicode_width::UnicodeWidthStr;

/// A prompt, as text segments each carrying the token it should be painted with.
///
/// ```
/// use ono_editor::Prompt;
/// use ono_render::{Presentation, Theme, Token};
/// let prompt = Prompt::plain("local").segment("://~ > ", Token::PromptContext);
/// assert_eq!(prompt.render(&Theme::default(), Presentation::Pipe), "local://~ > ");
/// assert_eq!(prompt.width(), 12);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    segments: Vec<(String, Token)>,
}

impl Prompt {
    /// A prompt showing `text` with no particular token.
    #[must_use]
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            segments: vec![(text.into(), Token::Foreground)],
        }
    }

    /// Appends a segment painted with `token`.
    #[must_use]
    pub fn segment(mut self, text: impl Into<String>, token: Token) -> Self {
        self.segments.push((text.into(), token));
        self
    }

    /// The prompt's width in terminal cells.
    #[must_use]
    pub fn width(&self) -> usize {
        self.segments
            .iter()
            .map(|(text, _)| sanitise(text).width())
            .sum()
    }

    /// The prompt as the terminal should receive it.
    ///
    /// A destination that takes no colour receives the text and nothing else, which is what
    /// makes a redirected session byte-for-byte reproducible.
    #[must_use]
    pub fn render(&self, theme: &Theme, presentation: Presentation) -> String {
        let mut rendered = String::new();
        for (text, token) in &self.segments {
            rendered.push_str(&theme.paint(text, *token, presentation));
        }
        rendered
    }
}

impl Default for Prompt {
    fn default() -> Self {
        Self::plain("> ")
    }
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        Self::plain(text)
    }
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Self::plain(text)
    }
}
