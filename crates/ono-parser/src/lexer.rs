//! The mode-sensitive lexer of ADR-0009.
//!
//! There is no single token stream: a stage's head decides whether its arguments are read as
//! words (where `<` and `>` redirect and a bare run of characters is one atom) or as an
//! expression (where `<` and `>` compare and a bare run is an identifier). The parser therefore
//! asks for the next token at a byte offset *in a mode*, and never buffers a stream it would
//! have to re-lex when the mode changes.

use ono_core::Span;

/// The lexical class of a token.
///
/// The same source character can produce different kinds in different modes: `>` is
/// [`TokenKind::Gt`] both as a redirection and as a comparison, and it is the stage's
/// [`crate::ArgMode`] that says which one the parser is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenKind {
    /// A words-mode atom: the maximal run of non-whitespace, non-structural characters.
    Word,
    /// An expression-mode identifier, `[A-Za-z_][A-Za-z0-9_-]*`.
    Ident,
    /// An integer literal, in decimal, hexadecimal or binary.
    Int,
    /// A literal with a fractional part or an exponent.
    Float,
    /// A numeric literal with an adjacent unit suffix, such as `512MiB`.
    Unit,
    /// A double-quoted string, which may interpolate.
    Str,
    /// A single-quoted string, which never interpolates.
    RawStr,
    /// A double-quoted string whose closing quote is missing.
    UnterminatedStr,
    /// A single-quoted string whose closing quote is missing.
    UnterminatedRawStr,
    /// A regex literal, `/pattern/flags`.
    Regex,
    /// A regex literal whose closing delimiter is missing.
    UnterminatedRegex,
    /// A variable reference, `$name`; in words mode it includes any `.field` steps.
    Variable,
    /// A current-value reference, `@`, `@-1` or `@3`.
    CurrentValue,
    /// `|`.
    Pipe,
    /// `&&`.
    AndAnd,
    /// `||`.
    OrOr,
    /// `&`.
    Amp,
    /// `;`.
    Semi,
    /// A line break, which terminates a statement.
    Newline,
    /// `,`.
    Comma,
    /// `:`.
    Colon,
    /// `(`.
    LParen,
    /// `)`.
    RParen,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `{`.
    LBrace,
    /// `}`.
    RBrace,
    /// `>`, optionally prefixed by a file descriptor in words mode.
    Gt,
    /// `>>`, optionally prefixed by a file descriptor.
    GtGt,
    /// `<`, optionally prefixed by a file descriptor.
    Lt,
    /// `>&`, optionally prefixed by a file descriptor.
    GtAmp,
    /// `<&`, optionally prefixed by a file descriptor.
    LtAmp,
    /// `==`.
    EqEq,
    /// `!=`.
    BangEq,
    /// `<=`.
    LtEq,
    /// `>=`.
    GtEq,
    /// `~=`.
    Match,
    /// `!~=`.
    NotMatch,
    /// `+`.
    Plus,
    /// `-`.
    Minus,
    /// `*`.
    Star,
    /// `/`.
    Slash,
    /// `%` used as the remainder operator.
    Percent,
    /// `=`.
    Eq,
    /// `=>`.
    FatArrow,
    /// `->`.
    ThinArrow,
    /// `.`.
    Dot,
    /// `?.`.
    QuestionDot,
    /// `?`.
    Question,
    /// `#` to the end of the line.
    Comment,
    /// The end of the input.
    Eof,
    /// A character that begins no token in this mode.
    Unknown,
}

impl TokenKind {
    /// Whether the token is a string literal whose closing quote is missing.
    #[must_use]
    pub const fn is_unterminated(self) -> bool {
        matches!(
            self,
            TokenKind::UnterminatedStr
                | TokenKind::UnterminatedRawStr
                | TokenKind::UnterminatedRegex
        )
    }

    /// Whether the token closes a bracketed construct, and so can never be completed by typing.
    #[must_use]
    pub const fn is_closing_delimiter(self) -> bool {
        matches!(
            self,
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace
        )
    }
}

/// One lexical token: what it is and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    /// The lexical class.
    pub kind: TokenKind,
    /// The source range the token covers.
    pub span: Span,
}

impl Token {
    /// The exact source text of the token.
    #[must_use]
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        self.span.of(source)
    }
}

