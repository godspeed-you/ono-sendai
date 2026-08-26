//! Semantic visual tokens and the theme that paints them.
//!
//! Spec §44: themes operate on semantic tokens, not hard-coded command colours, and "no
//! functionality may depend on color alone" — so a token may also carry a text marker that
//! survives a monochrome terminal, a pipe and a colour-blind reader.

use std::fmt::Write as _;

use crate::Presentation;

/// Builds [`Token`] together with the name spec §44 gives it.
macro_rules! tokens {
    ($( $variant:ident => $name:literal, $doc:literal; )*) => {
        /// A semantic role a piece of output plays, from spec §44.
        ///
        /// Output asks for a token; the theme decides what that looks like. Nothing in the shell
        /// names a colour.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Token {
            $( #[doc = $doc] $variant, )*
        }

        impl Token {
            /// Every token of spec §44, in the order it lists them.
            pub const ALL: &'static [Token] = &[ $( Token::$variant, )* ];

            /// The token's name, spelled as spec §44 spells it.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self { $( Token::$variant => $name, )* }
            }
        }
    };
}

tokens! {
    Foreground => "ui.fg", "Ordinary values and prose.";
    Dim => "ui.dim", "Metadata, provenance and secondary columns.";
    Accent => "ui.accent", "The selected object, the active link, a relationship edge.";
    Success => "ui.success", "A completed state change worth confirming.";
    Warning => "ui.warning", "A degraded or risky state.";
    Danger => "ui.danger", "A destructive action, privilege escalation, a critical error.";
    Border => "ui.border", "Rules and frames.";
    Selection => "ui.selection", "The interactive selection cursor.";
    PromptLink => "ui.prompt.link", "The machine that will execute native operations.";
    PromptContext => "ui.prompt.context", "The selected system object in the prompt.";
    PromptRoot => "ui.prompt.root", "An elevated identity, which must be impossible to miss.";
    TableHeader => "ui.table.header", "A column header.";
    TableKey => "ui.table.key", "A field name in a stacked record.";
    ValueString => "ui.value.string", "A string value.";
    ValueNumber => "ui.value.number", "A numeric value.";
    ValueUnit => "ui.value.unit", "The unit part of a semantic scalar.";
    ValueNull => "ui.value.null", "An unknown value — never an empty string (spec §10.5).";
    ErrorCode => "ui.error.code", "The stable error code of spec §43.";
    ErrorHint => "ui.error.hint", "The help line of a structured error.";
    GraphNode => "ui.graph.node", "A node in a relationship graph.";
    GraphEdge => "ui.graph.edge", "An observed relationship.";
    GraphEdgeInferred => "ui.graph.edge_inferred", "An inferred relationship (spec §22.2).";
    Path => "ui.value.path", "A filesystem path.";
    Timestamp => "ui.value.timestamp", "A point in time.";
}

impl Token {
    /// Resolves a token from its name, as a theme file names it.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|token| token.name() == name)
    }
}

/// A colour from the 256-colour palette, or none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Color {
    /// The terminal's own foreground.
    #[default]
    Default,
    /// An indexed colour, which every terminal worth supporting understands.
    Indexed(u8),
}

/// How one token is painted.
///
/// `marker` is what a reader sees when there is no colour at all, which is what keeps the shell
/// usable in a pipe, on a monochrome terminal and for a reader who cannot distinguish the hues
/// (spec §44).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Style {
    color: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    marker: Option<&'static str>,
}

impl Style {
    /// A style using the given colour.
    #[must_use]
    pub const fn color(color: Color) -> Self {
        Self {
            color,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            marker: None,
        }
    }

    /// Emphasised.
    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// De-emphasised.
    #[must_use]
    pub const fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    /// Underlined.
    #[must_use]
    pub const fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Adds a text marker that survives a destination with no colour.
    #[must_use]
    pub const fn with_marker(mut self, marker: &'static str) -> Self {
        self.marker = Some(marker);
        self
    }

    /// The colour this style paints with.
    #[must_use]
    pub const fn colour(self) -> Color {
        self.color
    }

    /// The text marker that carries this style's meaning without colour, if it has one.
    #[must_use]
    pub const fn marker(self) -> Option<&'static str> {
        self.marker
    }

    /// The ANSI escape sequence that begins this style, or `None` when it paints nothing.
    #[must_use]
    fn ansi_prefix(self) -> Option<String> {
        let mut codes: Vec<String> = Vec::new();
        if self.bold {
            codes.push("1".to_owned());
        }
        if self.dim {
            codes.push("2".to_owned());
        }
        if self.italic {
            codes.push("3".to_owned());
        }
        if self.underline {
            codes.push("4".to_owned());
        }
        if let Color::Indexed(index) = self.color {
            codes.push(format!("38;5;{index}"));
        }
        if codes.is_empty() {
            None
        } else {
            let mut sequence = String::from("\u{1b}[");
            for (position, code) in codes.iter().enumerate() {
                if position > 0 {
                    sequence.push(';');
                }
                sequence.push_str(code);
            }
            sequence.push('m');
            Some(sequence)
        }
    }
}

/// The mapping from semantic tokens to styles.
///
/// The default theme is dark, restrained and legible, as spec §44 requires of the theme that
/// ships. A stylised theme is a different mapping, not a different mechanism.
#[derive(Debug, Clone)]
pub struct Theme {
    name: String,
    styles: Vec<(Token, Style)>,
}

