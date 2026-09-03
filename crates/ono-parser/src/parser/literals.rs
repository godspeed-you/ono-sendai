//! Literals: numbers and units, lists, records, and the strings that interpolate.
//!
//! These are the leaves of the expression grammar, plus the text decoding they need — an escape
//! sequence, a unit suffix, a `@-1` selector.

use ono_core::Span;

use crate::ast::{
    CurrentSelector, Expr, FieldAccess, ListExpr, NumberValue, RecordExpr, RecordField, RecordKey,
    StrLit, StrPart, Variable,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token, TokenKind, is_ident_continue, is_ident_start};

use super::state::Parser;

impl Parser<'_> {
    pub(super) fn number_value(&mut self, text: &str, span: Span) -> NumberValue {
        let cleaned: String = text.chars().filter(|character| *character != '_').collect();
        let radix = if let Some(rest) = cleaned
            .strip_prefix("0x")
            .or_else(|| cleaned.strip_prefix("0X"))
        {
            Some((rest.to_owned(), 16))
        } else {
            cleaned
                .strip_prefix("0b")
                .or_else(|| cleaned.strip_prefix("0B"))
                .map(|rest| (rest.to_owned(), 2))
        };
        if let Some((digits, base)) = radix {
            return match i64::from_str_radix(&digits, base) {
                Ok(value) => NumberValue::Int(value),
                Err(_) => {
                    self.report(Diagnostic::syntax(
                        span,
                        "this numeric literal is too large to represent",
                    ));
                    NumberValue::Int(0)
                }
            };
        }
        if let Ok(value) = cleaned.parse::<i64>() {
            return NumberValue::Int(value);
        }
        NumberValue::Float(cleaned.parse::<f64>().unwrap_or(0.0))
    }

    pub(super) fn parse_list(&mut self) -> Expr {
        let open = self.bump(LexMode::ExprOperand);
        let saved = self.no_brace;
        self.no_brace = false;
        let mut items = Vec::new();
        let end;
        loop {
            self.skip_newlines(LexMode::ExprOperand);
            let token = self.peek(LexMode::ExprOperand);
            match token.kind {
                TokenKind::RBracket => {
                    end = self.bump(LexMode::ExprOperand).span;
                    break;
                }
                TokenKind::Eof => {
                    self.report(Diagnostic::incomplete(
                        open.span,
                        "this `[` is never closed",
                    ));
                    end = token.span;
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            items.push(self.parse_expression());
            self.skip_newlines(LexMode::Expr);
            if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                && !matches!(
                    self.peek(LexMode::Expr).kind,
                    TokenKind::RBracket | TokenKind::Eof
                )
            {
                let token = self.peek(LexMode::Expr);
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected `,` or `]` in the list, found {description}"),
                );
                self.bump(LexMode::Expr);
            }
            if self.pos == before {
                self.bump(LexMode::Expr);
            }
        }
        self.no_brace = saved;
        Expr::List(ListExpr {
            items,
            span: open.span.join(end),
        })
    }

    /// Whether the `{` at `open` starts a record rather than a block (ADR-0009).
    pub(super) fn is_record_brace(&self, open: Token) -> bool {
        let first = self.peek_after(LexMode::ExprOperand, open);
        match first.kind {
            TokenKind::RBrace => true,
            TokenKind::Ident | TokenKind::Str | TokenKind::RawStr => {
                self.peek_after(LexMode::Expr, first).kind == TokenKind::Colon
            }
            _ => false,
        }
    }

    pub(super) fn parse_brace(&mut self, open: Token) -> Expr {
        if self.is_record_brace(open) {
            self.parse_record(open)
        } else {
            Expr::Block(self.parse_block())
        }
    }

    pub(super) fn parse_record(&mut self, open: Token) -> Expr {
        self.bump(LexMode::ExprOperand);
        let saved = self.no_brace;
        self.no_brace = false;
        let mut fields = Vec::new();
        let end;
        loop {
            self.skip_newlines(LexMode::ExprOperand);
            let token = self.peek(LexMode::ExprOperand);
            match token.kind {
                TokenKind::RBrace => {
                    end = self.bump(LexMode::ExprOperand).span;
                    break;
                }
                TokenKind::Eof => {
                    self.report(Diagnostic::incomplete(
                        open.span,
                        "this `{` is never closed",
                    ));
                    end = token.span;
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            fields.push(self.parse_record_field());
            self.skip_newlines(LexMode::Expr);
            if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                && !matches!(
                    self.peek(LexMode::Expr).kind,
                    TokenKind::RBrace | TokenKind::Eof
                )
            {
                let token = self.peek(LexMode::Expr);
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected `,` or `}}` in the record, found {description}"),
                );
                self.bump(LexMode::Expr);
            }
            if self.pos == before {
                self.bump(LexMode::Expr);
            }
        }
        self.no_brace = saved;
        Expr::Record(RecordExpr {
            fields,
            span: open.span.join(end),
        })
    }

    pub(super) fn parse_record_field(&mut self) -> RecordField {
        let token = self.peek(LexMode::ExprOperand);
        let key = match token.kind {
            TokenKind::Ident => {
                self.bump(LexMode::ExprOperand);
                RecordKey::Ident {
                    name: token.text(self.source).to_owned(),
                    span: token.span,
                }
            }
            TokenKind::Str | TokenKind::RawStr => match self.parse_string(token) {
                Expr::Str(literal) => RecordKey::Str(literal),
                other => RecordKey::Ident {
                    name: String::new(),
                    span: other.span(),
                },
            },
            _ => {
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected a record field name, found {description}"),
                );
                RecordKey::Ident {
                    name: String::new(),
                    span: Span::at(token.span.start()),
                }
            }
        };
        let value = if self
            .expect(LexMode::Expr, TokenKind::Colon, "`:`")
            .is_some()
        {
            self.parse_expression()
        } else {
            Expr::Error(Span::at(key.span().end()))
        };
        RecordField {
            span: key.span().join(value.span()),
            key,
            value,
        }
    }

    /// Reads a string literal and decodes it into literal text and interpolated expressions.
    pub(super) fn parse_string(&mut self, token: Token) -> Expr {
        self.bump(LexMode::ExprOperand);
        let raw = matches!(
            token.kind,
            TokenKind::RawStr | TokenKind::UnterminatedRawStr
        );
        let terminated = !token.kind.is_unterminated();
        if !terminated {
            self.report(Diagnostic::incomplete(
                token.span,
                "this string is never closed",
            ));
        }
        let start = token.span.start().saturating_add(1).min(token.span.end());
        let end = if terminated {
            token.span.end().saturating_sub(1).max(start)
        } else {
            token.span.end()
        };
        let parts = if raw {
            let text = Span::new(start, end).of(self.source);
            if text.is_empty() {
                Vec::new()
            } else {
                vec![StrPart::Text {
                    text: text.to_owned(),
                    span: Span::new(start, end),
                }]
            }
        } else {
            self.decode_interpolated(start, end)
        };
        Expr::Str(StrLit {
            parts,
            raw,
            span: token.span,
        })
    }

    pub(super) fn decode_interpolated(&mut self, start: u32, end: u32) -> Vec<StrPart> {
        let source = self.source;
        let bytes = source.as_bytes();
        let saved_pos = self.pos;
        let end = end as usize;
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut text_start = start;
        let mut index = start as usize;
        while index < end {
            let byte = bytes.get(index).copied().unwrap_or(b'\0');
            if byte == b'\\' {
                let (decoded, next) = self.decode_escape(index, end);
                if let Some(character) = decoded {
                    text.push(character);
                }
                index = next.max(index + 1);
                continue;
            }
            if byte == b'$' {
                let following = bytes.get(index + 1).copied();
                // `$?` is the last exit status, which is not an identifier (ADR-0019).
                if following == Some(b'?') {
                    flush(&mut parts, &mut text, text_start, index as u32);
                    parts.push(StrPart::Expr(Expr::Variable(Variable {
                        name: "?".to_owned(),
                        span: Span::new(index as u32, index as u32 + 2),
                    })));
                    index += 2;
                    text_start = index as u32;
                    continue;
                }
                if following.is_some_and(is_ident_start) {
                    flush(&mut parts, &mut text, text_start, index as u32);
                    index = self.interpolated_variable(&mut parts, index, end);
                    text_start = index as u32;
                    continue;
                }
                if following == Some(b'(') {
                    flush(&mut parts, &mut text, text_start, index as u32);
                    index = self.interpolated_pipeline(&mut parts, index, end);
                    text_start = index as u32;
                    continue;
                }
            }
            let width = source
                .get(index..end)
                .and_then(|rest| rest.chars().next())
                .map_or(1, char::len_utf8);
            if let Some(slice) = source.get(index..index + width) {
                text.push_str(slice);
            }
            index += width;
        }
        flush(&mut parts, &mut text, text_start, end as u32);
        self.pos = saved_pos;
        parts
    }

    /// Reads `$name` and any `.field` steps that follow it inside a string.
    pub(super) fn interpolated_variable(
        &mut self,
        parts: &mut Vec<StrPart>,
        start: usize,
        end: usize,
    ) -> usize {
        let source = self.source;
        let bytes = source.as_bytes();
        let mut index = start + 1;
        while index < end && bytes.get(index).copied().is_some_and(is_ident_continue) {
            index += 1;
        }
        let name = source.get(start + 1..index).unwrap_or_default().to_owned();
        let mut expression = Expr::Variable(Variable {
            name,
            span: Span::new(start as u32, index as u32),
        });
        while index + 1 < end
            && bytes.get(index) == Some(&b'.')
            && bytes.get(index + 1).copied().is_some_and(is_ident_start)
        {
            let field_start = index + 1;
            let mut field_end = field_start;
            while field_end < end && bytes.get(field_end).copied().is_some_and(is_ident_continue) {
                field_end += 1;
            }
            let field_span = Span::new(field_start as u32, field_end as u32);
            expression = Expr::Field(Box::new(FieldAccess {
                span: Span::new(start as u32, field_end as u32),
                base: expression,
                field: source
                    .get(field_start..field_end)
                    .unwrap_or_default()
                    .to_owned(),
                field_span,
                optional: false,
            }));
            index = field_end;
        }
        parts.push(StrPart::Expr(expression));
        index
    }

    /// Reads `$( pipeline )` inside a string by parsing the region between the parentheses.
    pub(super) fn interpolated_pipeline(
        &mut self,
        parts: &mut Vec<StrPart>,
        start: usize,
        end: usize,
    ) -> usize {
        let saved_pos = self.pos;
        let saved_limit = self.limit;
        let saved_record = self.record.take();
        self.pos = (start + 1) as u32;
        self.limit = end;
        let paren = self.parse_paren_value();
        let consumed = self.pos as usize;
        self.limit = saved_limit;
        self.record = saved_record;
        self.pos = saved_pos;
        parts.push(StrPart::Expr(Expr::Paren(Box::new(paren))));
        consumed.max(start + 2).min(end)
    }

    /// Decodes the escape that starts at `index`, returning the character and the next offset.
    pub(super) fn decode_escape(&mut self, index: usize, end: usize) -> (Option<char>, usize) {
        let source = self.source;
        let bytes = source.as_bytes();
        if index + 1 >= end {
            // A backslash at the very end of an unfinished string is not yet an error.
            return (None, end);
        }
        let escape = bytes.get(index + 1).copied().unwrap_or(b'\0');
        let simple = match escape {
            b'\\' => Some('\\'),
            b'"' => Some('"'),
            b'\'' => Some('\''),
            b'n' => Some('\n'),
            b'r' => Some('\r'),
            b't' => Some('\t'),
            b'0' => Some('\0'),
            b'e' => Some('\u{1b}'),
            // Without `\$` there is no way at all to write a literal dollar sign inside an
            // interpolating string, and every user needs one the first time they write a
            // shell command that mentions `$$` or a price.
            b'$' => Some('$'),
            _ => None,
        };
        if let Some(character) = simple {
            return (Some(character), index + 2);
        }
        if escape == b'x' {
            let digits = source
                .get(index + 2..(index + 4).min(end))
                .unwrap_or_default();
            if digits.len() == 2
                && let Ok(value) = u8::from_str_radix(digits, 16)
            {
                return (Some(char::from(value)), index + 4);
            }
            self.report(Diagnostic::syntax(
                Span::new(index as u32, (index + 2).min(end) as u32),
                "`\\x` needs exactly two hexadecimal digits",
            ));
            return (None, index + 2);
        }
        if escape == b'u' {
            if bytes.get(index + 2) == Some(&b'{') {
                let mut cursor = index + 3;
                while cursor < end && bytes.get(cursor).is_some_and(u8::is_ascii_hexdigit) {
                    cursor += 1;
                }
                if bytes.get(cursor) == Some(&b'}') && cursor > index + 3 {
                    let digits = source.get(index + 3..cursor).unwrap_or_default();
                    let value = u32::from_str_radix(digits, 16).unwrap_or(u32::MAX);
                    if let Some(character) = char::from_u32(value) {
                        return (Some(character), cursor + 1);
                    }
                    self.report(Diagnostic::syntax(
                        Span::new(index as u32, (cursor + 1) as u32),
                        "this is not a Unicode scalar value",
                    ));
                    return (Some('\u{fffd}'), cursor + 1);
                }
                self.report(Diagnostic::syntax(
                    Span::new(index as u32, cursor.min(end) as u32),
                    "`\\u{…}` needs hexadecimal digits and a closing brace",
                ));
                return (None, cursor.min(end));
            }
            self.report(Diagnostic::syntax(
                Span::new(index as u32, (index + 2).min(end) as u32),
                "`\\u` must be written as `\\u{…}`",
            ));
            return (None, index + 2);
        }
        self.report(Diagnostic::syntax(
            Span::new(index as u32, (index + 2).min(end) as u32),
            "this escape sequence is not one Ono knows",
        ));
        let width = source
            .get(index + 1..end)
            .and_then(|rest| rest.chars().next())
            .map_or(1, char::len_utf8);
        let character = source
            .get(index + 1..index + 1 + width)
            .and_then(|slice| slice.chars().next());
        (character, index + 1 + width)
    }
}

pub(super) fn current_selector(text: &str) -> CurrentSelector {
    let digits = text.trim_start_matches('@');
    if let Some(previous) = digits.strip_prefix('-') {
        CurrentSelector::Previous(previous.parse().unwrap_or(1))
    } else if digits.is_empty() {
        CurrentSelector::Current
    } else {
        CurrentSelector::Item(digits.parse().unwrap_or(0))
    }
}
/// Splits `512MiB` into its numeric part and its unit.
pub(super) fn split_unit(text: &str) -> (&str, crate::ast::Unit) {
    for (suffix, unit) in crate::lexer::UNIT_SUFFIXES {
        if let Some(number) = text.strip_suffix(suffix) {
            return (number, *unit);
        }
    }
    (text, crate::ast::Unit::Percent)
}
pub(super) fn flush(parts: &mut Vec<StrPart>, text: &mut String, start: u32, end: u32) {
    if !text.is_empty() {
        parts.push(StrPart::Text {
            text: std::mem::take(text),
            span: Span::new(start, end),
        });
    }
}