/// Which lexical rules apply at the offset the parser is asking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LexMode {
    /// Words mode: `<` `>` redirect, bare runs are word atoms.
    Words,
    /// Expression mode at an infix position: `/` divides.
    Expr,
    /// Expression mode at an operand position: `/` begins a regex literal.
    ExprOperand,
}

impl LexMode {
    const fn is_words(self) -> bool {
        matches!(self, LexMode::Words)
    }
}

/// The unit suffixes, longest first so that `ms` wins over `m` and `MiB` over `B`.
pub(crate) const UNIT_SUFFIXES: &[(&str, crate::ast::Unit)] = &[
    ("KiB", crate::ast::Unit::KiB),
    ("MiB", crate::ast::Unit::MiB),
    ("GiB", crate::ast::Unit::GiB),
    ("TiB", crate::ast::Unit::TiB),
    ("PiB", crate::ast::Unit::PiB),
    ("KB", crate::ast::Unit::KB),
    ("MB", crate::ast::Unit::MB),
    ("GB", crate::ast::Unit::GB),
    ("TB", crate::ast::Unit::TB),
    ("PB", crate::ast::Unit::PB),
    ("ns", crate::ast::Unit::Ns),
    ("us", crate::ast::Unit::Us),
    ("ms", crate::ast::Unit::Ms),
    ("B", crate::ast::Unit::B),
    ("s", crate::ast::Unit::S),
    ("m", crate::ast::Unit::M),
    ("h", crate::ast::Unit::H),
    ("d", crate::ast::Unit::D),
    ("w", crate::ast::Unit::W),
    ("%", crate::ast::Unit::Percent),
];

/// Characters that always end a words-mode atom (ADR-0009 "Lexical rules").
const WORD_STOPPERS: &[u8] = b"|()[]{};,'\"<>";

pub(crate) const fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

pub(crate) const fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

/// Reads the unit suffix at `at`, if one is there and nothing identifier-like follows it.
pub(crate) fn unit_suffix_at(source: &str, at: usize) -> Option<(crate::ast::Unit, usize)> {
    let rest = source.get(at..)?;
    for (text, unit) in UNIT_SUFFIXES {
        if rest.starts_with(text) {
            let end = at + text.len();
            let follower = source.as_bytes().get(end).copied();
            let continues =
                follower.is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_');
            if !continues {
                return Some((*unit, end));
            }
        }
    }
    None
}

/// Reads the token that starts at or after `at`, under the rules of `mode`.
///
/// Whitespace other than a line break is skipped; the returned span therefore starts at the
/// first significant byte. At the end of the input an [`TokenKind::Eof`] token of zero width is
/// returned, so callers always have a span to point a diagnostic at.
pub(crate) fn next_token(source: &str, at: u32, mode: LexMode) -> Token {
    let bytes = source.as_bytes();
    let mut index = (at as usize).min(bytes.len());
    while matches!(bytes.get(index), Some(b' ' | b'\t' | b'\r')) {
        index += 1;
    }
    let start = index;
    let Some(&first) = bytes.get(index) else {
        return token(TokenKind::Eof, start, start);
    };

    match first {
        b'\n' => token(TokenKind::Newline, start, start + 1),
        b'#' => {
            let end = bytes[index..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |offset| index + offset);
            token(TokenKind::Comment, start, end)
        }
        b'|' if bytes.get(index + 1) == Some(&b'|') => token(TokenKind::OrOr, start, start + 2),
        b'|' => token(TokenKind::Pipe, start, start + 1),
        b'&' if bytes.get(index + 1) == Some(&b'&') => token(TokenKind::AndAnd, start, start + 2),
        b'&' => token(TokenKind::Amp, start, start + 1),
        b';' => token(TokenKind::Semi, start, start + 1),
        b',' => token(TokenKind::Comma, start, start + 1),
        b'(' => token(TokenKind::LParen, start, start + 1),
        b')' => token(TokenKind::RParen, start, start + 1),
        b'[' => token(TokenKind::LBracket, start, start + 1),
        b']' => token(TokenKind::RBracket, start, start + 1),
        b'{' => token(TokenKind::LBrace, start, start + 1),
        b'}' => token(TokenKind::RBrace, start, start + 1),
        b'"' => quoted(bytes, start, b'"', true),
        b'\'' => quoted(bytes, start, b'\'', false),
        b'$' if bytes.get(index + 1).copied().is_some_and(is_ident_start) => {
            index += 1;
            while bytes.get(index).copied().is_some_and(is_ident_continue) {
                index += 1;
            }
            if mode.is_words() {
                index = take_dotted_steps(bytes, index);
            }
            token(TokenKind::Variable, start, index)
        }
        b'@' if !mode.is_words() || at_current_value(bytes, index) => {
            index += 1;
            if bytes.get(index) == Some(&b'-')
                && bytes.get(index + 1).is_some_and(u8::is_ascii_digit)
            {
                index += 1;
            }
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index += 1;
            }
            if mode.is_words() {
                index = take_dotted_steps(bytes, index);
            }
            token(TokenKind::CurrentValue, start, index)
        }
        _ if mode.is_words() => words_token(bytes, start),
        _ => expression_token(source, bytes, start, mode),
    }
}