impl Theme {
    /// The theme's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The style for `token`.
    #[must_use]
    pub fn style(&self, token: Token) -> Style {
        self.styles
            .iter()
            .find(|(candidate, _)| *candidate == token)
            .map_or_else(Style::default, |(_, style)| *style)
    }

    /// Overrides one token's style, as a theme file or a user setting does.
    pub fn set(&mut self, token: Token, style: Style) {
        match self
            .styles
            .iter_mut()
            .find(|(candidate, _)| *candidate == token)
        {
            Some(entry) => entry.1 = style,
            None => self.styles.push((token, style)),
        }
    }

    /// Renders `text` in `token`'s style, as far as `presentation` allows.
    ///
    /// Control characters in `text` are neutralised whatever the destination: a value read from
    /// the system is data, and must never be able to drive the terminal (spec §49,
    /// `docs/ACCEPTANCE.md` §4.4).
    ///
    /// ```
    /// use ono_render::{Presentation, Theme, Token};
    /// let theme = Theme::default();
    /// assert_eq!(theme.paint("nginx", Token::ValueString, Presentation::Pipe), "nginx");
    /// ```
    #[must_use]
    pub fn paint(&self, text: &str, token: Token, presentation: Presentation) -> String {
        let safe = sanitise(text);
        if !presentation.allows_color() {
            return safe;
        }
        match self.style(token).ansi_prefix() {
            Some(prefix) => format!("{prefix}{safe}\u{1b}[0m"),
            None => safe,
        }
    }
}

/// Replaces every control character with a visible, inert representation.
///
/// Escape sequences, carriage returns and backspaces let a value rewrite the screen, retitle the
/// window or hide what came before it. A shell that prints system data must assume that data is
/// hostile, so nothing here is conditional on a policy setting.
///
/// **A newline and a tab are control characters too, and they are escaped like the rest.** An
/// earlier version let them through on the grounds that they are ordinary text — but a table cell
/// holding `"evil\nroot      1"` then rendered as two terminal lines, the second of them
/// indistinguishable from a real row. A value cannot be allowed to forge the frame it is being
/// displayed in, and a filename may contain a newline, so this is reachable by anyone who can
/// create a file.
///
/// A caller with genuinely multi-line output splits it into lines and sanitises each, which is
/// what every renderer in this crate does. Nothing that needs the layout to survive can afford to
/// have a value decide where its lines end.
#[must_use]
pub fn sanitise(text: &str) -> String {
    if !text.chars().any(char::is_control) {
        return text.to_owned();
    }
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        if character.is_control() {
            // `\u{1b}` becomes a printable escape rather than vanishing, so a value that
            // contained one is still visibly different from one that did not.
            let _ = write!(safe, "\\u{{{:x}}}", character as u32);
        } else {
            safe.push(character);
        }
    }
    safe
}

impl Default for Theme {
    fn default() -> Self {
        // Palette: 256-colour indices chosen for legibility on a dark background, none of them
        // relied on alone — every token whose meaning matters also carries a marker.
        Self {
            name: "ono".to_owned(),
            styles: vec![
                (Token::Foreground, Style::color(Color::Default)),
                (Token::Dim, Style::color(Color::Indexed(244)).dim()),
                (Token::Accent, Style::color(Color::Indexed(38)).bold()),
                (
                    Token::Success,
                    Style::color(Color::Indexed(78)).with_marker("ok"),
                ),
                (
                    Token::Warning,
                    Style::color(Color::Indexed(179)).with_marker("!"),
                ),
                (
                    Token::Danger,
                    Style::color(Color::Indexed(203)).bold().with_marker("!!"),
                ),
                (Token::Border, Style::color(Color::Indexed(240)).dim()),
                (
                    Token::Selection,
                    Style::color(Color::Indexed(38))
                        .underline()
                        .with_marker(">"),
                ),
                (Token::PromptLink, Style::color(Color::Indexed(38))),
                (Token::PromptContext, Style::color(Color::Indexed(109))),
                (
                    Token::PromptRoot,
                    Style::color(Color::Indexed(203)).bold().with_marker("!"),
                ),
                (Token::TableHeader, Style::color(Color::Indexed(250)).bold()),
                (Token::TableKey, Style::color(Color::Indexed(244)).dim()),
                (Token::ValueString, Style::color(Color::Default)),
                (Token::ValueNumber, Style::color(Color::Indexed(180))),
                (Token::ValueUnit, Style::color(Color::Indexed(244)).dim()),
                (
                    Token::ValueNull,
                    Style::color(Color::Indexed(240)).dim().with_marker("null"),
                ),
                (Token::ErrorCode, Style::color(Color::Indexed(203)).bold()),
                (Token::ErrorHint, Style::color(Color::Indexed(244)).dim()),
                (Token::GraphNode, Style::color(Color::Default)),
                (Token::GraphEdge, Style::color(Color::Indexed(38))),
                (
                    Token::GraphEdgeInferred,
                    Style::color(Color::Indexed(244)).dim().with_marker("~"),
                ),
                (Token::Path, Style::color(Color::Indexed(109))),
                (Token::Timestamp, Style::color(Color::Indexed(244))),
            ],
        }
    }
}
