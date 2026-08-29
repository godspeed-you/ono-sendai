//! Semantic visual tokens and the theme that paints them.
//!
//! Spec §44: themes operate on semantic tokens, not hard-coded command colours, and "no
//! functionality may depend on color alone" — so a token may also carry a text marker that
//! survives a monochrome terminal, a pipe and a colour-blind reader.

use std::fmt::Write as _;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_value::ErrorValue;

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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    color: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    marker: Option<Arc<str>>,
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

    /// The same style with another colour, as a theme file overriding one key produces.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Emphasised, or not, as a theme file says.
    #[must_use]
    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }

    /// De-emphasised, or not, as a theme file says.
    #[must_use]
    pub fn with_dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    /// Underlined, or not, as a theme file says.
    #[must_use]
    pub fn with_underline(mut self, underline: bool) -> Self {
        self.underline = underline;
        self
    }

    /// Emphasised.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// De-emphasised.
    #[must_use]
    pub fn dim(mut self) -> Self {
        self.dim = true;
        self
    }

    /// Underlined.
    #[must_use]
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Adds a text marker that survives a destination with no colour.
    #[must_use]
    pub fn with_marker(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.marker = Some(marker.into());
        self
    }

    /// The colour this style paints with.
    #[must_use]
    pub const fn colour(&self) -> Color {
        self.color
    }

    /// The text marker that carries this style's meaning without colour, if it has one.
    #[must_use]
    pub fn marker(&self) -> Option<&str> {
        self.marker.as_deref()
    }

    /// The ANSI escape sequence that begins this style, or `None` when it paints nothing.
    #[must_use]
    fn ansi_prefix(&self) -> Option<String> {
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
            .map_or_else(Style::default, |(_, style)| style.clone())
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

/// The names of every theme this build ships.
const BUILT_IN: [&str; 2] = ["ono", "neon"];

/// The longest a marker may be: it sits in a table cell beside the value it marks.
const MARKER_LIMIT: usize = 4;

impl Theme {
    /// The themes this build ships, in the order `get config theme.name` should suggest them.
    ///
    /// ```
    /// assert!(ono_render::Theme::builtin_names().contains(&"ono"));
    /// ```
    #[must_use]
    pub fn builtin_names() -> &'static [&'static str] {
        &BUILT_IN
    }

    /// One of the themes this build ships, by name.
    ///
    /// ```
    /// let theme = ono_render::Theme::named("ono").expect("the default theme");
    /// assert_eq!(theme.name(), "ono");
    /// ```
    #[must_use]
    pub fn named(name: &str) -> Option<Theme> {
        match name {
            "ono" => Some(Theme::default()),
            "neon" => Some(neon()),
            _ => None,
        }
    }

    /// Reads a theme file — the `~/.config/ono/themes/*.toml` of spec §30.
    ///
    /// A file names the built-in theme it starts from and overrides the tokens it cares about;
    /// every token it does not name keeps the base's style, because a theme that silently
    /// unstyled twenty-three tokens to restyle one would be a theme nobody could write.
    ///
    /// ```
    /// use ono_render::{Theme, Token};
    /// let theme = Theme::parse("mine", "[tokens]\n\"ui.danger\" = { color = 9 }\n")
    ///     .expect("a valid theme file");
    /// assert_eq!(theme.style(Token::Danger).colour(), ono_render::Color::Indexed(9));
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a structured error when the file is not valid TOML, names a token spec §44 does
    /// not define, uses a style key nothing implements, carries a marker that could drive the
    /// terminal, or would leave a token's meaning readable by colour alone (spec §44).
    pub fn parse(name: &str, text: &str) -> Result<Theme, ErrorValue> {
        let document: toml::Table = text.parse().map_err(|error| {
            mismatch(format!("`{name}` is not a valid theme file: {error}")).with_help(
                "a theme file is TOML: an optional `extends`, then a `[tokens]` table keyed by \
                 the token names of spec §44",
            )
        })?;

        let base = match document.get("extends") {
            None => "ono",
            Some(toml::Value::String(base)) => base.as_str(),
            Some(_) => {
                return Err(mismatch(format!(
                    "`{name}` declares an `extends` that is not a theme name"
                )));
            }
        };
        let mut theme = Theme::named(base).ok_or_else(|| {
            unknown(format!(
                "`{name}` extends `{base}`, which is not a theme this build ships"
            ))
            .with_help(format!("the themes it ships are {}", BUILT_IN.join(", ")))
        })?;
        theme.name = name.to_owned();

        for key in document.keys() {
            if key != "extends" && key != "tokens" {
                return Err(unknown(format!(
                    "`{name}` declares `{key}`, which a theme file has no such section for"
                ))
                .with_help("a theme file has an `extends` and a `[tokens]` table"));
            }
        }

        let tokens = match document.get("tokens") {
            None => return Ok(theme),
            Some(toml::Value::Table(tokens)) => tokens,
            Some(_) => {
                return Err(mismatch(format!(
                    "`{name}` declares a `tokens` that is not a table"
                )));
            }
        };

        for (token_name, declared) in tokens {
            let token = Token::from_name(token_name).ok_or_else(|| {
                unknown(format!(
                    "`{name}` styles `{token_name}`, which is not a semantic token of spec §44"
                ))
                .with_help(format!(
                    "the tokens are {}",
                    Token::ALL
                        .iter()
                        .map(|token| token.name())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
            let style = parse_style(name, token, theme.style(token), declared)?;
            theme.set(token, style);
        }

        theme.check_readable_without_colour(name)?;
        Ok(theme)
    }

    /// Spec §44: "No functionality may depend on color alone."
    ///
    /// With colour disabled every theme paints the same bytes, so a theme cannot make output
    /// unreadable there — [`Theme::paint`] never consults it. What a theme *can* do is take away
    /// the mark that tells two opposite meanings apart when the colour is gone, and that is what
    /// this refuses. The pairs are the ones whose confusion costs something: a destructive
    /// outcome that reads as a successful one, and a warning that reads as either.
    fn check_readable_without_colour(&self, name: &str) -> Result<(), ErrorValue> {
        for (token, other) in [
            (Token::Danger, Token::Success),
            (Token::Warning, Token::Success),
            (Token::Danger, Token::Warning),
        ] {
            let mark = self.style(token);
            let against = self.style(other);
            if mark.marker().is_none() || mark.marker() == against.marker() {
                return Err(mismatch(format!(
                    "`{name}` leaves `{}` indistinguishable from `{}` once colour is gone",
                    token.name(),
                    other.name()
                ))
                .with_help(
                    "give the token a `marker` of its own: spec §44 requires that no \
                     functionality depend on colour alone, and a pipe, a dumb terminal and a \
                     reader who cannot see the hue all get the marker instead",
                ));
            }
        }
        Ok(())
    }
}

/// One token's style, as a theme file declares it, over the style it is replacing.
fn parse_style(
    theme: &str,
    token: Token,
    base: Style,
    declared: &toml::Value,
) -> Result<Style, ErrorValue> {
    let Some(table) = declared.as_table() else {
        return Err(mismatch(format!(
            "`{theme}` styles `{}` with something that is not a table of style keys",
            token.name()
        ))
        .with_help("a style is `{ color = 203, bold = true, marker = \"!!\" }`"));
    };

    let mut style = base;
    for (key, value) in table {
        match (key.as_str(), value) {
            ("color", toml::Value::Integer(index)) => {
                let index = u8::try_from(*index).map_err(|_| {
                    mismatch(format!(
                        "`{theme}` paints `{}` with colour {index}, and the palette every \
                         terminal understands runs from 0 to 255",
                        token.name()
                    ))
                })?;
                style = style.with_color(Color::Indexed(index));
            }
            ("color", toml::Value::String(word)) if word == "default" => {
                style = style.with_color(Color::Default);
            }
            ("bold", toml::Value::Boolean(on)) => style = style.with_bold(*on),
            ("dim", toml::Value::Boolean(on)) => style = style.with_dim(*on),
            ("underline", toml::Value::Boolean(on)) => style = style.with_underline(*on),
            ("marker", toml::Value::String(mark)) => {
                check_marker(theme, token, mark)?;
                style = style.with_marker(mark.as_str());
            }
            ("color" | "bold" | "dim" | "underline" | "marker", _) => {
                return Err(mismatch(format!(
                    "`{theme}` gives `{}` a `{key}` of the wrong kind",
                    token.name()
                ))
                .with_help(
                    "`color` is 0-255 or \"default\"; `bold`, `dim` and `underline` are true or \
                     false; `marker` is a short piece of text",
                ));
            }
            _ => {
                return Err(unknown(format!(
                    "`{theme}` gives `{}` a `{key}`, which is not a style key this shell \
                     implements",
                    token.name()
                ))
                .with_help("the style keys are color, bold, dim, underline, marker"));
            }
        }
    }
    Ok(style)
}

/// A marker is printed verbatim beside a value, so it is held to what a value is held to.
fn check_marker(theme: &str, token: Token, mark: &str) -> Result<(), ErrorValue> {
    if mark.chars().any(char::is_control) {
        return Err(mismatch(format!(
            "`{theme}` gives `{}` a marker containing a control character",
            token.name()
        ))
        .with_help(
            "a marker is printed beside the value it marks; a value may never drive the \
             terminal, and neither may a theme (spec §49)",
        ));
    }
    if mark.chars().count() > MARKER_LIMIT {
        return Err(mismatch(format!(
            "`{theme}` gives `{}` a marker of {} characters, and a marker may be at most \
             {MARKER_LIMIT}",
            token.name(),
            mark.chars().count()
        ))
        .with_help(
            "a marker sits inside a table cell beside its value: it is a mark, not a note",
        ));
    }
    Ok(())
}

fn mismatch(message: String) -> ErrorValue {
    ErrorValue::new(ErrorCode::TypeMismatch, message)
}

fn unknown(message: String) -> ErrorValue {
    ErrorValue::new(ErrorCode::TypeUnknownField, message)
}

/// The cyberpunk theme of spec §44: the same semantics, the accent colours used harder.
///
/// §44 allows it to "use accent colors more aggressively" and requires that "semantic contrast
/// and accessibility remain requirements", so every marker of the default theme survives here
/// unchanged; only the palette is louder.
fn neon() -> Theme {
    let mut theme = Theme {
        name: "neon".to_owned(),
        ..Theme::default()
    };
    for (token, style) in [
        (Token::Accent, Style::color(Color::Indexed(51)).bold()),
        (Token::Success, Style::color(Color::Indexed(46)).bold()),
        (Token::Warning, Style::color(Color::Indexed(226)).bold()),
        (Token::Danger, Style::color(Color::Indexed(197)).bold()),
        (
            Token::Selection,
            Style::color(Color::Indexed(51)).bold().underline(),
        ),
        (Token::PromptLink, Style::color(Color::Indexed(51)).bold()),
        (Token::PromptContext, Style::color(Color::Indexed(213))),
        (Token::PromptRoot, Style::color(Color::Indexed(197)).bold()),
        (Token::TableHeader, Style::color(Color::Indexed(51)).bold()),
        (Token::ValueNumber, Style::color(Color::Indexed(213))),
        (Token::ErrorCode, Style::color(Color::Indexed(197)).bold()),
        (Token::GraphEdge, Style::color(Color::Indexed(51))),
        (Token::Path, Style::color(Color::Indexed(87))),
    ] {
        // The marker is the part a reader without colour depends on, so it is carried over from
        // the default theme rather than restated: a louder palette may not cost anyone the mark.
        let marker = theme.style(token).marker().map(str::to_owned);
        theme.set(
            token,
            match marker {
                Some(mark) => style.with_marker(mark),
                None => style,
            },
        );
    }
    theme
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