/// Whether `@` at `index` begins a current-value reference rather than an ordinary word.
fn at_current_value(bytes: &[u8], index: usize) -> bool {
    match bytes.get(index + 1) {
        None => true,
        Some(byte) => {
            byte.is_ascii_digit()
                || byte.is_ascii_whitespace()
                || WORD_STOPPERS.contains(byte)
                || *byte == b'-' && bytes.get(index + 2).is_some_and(u8::is_ascii_digit)
                || *byte == b'.' && bytes.get(index + 2).copied().is_some_and(is_ident_start)
        }
    }
}

/// Consumes `.field` steps, which words mode folds into the variable or current-value token.
fn take_dotted_steps(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index) == Some(&b'.')
        && bytes.get(index + 1).copied().is_some_and(is_ident_start)
    {
        index += 1;
        while bytes.get(index).copied().is_some_and(is_ident_continue) {
            index += 1;
        }
    }
    index
}

fn words_token(bytes: &[u8], start: usize) -> Token {
    // A run of digits directly before a redirection operator is its file descriptor.
    let mut index = start;
    while bytes.get(index).is_some_and(u8::is_ascii_digit) {
        index += 1;
    }
    if index > start && matches!(bytes.get(index), Some(b'>' | b'<')) {
        return redirect_token(bytes, start, index);
    }
    if matches!(bytes.get(start), Some(b'>' | b'<')) {
        return redirect_token(bytes, start, start);
    }

    let mut index = start;
    while let Some(&byte) = bytes.get(index) {
        // A backslash carries the next character into the word, whitespace included, so
        // `cd My\ Documents` is one argument (ADR-0019). The escape is kept in the token text;
        // removing it is the evaluator's job, because an external command must be able to
        // receive what was typed.
        if byte == b'\\' {
            match bytes.get(index + 1) {
                Some(_) => index += 2,
                // A line ending in a lone backslash is being typed, not broken.
                None => index += 1,
            }
            continue;
        }
        if byte.is_ascii_whitespace() || WORD_STOPPERS.contains(&byte) {
            break;
        }
        index += 1;
    }
    token(TokenKind::Word, start, index.max(start + 1))
}

/// Reads a redirection operator whose descriptor prefix, if any, runs from `start` to `operator`.
fn redirect_token(bytes: &[u8], start: usize, operator: usize) -> Token {
    let kind_and_len = match (bytes.get(operator), bytes.get(operator + 1)) {
        (Some(b'>'), Some(b'>')) => (TokenKind::GtGt, 2),
        (Some(b'>'), Some(b'&')) => (TokenKind::GtAmp, 2),
        (Some(b'>'), _) => (TokenKind::Gt, 1),
        (Some(b'<'), Some(b'&')) => (TokenKind::LtAmp, 2),
        _ => (TokenKind::Lt, 1),
    };
    token(kind_and_len.0, start, operator + kind_and_len.1)
}

fn quoted(bytes: &[u8], start: usize, quote: u8, escapes: bool) -> Token {
    let mut index = start + 1;
    while let Some(&byte) = bytes.get(index) {
        if escapes && byte == b'\\' {
            index += 2;
            continue;
        }
        if byte == quote {
            let kind = if escapes {
                TokenKind::Str
            } else {
                TokenKind::RawStr
            };
            return token(kind, start, index + 1);
        }
        index += 1;
    }
    let kind = if escapes {
        TokenKind::UnterminatedStr
    } else {
        TokenKind::UnterminatedRawStr
    };
    token(kind, start, bytes.len())
}

fn expression_token(source: &str, bytes: &[u8], start: usize, mode: LexMode) -> Token {
    let first = bytes[start];
    if first.is_ascii_digit() {
        return number_token(source, bytes, start);
    }
    if is_ident_start(first) {
        let mut index = start + 1;
        while bytes.get(index).copied().is_some_and(is_ident_continue) {
            index += 1;
        }
        return token(TokenKind::Ident, start, index);
    }
    if first == b'/' && mode == LexMode::ExprOperand {
        return regex_token(bytes, start);
    }

    let rest = &source[start..];
    for (text, kind) in [
        ("!~=", TokenKind::NotMatch),
        ("?.", TokenKind::QuestionDot),
        ("==", TokenKind::EqEq),
        ("!=", TokenKind::BangEq),
        ("~=", TokenKind::Match),
        ("<=", TokenKind::LtEq),
        (">=", TokenKind::GtEq),
        ("=>", TokenKind::FatArrow),
        ("->", TokenKind::ThinArrow),
    ] {
        if rest.starts_with(text) {
            return token(kind, start, start + text.len());
        }
    }
    let single = match first {
        b'<' => TokenKind::Lt,
        b'>' => TokenKind::Gt,
        b'=' => TokenKind::Eq,
        b'+' => TokenKind::Plus,
        b'-' => TokenKind::Minus,
        b'*' => TokenKind::Star,
        b'/' => TokenKind::Slash,
        b'%' => TokenKind::Percent,
        b'.' => TokenKind::Dot,
        b'?' => TokenKind::Question,
        b':' => TokenKind::Colon,
        _ => TokenKind::Unknown,
    };
    if single == TokenKind::Unknown {
        let width = source[start..].chars().next().map_or(1, char::len_utf8);
        return token(TokenKind::Unknown, start, start + width);
    }
    token(single, start, start + 1)
}

fn number_token(source: &str, bytes: &[u8], start: usize) -> Token {
    let mut index = start;
    let mut kind = TokenKind::Int;
    let radix_prefix = matches!(
        (bytes.get(start), bytes.get(start + 1), bytes.get(start + 2)),
        (Some(b'0'), Some(b'x' | b'X' | b'b' | b'B'), Some(byte)) if byte.is_ascii_hexdigit()
    );
    if radix_prefix {
        let hex = matches!(bytes[start + 1], b'x' | b'X');
        index = start + 2;
        while bytes
            .get(index)
            .is_some_and(|byte| *byte == b'_' || digit_of(*byte, hex))
        {
            index += 1;
        }
    } else {
        while bytes
            .get(index)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
        {
            index += 1;
        }
        if bytes.get(index) == Some(&b'.') && bytes.get(index + 1).is_some_and(u8::is_ascii_digit) {
            kind = TokenKind::Float;
            index += 1;
            while bytes
                .get(index)
                .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'_')
            {
                index += 1;
            }
        }
        if matches!(bytes.get(index), Some(b'e' | b'E')) {
            let after_sign = if matches!(bytes.get(index + 1), Some(b'+' | b'-')) {
                index + 2
            } else {
                index + 1
            };
            if bytes.get(after_sign).is_some_and(u8::is_ascii_digit) {
                kind = TokenKind::Float;
                index = after_sign;
                while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    index += 1;
                }
            }
        }
    }
    if let Some((_, end)) = unit_suffix_at(source, index) {
        return token(TokenKind::Unit, start, end);
    }
    token(kind, start, index)
}

const fn digit_of(byte: u8, hex: bool) -> bool {
    if hex {
        byte.is_ascii_hexdigit()
    } else {
        matches!(byte, b'0' | b'1')
    }
}

fn regex_token(bytes: &[u8], start: usize) -> Token {
    let mut index = start + 1;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'\\' => index += 2,
            b'\n' => return token(TokenKind::UnterminatedRegex, start, index),
            b'/' => {
                index += 1;
                while matches!(bytes.get(index), Some(b'i' | b'm' | b's' | b'x')) {
                    index += 1;
                }
                return token(TokenKind::Regex, start, index);
            }
            _ => index += 1,
        }
    }
    token(TokenKind::UnterminatedRegex, start, bytes.len())
}

fn token(kind: TokenKind, start: usize, end: usize) -> Token {
    Token {
        kind,
        span: Span::new(start as u32, end as u32),
    }
}
