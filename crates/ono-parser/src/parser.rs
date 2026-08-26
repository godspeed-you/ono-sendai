//! The recoverable recursive-descent parser of ADR-0009.
//!
//! The parser never fails and never panics: it always returns a tree, collecting problems as
//! diagnostics instead of throwing them. Every construct has a recovery point — the end of a
//! stage at `|`, the end of a statement at a newline or `;`, the closing delimiter of a
//! bracketed form — so a half-typed line still produces something the editor can highlight.

use ono_core::Span;

use crate::ast::{
    ArgMode, Argument, BinaryExpr, BinaryOp, Block, CallExpr, CatchClause, ChainOp, ChainedList,
    CurrentSelector, CurrentValue, Expr, FieldAccess, FieldPath, FnDecl, ForStmt, IfBranch, IfStmt,
    IndexExpr, LetStmt, ListExpr, MatchArm, MatchArmBody, MatchStmt, NumberLit, NumberValue,
    OptionArg, Param, ParenInner, ParenValue, Pattern, Pipeline, Program, QualifiedName,
    RecordExpr, RecordField, RecordKey, RedirectOp, RedirectTarget, Redirection, RegexLit,
    ReturnStmt, Stage, StageHead, StageList, Statement, StrLit, StrPart, TryStmt, TypeRef,
    UnaryExpr, UnaryOp, UnitLit, UseStmt, Variable, WhileStmt, WordArg,
};
use crate::diagnostic::Diagnostic;
use crate::lexer::{LexMode, Token, TokenKind, is_ident_continue, is_ident_start, next_token};

/// How deeply constructs may nest before the parser stops descending.
///
/// Adversarial input is a given for a shell (spec §35.6); the limit keeps a line of ten thousand
/// open parentheses a diagnostic rather than a stack overflow.
const MAX_DEPTH: u32 = 96;

/// The result of parsing a source: a tree, plus everything that was wrong with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    program: Program,
    diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// The tree. Always present, even for input that was rejected outright.
    #[must_use]
    pub fn program(&self) -> &Program {
        &self.program
    }

    /// Everything the parser found wrong, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Whether the input ends outside every construct it opened.
    ///
    /// A line that is merely unfinished — an open quote, an open bracket, a trailing `|` — is
    /// not complete but is not wrong either, which is the distinction the editor needs
    /// (ADR-0009).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(crate::diagnostic::Diagnostic::is_incomplete)
    }

    /// Whether the input contains something no amount of further typing can rescue.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| !diagnostic.is_incomplete())
    }

    /// Takes the tree and the diagnostics apart, for a caller that wants to own both.
    #[must_use]
    pub fn into_parts(self) -> (Program, Vec<Diagnostic>) {
        (self.program, self.diagnostics)
    }
}

/// Parses a source into a tree and a list of diagnostics.
///
/// The call never panics and always returns a tree, whatever it is handed.
///
/// ```
/// let parsed = ono_parser::parse("get process | where cpu > 20");
/// assert!(parsed.diagnostics().is_empty());
/// assert_eq!(parsed.program().statements.len(), 1);
/// ```
#[must_use]
pub fn parse(source: &str) -> Parsed {
    let mut parser = Parser::new(source);
    let program = parser.parse_program();
    Parsed {
        program,
        diagnostics: parser.diagnostics,
    }
}

/// Lexes a source into the tokens the parser actually read, for syntax highlighting.
///
/// Because a token's class depends on the mode its stage is in, the stream is produced by the
/// parser rather than by a standalone lexer: `>` comes back as a redirection in `cat a > b` and
/// as a comparison in `where a > b`. Tokens are ordered, non-overlapping and inside the source.
///
/// ```
/// use ono_parser::TokenKind;
/// let kinds: Vec<TokenKind> = ono_parser::tokens("ls -la").iter().map(|t| t.kind).collect();
/// assert_eq!(kinds, vec![TokenKind::Word, TokenKind::Word]);
/// ```
#[must_use]
pub fn tokens(source: &str) -> Vec<Token> {
    let mut parser = Parser::new(source);
    parser.record = Some(Vec::new());
    let _ = parser.parse_program();
    parser.record.unwrap_or_default()
}

struct Parser<'a> {
    source: &'a str,
    limit: usize,
    pos: u32,
    diagnostics: Vec<Diagnostic>,
    record: Option<Vec<Token>>,
    depth: u32,
    no_brace: bool,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            limit: source.len(),
            pos: 0,
            diagnostics: Vec::new(),
            record: None,
            depth: 0,
            no_brace: false,
        }
    }

    fn text(&self) -> &'a str {
        self.source.get(..self.limit).unwrap_or(self.source)
    }

    /// The next significant token, without consuming it. Comments are consumed on sight.
    fn peek(&mut self, mode: LexMode) -> Token {
        loop {
            let token = next_token(self.text(), self.pos, mode);
            if token.kind == TokenKind::Comment {
                self.pos = token.span.end();
                if let Some(record) = &mut self.record {
                    record.push(token);
                }
                continue;
            }
            return token;
        }
    }

    /// The token after `token`, without consuming anything.
    fn peek_after(&self, mode: LexMode, token: Token) -> Token {
        let mut at = token.span.end();
        loop {
            let next = next_token(self.text(), at, mode);
            if next.kind == TokenKind::Comment {
                at = next.span.end();
                continue;
            }
            return next;
        }
    }

    fn bump(&mut self, mode: LexMode) -> Token {
        let token = self.peek(mode);
        if token.kind != TokenKind::Eof {
            self.pos = token.span.end();
            if let Some(record) = &mut self.record {
                record.push(token);
            }
        }
        token
    }

    fn eat(&mut self, mode: LexMode, kind: TokenKind) -> Option<Token> {
        let token = self.peek(mode);
        (token.kind == kind).then(|| self.bump(mode))
    }

    fn at_keyword(&mut self, mode: LexMode, keyword: &str) -> bool {
        let token = self.peek(mode);
        matches!(token.kind, TokenKind::Ident | TokenKind::Word)
            && token.text(self.source) == keyword
    }

    /// Whether more typing at the end of the input could still turn `token` into something valid.
    ///
    /// The end of the input is the one place where a token may be a fragment of a longer one, so
    /// a problem there is reported as unfinished rather than as wrong. A closing delimiter is
    /// the exception: nothing appended after it can rescue it.
    fn is_possibly_unfinished(&self, token: Token) -> bool {
        token.kind == TokenKind::Eof
            || (token.span.end() as usize == self.limit && !token.kind.is_closing_delimiter())
    }

    fn report(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Reports that `token` was not what the grammar expected here.
    fn report_unexpected(&mut self, token: Token, message: impl Into<String>) {
        let diagnostic = if self.is_possibly_unfinished(token) {
            Diagnostic::incomplete(token.span, message)
        } else {
            Diagnostic::syntax(token.span, message)
        };
        self.report(diagnostic);
    }

    fn describe(&self, token: Token) -> String {
        match token.kind {
            TokenKind::Eof => "the end of the input".to_owned(),
            TokenKind::Newline => "the end of the line".to_owned(),
            _ => format!("`{}`", token.text(self.source)),
        }
    }

    // --- program and statements ---------------------------------------------------------

    fn parse_program(&mut self) -> Program {
        let mut statements = Vec::new();
        loop {
            self.skip_separators();
            if self.peek(LexMode::Words).kind == TokenKind::Eof {
                break;
            }
            let before = self.pos;
            self.parse_statement_into(&mut statements);
            if self.pos == before {
                let token = self.bump(LexMode::Words);
                if token.kind == TokenKind::Eof {
                    break;
                }
            }
        }
        Program {
            statements,
            span: Span::new(0, self.limit as u32),
        }
    }

    fn skip_separators(&mut self) {
        while matches!(
            self.peek(LexMode::Words).kind,
            TokenKind::Newline | TokenKind::Semi
        ) {
            self.bump(LexMode::Words);
        }
    }

    /// Parses one statement and the terminator that follows it.
    fn parse_statement_into(&mut self, statements: &mut Vec<Statement>) {
        let (statement, block_terminated) = self.parse_statement();
        statements.push(statement);
        self.finish_statement(block_terminated);
    }

    fn finish_statement(&mut self, block_terminated: bool) {
        let token = self.peek(LexMode::Words);
        match token.kind {
            TokenKind::Semi | TokenKind::Newline => {
                self.bump(LexMode::Words);
            }
            TokenKind::Eof | TokenKind::RBrace => {}
            _ if block_terminated => {}
            _ => {
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected the end of the statement, found {description}"),
                );
                self.recover_to_statement_end();
            }
        }
    }

    fn recover_to_statement_end(&mut self) {
        loop {
            let token = self.peek(LexMode::Words);
            match token.kind {
                TokenKind::Eof | TokenKind::RBrace => return,
                TokenKind::Semi | TokenKind::Newline => {
                    self.bump(LexMode::Words);
                    return;
                }
                _ => {
                    self.bump(LexMode::Words);
                }
            }
        }
    }

    /// Parses one statement. The flag says whether it ended with a block, in which case no
    /// terminator is required before the next statement.
    fn parse_statement(&mut self) -> (Statement, bool) {
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::Word {
            match token.text(self.source) {
                "let" => return (Statement::Let(self.parse_let()), false),
                "fn" => return (Statement::Fn(self.parse_fn()), true),
                "if" => return (Statement::If(self.parse_if()), true),
                "for" => return (Statement::For(self.parse_for()), true),
                "while" => return (Statement::While(self.parse_while()), true),
                "match" => return (Statement::Match(self.parse_match()), true),
                "try" => return (Statement::Try(self.parse_try()), true),
                "return" => return (Statement::Return(self.parse_return()), false),
                "break" => {
                    let keyword = self.bump(LexMode::Words);
                    return (Statement::Break(keyword.span), false);
                }
                "continue" => {
                    let keyword = self.bump(LexMode::Words);
                    return (Statement::Continue(keyword.span), false);
                }
                "use" => return (Statement::Use(self.parse_use()), false),
                _ => {}
            }
        }
        (Statement::Pipeline(self.parse_pipeline()), false)
    }

    fn parse_let(&mut self) -> LetStmt {
        let keyword = self.bump(LexMode::Words);
        let (name, name_span) = self.expect_ident("a name for the binding");
        let ty = if self.eat(LexMode::Expr, TokenKind::Colon).is_some() {
            Some(self.parse_type())
        } else {
            None
        };
        let assign = self.peek(LexMode::Expr);
        if assign.kind == TokenKind::Eq {
            self.bump(LexMode::Expr);
        } else {
            let description = self.describe(assign);
            self.report_unexpected(assign, format!("expected `=`, found {description}"));
        }
        // `let hot = get process | where …` binds a pipeline; `let n = 3` binds a value. Both are
        // ordinary and the same lookahead that disambiguates `( … )` decides which is which.
        let value = if self.parens_hold_expression(assign) {
            let expression = self.parse_expression();
            expression_as_pipeline(expression)
        } else {
            self.parse_pipeline()
        };
        let span = keyword.span.join(value.span);
        LetStmt {
            name,
            name_span,
            ty,
            value,
            span,
        }
    }

    fn parse_fn(&mut self) -> FnDecl {
        let keyword = self.bump(LexMode::Words);
        let (name, name_span) = self.expect_ident("a name for the function");
        let mut parameters = Vec::new();
        if self
            .expect(LexMode::Expr, TokenKind::LParen, "`(`")
            .is_some()
        {
            loop {
                self.skip_newlines(LexMode::ExprOperand);
                let token = self.peek(LexMode::Expr);
                match token.kind {
                    TokenKind::RParen => {
                        self.bump(LexMode::Expr);
                        break;
                    }
                    TokenKind::Eof | TokenKind::LBrace => {
                        self.report_unexpected(token, "expected `)` to close the parameter list");
                        break;
                    }
                    _ => {}
                }
                let before = self.pos;
                parameters.push(self.parse_param());
                if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                    && !matches!(
                        self.peek(LexMode::Expr).kind,
                        TokenKind::RParen | TokenKind::Eof | TokenKind::LBrace
                    )
                {
                    let token = self.peek(LexMode::Expr);
                    let description = self.describe(token);
                    self.report_unexpected(
                        token,
                        format!("expected `,` or `)` in the parameter list, found {description}"),
                    );
                    self.bump(LexMode::Expr);
                }
                if self.pos == before {
                    self.bump(LexMode::Expr);
                }
            }
        }
        let return_type = if self.eat(LexMode::Expr, TokenKind::ThinArrow).is_some() {
            Some(self.parse_type())
        } else {
            None
        };
        let body = self.parse_block_or_error();
        let span = keyword.span.join(body.span);
        FnDecl {
            name,
            name_span,
            parameters,
            return_type,
            body,
            span,
        }
    }

    fn parse_param(&mut self) -> Param {
        let (name, name_span) = self.expect_ident("a parameter name");
        let ty = if self.eat(LexMode::Expr, TokenKind::Colon).is_some() {
            Some(self.parse_type())
        } else {
            None
        };
        let default = if self.eat(LexMode::Expr, TokenKind::Eq).is_some() {
            Some(self.parse_expression())
        } else {
            None
        };
        let end = default
            .as_ref()
            .map_or_else(|| ty.as_ref().map_or(name_span, |ty| ty.span), Expr::span);
        Param {
            name,
            name_span,
            ty,
            default,
            span: name_span.join(end),
        }
    }

    fn parse_type(&mut self) -> TypeRef {
        let (name, span) = self.expect_ident("a type name");
        let mut arguments = Vec::new();
        let mut end = span;
        if self.eat(LexMode::Expr, TokenKind::Lt).is_some() {
            loop {
                let token = self.peek(LexMode::Expr);
                if matches!(token.kind, TokenKind::Gt | TokenKind::Eof) {
                    if let Some(close) = self.eat(LexMode::Expr, TokenKind::Gt) {
                        end = close.span;
                    } else {
                        self.report_unexpected(token, "expected `>` to close the type arguments");
                    }
                    break;
                }
                let before = self.pos;
                let argument = self.parse_type();
                end = argument.span;
                arguments.push(argument);
                if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                    && !matches!(
                        self.peek(LexMode::Expr).kind,
                        TokenKind::Gt | TokenKind::Eof
                    )
                {
                    let token = self.peek(LexMode::Expr);
                    let description = self.describe(token);
                    self.report_unexpected(
                        token,
                        format!("expected `,` or `>` in the type arguments, found {description}"),
                    );
                    self.bump(LexMode::Expr);
                }
                if self.pos == before {
                    self.bump(LexMode::Expr);
                }
            }
        }
        let optional = if let Some(mark) = self.eat(LexMode::Expr, TokenKind::Question) {
            end = mark.span;
            true
        } else {
            false
        };
        TypeRef {
            name,
            arguments,
            optional,
            span: span.join(end),
        }
    }

    fn parse_if(&mut self) -> IfStmt {
        let keyword = self.bump(LexMode::Words);
        let mut branches = Vec::new();
        let mut else_block = None;
        let mut end;
        loop {
            let condition = self.parse_condition();
            let block = self.parse_block_or_error();
            end = block.span;
            branches.push(IfBranch {
                span: condition.span().join(block.span),
                condition,
                block,
            });
            if !self.at_keyword(LexMode::Words, "else") {
                break;
            }
            self.bump(LexMode::Words);
            if self.at_keyword(LexMode::Words, "if") {
                self.bump(LexMode::Words);
                continue;
            }
            let block = self.parse_block_or_error();
            end = block.span;
            else_block = Some(block);
            break;
        }
        IfStmt {
            branches,
            else_block,
            span: keyword.span.join(end),
        }
    }

    fn parse_for(&mut self) -> ForStmt {
        let keyword = self.bump(LexMode::Words);
        let (binding, binding_span) = self.expect_ident("a name to bind each item to");
        if !self.at_keyword(LexMode::Expr, "in") {
            let token = self.peek(LexMode::Expr);
            let description = self.describe(token);
            self.report_unexpected(token, format!("expected `in`, found {description}"));
        } else {
            self.bump(LexMode::Expr);
        }
        let iterable = self.parse_condition();
        let body = self.parse_block_or_error();
        ForStmt {
            binding,
            binding_span,
            iterable,
            span: keyword.span.join(body.span),
            body,
        }
    }

    fn parse_while(&mut self) -> WhileStmt {
        let keyword = self.bump(LexMode::Words);
        let condition = self.parse_condition();
        let body = self.parse_block_or_error();
        WhileStmt {
            condition,
            span: keyword.span.join(body.span),
            body,
        }
    }

    fn parse_match(&mut self) -> MatchStmt {
        let keyword = self.bump(LexMode::Words);
        let subject = self.parse_condition();
        let mut arms = Vec::new();
        let mut end = subject.span();
        if let Some(open) = self.expect(LexMode::Expr, TokenKind::LBrace, "`{`") {
            loop {
                self.skip_separators();
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
                arms.push(self.parse_match_arm());
                self.eat(LexMode::Expr, TokenKind::Comma);
                if self.pos == before {
                    self.bump(LexMode::Expr);
                }
            }
        }
        MatchStmt {
            subject,
            arms,
            span: keyword.span.join(end),
        }
    }

    fn parse_match_arm(&mut self) -> MatchArm {
        let pattern = self.parse_pattern();
        if self
            .expect(LexMode::Expr, TokenKind::FatArrow, "`=>`")
            .is_none()
        {
            let span = pattern.span();
            return MatchArm {
                pattern,
                body: MatchArmBody::Expr(Expr::Error(Span::at(span.end()))),
                span,
            };
        }
        let token = self.peek(LexMode::ExprOperand);
        let body = if token.kind == TokenKind::LBrace && !self.is_record_brace(token) {
            MatchArmBody::Block(self.parse_block())
        } else {
            MatchArmBody::Expr(self.parse_expression())
        };
        let end = match &body {
            MatchArmBody::Block(block) => block.span,
            MatchArmBody::Expr(expression) => expression.span(),
        };
        MatchArm {
            span: pattern.span().join(end),
            pattern,
            body,
        }
    }

    fn parse_pattern(&mut self) -> Pattern {
        let token = self.peek(LexMode::ExprOperand);
        match token.kind {
            TokenKind::Ident => {
                let text = token.text(self.source);
                match text {
                    "_" => {
                        self.bump(LexMode::ExprOperand);
                        Pattern::Wildcard(token.span)
                    }
                    "true" | "false" | "null" => Pattern::Literal(self.parse_expression()),
                    _ => {
                        self.bump(LexMode::ExprOperand);
                        Pattern::Binding {
                            name: text.to_owned(),
                            span: token.span,
                        }
                    }
                }
            }
            TokenKind::Int
            | TokenKind::Float
            | TokenKind::Unit
            | TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr
            | TokenKind::Regex
            | TokenKind::UnterminatedRegex
            | TokenKind::Minus => Pattern::Literal(self.parse_expression()),
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected a pattern, found {description}"));
                Pattern::Error(Span::at(token.span.start()))
            }
        }
    }

    fn parse_try(&mut self) -> TryStmt {
        let keyword = self.bump(LexMode::Words);
        let body = self.parse_block_or_error();
        let mut end = body.span;
        let catch = if self.at_keyword(LexMode::Words, "catch") {
            let catch_keyword = self.bump(LexMode::Words);
            let mut binding = None;
            let mut binding_span = None;
            if self.peek(LexMode::Expr).kind == TokenKind::Ident {
                let token = self.bump(LexMode::Expr);
                binding = Some(token.text(self.source).to_owned());
                binding_span = Some(token.span);
            }
            let handler = self.parse_block_or_error();
            end = handler.span;
            Some(CatchClause {
                binding,
                binding_span,
                span: catch_keyword.span.join(handler.span),
                body: handler,
            })
        } else {
            None
        };
        TryStmt {
            body,
            catch,
            span: keyword.span.join(end),
        }
    }

    fn parse_return(&mut self) -> ReturnStmt {
        let keyword = self.bump(LexMode::Words);
        let token = self.peek(LexMode::ExprOperand);
        let value = if matches!(
            token.kind,
            TokenKind::Eof
                | TokenKind::Newline
                | TokenKind::Semi
                | TokenKind::RBrace
                | TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::Comma
                | TokenKind::Pipe
        ) {
            None
        } else {
            Some(self.parse_expression())
        };
        let span = value
            .as_ref()
            .map_or(keyword.span, |value| keyword.span.join(value.span()));
        ReturnStmt { value, span }
    }

    fn parse_use(&mut self) -> UseStmt {
        let keyword = self.bump(LexMode::Words);
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::Word {
            self.bump(LexMode::Words);
            let module = self.qualified_name(token);
            return UseStmt {
                span: keyword.span.join(module.span),
                module,
            };
        }
        let description = self.describe(token);
        self.report_unexpected(
            token,
            format!("expected a module name, found {description}"),
        );
        UseStmt {
            module: QualifiedName {
                namespace: None,
                name: String::new(),
                span: Span::at(token.span.start()),
            },
            span: keyword.span,
        }
    }

    fn expect_ident(&mut self, what: &str) -> (String, Span) {
        let token = self.peek(LexMode::Expr);
        if token.kind == TokenKind::Ident {
            self.bump(LexMode::Expr);
            return (token.text(self.source).to_owned(), token.span);
        }
        let description = self.describe(token);
        self.report_unexpected(token, format!("expected {what}, found {description}"));
        (String::new(), Span::at(token.span.start()))
    }

    fn expect(&mut self, mode: LexMode, kind: TokenKind, what: &str) -> Option<Token> {
        let token = self.peek(mode);
        if token.kind == kind {
            return Some(self.bump(mode));
        }
        let description = self.describe(token);
        self.report_unexpected(token, format!("expected {what}, found {description}"));
        None
    }

    fn skip_newlines(&mut self, mode: LexMode) {
        while self.peek(mode).kind == TokenKind::Newline {
            self.bump(mode);
        }
    }

    /// Parses the expression that guards a block, where a `{` always opens the block.
    fn parse_condition(&mut self) -> Expr {
        let saved = self.no_brace;
        self.no_brace = true;
        let expression = self.parse_expression();
        self.no_brace = saved;
        expression
    }

    fn parse_block_or_error(&mut self) -> Block {
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::LBrace {
            return self.parse_block();
        }
        let description = self.describe(token);
        self.report_unexpected(token, format!("expected a block, found {description}"));
        Block {
            statements: Vec::new(),
            span: Span::at(token.span.start()),
        }
    }

    /// Parses `{ statements }`, with the `{` still unconsumed.
    fn parse_block(&mut self) -> Block {
        let open = self.bump(LexMode::Words);
        let mut statements = Vec::new();
        let end;
        loop {
            self.skip_separators();
            let token = self.peek(LexMode::Words);
            match token.kind {
                TokenKind::RBrace => {
                    end = self.bump(LexMode::Words).span;
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
            let saved = self.no_brace;
            self.no_brace = false;
            self.parse_statement_into(&mut statements);
            self.no_brace = saved;
            if self.pos == before {
                self.bump(LexMode::Words);
            }
        }
        Block {
            statements,
            span: open.span.join(end),
        }
    }
}

/// Tokens that end a stage without belonging to it.
const fn ends_stage(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Eof
            | TokenKind::Newline
            | TokenKind::Semi
            | TokenKind::Pipe
            | TokenKind::AndAnd
            | TokenKind::OrOr
            | TokenKind::Amp
            | TokenKind::Comma
            | TokenKind::RParen
            | TokenKind::RBracket
            | TokenKind::RBrace
    )
}

impl Parser<'_> {
    // --- pipelines and stages -----------------------------------------------------------

    fn parse_pipeline(&mut self) -> Pipeline {
        let head = self.parse_stage_list();
        let start = head.span;
        let mut end = head.span;
        let mut tail = Vec::new();
        loop {
            let token = self.peek(LexMode::Words);
            let op = match token.kind {
                TokenKind::AndAnd => ChainOp::And,
                TokenKind::OrOr => ChainOp::Or,
                _ => break,
            };
            self.bump(LexMode::Words);
            self.skip_newlines(LexMode::Words);
            let list = self.parse_stage_list();
            end = list.span;
            tail.push(ChainedList {
                op,
                op_span: token.span,
                list,
            });
        }
        let background = if let Some(amp) = self.eat(LexMode::Words, TokenKind::Amp) {
            end = amp.span;
            true
        } else {
            false
        };
        Pipeline {
            head,
            tail,
            background,
            span: start.join(end),
        }
    }

    fn parse_stage_list(&mut self) -> StageList {
        let first = self.parse_stage();
        let start = first.span;
        let mut end = first.span;
        let mut stages = vec![first];
        while self.eat(LexMode::Words, TokenKind::Pipe).is_some() {
            self.skip_newlines(LexMode::Words);
            let stage = self.parse_stage();
            end = stage.span;
            stages.push(stage);
        }
        StageList {
            stages,
            span: start.join(end),
        }
    }

    fn parse_stage(&mut self) -> Stage {
        if self.depth >= MAX_DEPTH {
            let token = self.peek(LexMode::Words);
            self.report(Diagnostic::syntax(
                token.span,
                "this line nests more deeply than the parser will follow",
            ));
            if !ends_stage(token.kind) {
                self.bump(LexMode::Words);
            }
            return Stage {
                head: StageHead::Error(Span::at(token.span.start())),
                mode: ArgMode::Words,
                arguments: Vec::new(),
                redirections: Vec::new(),
                span: Span::at(token.span.start()),
            };
        }
        self.depth += 1;
        let stage = self.parse_stage_inner();
        self.depth -= 1;
        stage
    }

    fn parse_stage_inner(&mut self) -> Stage {
        let token = self.peek(LexMode::Words);
        let head = match token.kind {
            TokenKind::Word => {
                self.bump(LexMode::Words);
                StageHead::Command(self.qualified_name(token))
            }
            TokenKind::Variable => {
                self.bump(LexMode::Words);
                StageHead::Value(self.words_variable(token))
            }
            TokenKind::LParen => {
                let paren = self.parse_paren_value();
                StageHead::Value(Expr::Paren(Box::new(paren)))
            }
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected a command, found {description}"));
                if !ends_stage(token.kind) {
                    self.bump(LexMode::Words);
                }
                StageHead::Error(Span::at(token.span.start()))
            }
        };
        let mode = head.name().map_or(ArgMode::Words, ArgMode::for_head);
        let lex_mode = match mode {
            ArgMode::Words => LexMode::Words,
            ArgMode::Expression => LexMode::ExprOperand,
        };

        let mut arguments = Vec::new();
        let mut redirections = Vec::new();
        let mut end = head.span();
        loop {
            let token = self.peek(lex_mode);
            if ends_stage(token.kind) {
                break;
            }
            let before = self.pos;
            match (mode, token.kind) {
                (
                    ArgMode::Words,
                    TokenKind::Gt
                    | TokenKind::GtGt
                    | TokenKind::Lt
                    | TokenKind::GtAmp
                    | TokenKind::LtAmp,
                ) => {
                    let redirection = self.parse_redirection(token);
                    end = redirection.span;
                    redirections.push(redirection);
                }
                (
                    ArgMode::Expression,
                    TokenKind::Gt | TokenKind::Lt | TokenKind::GtEq | TokenKind::LtEq,
                ) => {
                    self.report(
                        Diagnostic::syntax(
                            token.span,
                            "a stage whose arguments are expressions cannot be redirected",
                        )
                        .with_help("serialize the stream first, then redirect: `| to text > file`"),
                    );
                    self.bump(lex_mode);
                }
                (ArgMode::Words, _) => {
                    let argument = self.parse_words_argument(token);
                    end = argument.span();
                    arguments.push(argument);
                }
                (ArgMode::Expression, _) => {
                    let argument = Argument::Value(self.parse_expression());
                    end = argument.span();
                    arguments.push(argument);
                }
            }
            if self.pos == before {
                self.bump(lex_mode);
            }
        }
        Stage {
            span: head.span().join(end),
            head,
            mode,
            arguments,
            redirections,
        }
    }

    /// Splits `namespace:name` apart, leaving anything else as a bare name (spec §6.5).
    fn qualified_name(&self, token: Token) -> QualifiedName {
        let text = token.text(self.source);
        if let Some((namespace, name)) = text.split_once(':') {
            let is_ident = !namespace.is_empty()
                && namespace.bytes().next().is_some_and(is_ident_start)
                && namespace.bytes().all(is_ident_continue);
            let is_bare = !name.is_empty() && !name.contains([':', '/']);
            if is_ident && is_bare {
                return QualifiedName {
                    namespace: Some(namespace.to_owned()),
                    name: name.to_owned(),
                    span: token.span,
                };
            }
        }
        QualifiedName {
            namespace: None,
            name: text.to_owned(),
            span: token.span,
        }
    }

    fn parse_redirection(&mut self, token: Token) -> Redirection {
        self.bump(LexMode::Words);
        let text = token.text(self.source);
        let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
        let fd = if digits.is_empty() {
            None
        } else if let Ok(number) = digits.parse::<u32>() {
            Some(number)
        } else {
            self.report(Diagnostic::syntax(
                token.span,
                "this file descriptor is out of range",
            ));
            None
        };
        let op = match token.kind {
            TokenKind::Gt => RedirectOp::Write,
            TokenKind::GtGt => RedirectOp::Append,
            TokenKind::Lt => RedirectOp::Read,
            TokenKind::GtAmp => RedirectOp::DupWrite,
            _ => RedirectOp::DupRead,
        };
        let (target, end) = match op {
            RedirectOp::DupWrite | RedirectOp::DupRead => self.parse_fd_target(token),
            _ => self.parse_redirect_target(token),
        };
        Redirection {
            fd,
            op,
            target,
            span: token.span.join(end),
        }
    }

    fn parse_fd_target(&mut self, operator: Token) -> (RedirectTarget, Span) {
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::Word {
            let text = token.text(self.source);
            if let Ok(number) = text.parse::<u32>() {
                self.bump(LexMode::Words);
                return (RedirectTarget::Fd(number), token.span);
            }
        }
        let description = self.describe(token);
        self.report_unexpected(
            token,
            format!("expected a file descriptor to duplicate, found {description}"),
        );
        (
            RedirectTarget::Error(Span::at(token.span.start())),
            operator.span,
        )
    }

    fn parse_redirect_target(&mut self, operator: Token) -> (RedirectTarget, Span) {
        let token = self.peek(LexMode::Words);
        match token.kind {
            TokenKind::Word => {
                self.bump(LexMode::Words);
                (
                    RedirectTarget::Word(WordArg {
                        text: token.text(self.source).to_owned(),
                        span: token.span,
                    }),
                    token.span,
                )
            }
            TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr => {
                let value = self.parse_string(token);
                (RedirectTarget::Value(value), token.span)
            }
            TokenKind::Variable => {
                self.bump(LexMode::Words);
                let value = self.words_variable(token);
                (RedirectTarget::Value(value), token.span)
            }
            _ => {
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected a redirection target, found {description}"),
                );
                (
                    RedirectTarget::Error(Span::at(token.span.start())),
                    operator.span,
                )
            }
        }
    }

    // --- words-mode arguments ------------------------------------------------------------

    fn parse_words_argument(&mut self, token: Token) -> Argument {
        match token.kind {
            TokenKind::Word => self.parse_word_argument(token),
            TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr => Argument::Value(self.parse_string(token)),
            TokenKind::Variable => {
                self.bump(LexMode::Words);
                Argument::Value(self.words_variable(token))
            }
            TokenKind::CurrentValue => {
                self.bump(LexMode::Words);
                Argument::Value(self.words_current_value(token))
            }
            TokenKind::LParen => {
                let paren = self.parse_paren_value();
                Argument::Value(Expr::Paren(Box::new(paren)))
            }
            TokenKind::LBracket => Argument::Value(self.parse_list()),
            TokenKind::LBrace => Argument::Value(self.parse_brace(token)),
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected an argument, found {description}"));
                self.bump(LexMode::Words);
                Argument::Error(token.span)
            }
        }
    }

    fn parse_word_argument(&mut self, token: Token) -> Argument {
        self.bump(LexMode::Words);
        let text = token.text(self.source);
        if let Some(rest) = text.strip_prefix("--")
            && !rest.is_empty()
        {
            let (name, value_text) = match rest.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (rest, None),
            };
            let named = !name.is_empty()
                && name.bytes().next().is_some_and(is_ident_start)
                && name.bytes().all(is_ident_continue);
            if named {
                let value_start = token.span.start() + 2 + name.len() as u32 + 1;
                let value = value_text
                    .map(|text| self.option_value(text, Span::new(value_start, token.span.end())));
                let span = value
                    .as_ref()
                    .map_or(token.span, |value| token.span.join(value.span()));
                return Argument::Option(OptionArg {
                    name: name.to_owned(),
                    value,
                    span,
                });
            }
        }
        Argument::Word(WordArg {
            text: text.to_owned(),
            span: token.span,
        })
    }

    /// The value of `--name=value`: the text after the `=`, or an adjacent quoted or
    /// parenthesised value when the option ends with the `=`.
    fn option_value(&mut self, text: &str, span: Span) -> Expr {
        if text.is_empty() {
            let token = self.peek(LexMode::Words);
            if token.span.start() == span.end() {
                match token.kind {
                    TokenKind::Str
                    | TokenKind::RawStr
                    | TokenKind::UnterminatedStr
                    | TokenKind::UnterminatedRawStr => return self.parse_string(token),
                    TokenKind::LParen => {
                        let paren = self.parse_paren_value();
                        return Expr::Paren(Box::new(paren));
                    }
                    TokenKind::Variable => {
                        self.bump(LexMode::Words);
                        return self.words_variable(token);
                    }
                    _ => {}
                }
            }
        }
        Expr::Str(StrLit {
            parts: vec![StrPart::Text {
                text: text.to_owned(),
                span,
            }],
            raw: true,
            span,
        })
    }

    /// Expands a words-mode `$name.field.field` token, whose steps the lexer folded in.
    fn words_variable(&mut self, token: Token) -> Expr {
        let text = token.text(self.source);
        let start = token.span.start();
        let mut steps = text.split('.');
        let name = steps.next().unwrap_or("$");
        let mut offset = start + name.len() as u32;
        let mut expression = Expr::Variable(Variable {
            name: name.strip_prefix('$').unwrap_or(name).to_owned(),
            span: Span::new(start, offset),
        });
        for step in steps {
            let field_span = Span::new(offset + 1, offset + 1 + step.len() as u32);
            offset = field_span.end();
            expression = Expr::Field(Box::new(FieldAccess {
                span: Span::new(start, offset),
                base: expression,
                field: step.to_owned(),
                field_span,
                optional: false,
            }));
        }
        expression
    }

    /// Expands a words-mode `@`, `@-1` or `@3` token and any folded `.field` steps.
    fn words_current_value(&mut self, token: Token) -> Expr {
        let text = token.text(self.source);
        let start = token.span.start();
        let mut steps = text.split('.');
        let head = steps.next().unwrap_or("@");
        let mut offset = start + head.len() as u32;
        let mut expression = Expr::CurrentValue(CurrentValue {
            selector: current_selector(head),
            span: Span::new(start, offset),
        });
        for step in steps {
            let field_span = Span::new(offset + 1, offset + 1 + step.len() as u32);
            offset = field_span.end();
            expression = Expr::Field(Box::new(FieldAccess {
                span: Span::new(start, offset),
                base: expression,
                field: step.to_owned(),
                field_span,
                optional: false,
            }));
        }
        expression
    }
}

fn current_selector(text: &str) -> CurrentSelector {
    let digits = text.trim_start_matches('@');
    if let Some(previous) = digits.strip_prefix('-') {
        CurrentSelector::Previous(previous.parse().unwrap_or(1))
    } else if digits.is_empty() {
        CurrentSelector::Current
    } else {
        CurrentSelector::Item(digits.parse().unwrap_or(0))
    }
}

impl Parser<'_> {
    // --- expressions ----------------------------------------------------------------------

    fn parse_expression(&mut self) -> Expr {
        self.parse_logical_or()
    }

    fn parse_logical_or(&mut self) -> Expr {
        let mut lhs = self.parse_logical_and();
        while self.at_keyword(LexMode::Expr, "or") {
            let op = self.bump(LexMode::Expr);
            let rhs = self.parse_logical_and();
            lhs = binary(BinaryOp::Or, op.span, lhs, rhs);
        }
        lhs
    }

    fn parse_logical_and(&mut self) -> Expr {
        let mut lhs = self.parse_equality();
        while self.at_keyword(LexMode::Expr, "and") {
            let op = self.bump(LexMode::Expr);
            let rhs = self.parse_equality();
            lhs = binary(BinaryOp::And, op.span, lhs, rhs);
        }
        lhs
    }

    fn parse_equality(&mut self) -> Expr {
        let mut lhs = self.parse_comparison();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::EqEq => BinaryOp::Eq,
                TokenKind::BangEq => BinaryOp::NotEq,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_comparison();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    fn parse_comparison(&mut self) -> Expr {
        let mut lhs = self.parse_membership();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::Lt => BinaryOp::Lt,
                TokenKind::LtEq => BinaryOp::LtEq,
                TokenKind::Gt => BinaryOp::Gt,
                TokenKind::GtEq => BinaryOp::GtEq,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_membership();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    fn parse_membership(&mut self) -> Expr {
        let mut lhs = self.parse_additive();
        loop {
            let token = self.peek(LexMode::Expr);
            let (op, op_span) = match token.kind {
                TokenKind::Match => (BinaryOp::Match, token.span),
                TokenKind::NotMatch => (BinaryOp::NotMatch, token.span),
                TokenKind::Ident if token.text(self.source) == "in" => (BinaryOp::In, token.span),
                TokenKind::Ident if token.text(self.source) == "not" => {
                    let following = self.peek_after(LexMode::Expr, token);
                    if following.kind != TokenKind::Ident || following.text(self.source) != "in" {
                        break;
                    }
                    self.bump(LexMode::Expr);
                    (BinaryOp::NotIn, token.span.join(following.span))
                }
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_additive();
            lhs = binary(op, op_span, lhs, rhs);
        }
        lhs
    }

    fn parse_additive(&mut self) -> Expr {
        let mut lhs = self.parse_multiplicative();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Sub,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_multiplicative();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let mut lhs = self.parse_unary();
        loop {
            let token = self.peek(LexMode::Expr);
            let op = match token.kind {
                TokenKind::Star => BinaryOp::Mul,
                TokenKind::Slash => BinaryOp::Div,
                TokenKind::Percent => BinaryOp::Rem,
                _ => break,
            };
            self.bump(LexMode::Expr);
            let rhs = self.parse_unary();
            lhs = binary(op, token.span, lhs, rhs);
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
        let token = self.peek(LexMode::ExprOperand);
        let op = match token.kind {
            TokenKind::Minus => UnaryOp::Neg,
            TokenKind::Ident if token.text(self.source) == "not" => UnaryOp::Not,
            _ => return self.parse_postfix(),
        };
        self.bump(LexMode::ExprOperand);
        let operand = self.parse_unary();
        Expr::Unary(Box::new(UnaryExpr {
            op,
            op_span: token.span,
            span: token.span.join(operand.span()),
            operand,
        }))
    }

    fn parse_postfix(&mut self) -> Expr {
        let mut expression = self.parse_primary();
        loop {
            let token = self.peek(LexMode::Expr);
            let adjacent = token.span.start() == expression.span().end();
            match token.kind {
                TokenKind::Dot | TokenKind::QuestionDot => {
                    self.bump(LexMode::Expr);
                    let optional = token.kind == TokenKind::QuestionDot;
                    let name = self.peek(LexMode::Expr);
                    let (field, field_span) = if name.kind == TokenKind::Ident {
                        self.bump(LexMode::Expr);
                        (name.text(self.source).to_owned(), name.span)
                    } else {
                        let description = self.describe(name);
                        self.report_unexpected(
                            name,
                            format!("expected a field name, found {description}"),
                        );
                        (String::new(), Span::at(name.span.start()))
                    };
                    expression = Expr::Field(Box::new(FieldAccess {
                        span: expression.span().join(field_span),
                        base: expression,
                        field,
                        field_span,
                        optional,
                    }));
                    if field_span.is_empty() {
                        break;
                    }
                }
                TokenKind::LBracket if adjacent => {
                    self.bump(LexMode::Expr);
                    let saved = self.no_brace;
                    self.no_brace = false;
                    let index = self.parse_expression();
                    self.no_brace = saved;
                    let end = self
                        .close(TokenKind::RBracket, token, "`]`")
                        .unwrap_or(index.span());
                    expression = Expr::Index(Box::new(IndexExpr {
                        span: expression.span().join(end),
                        base: expression,
                        index,
                    }));
                }
                TokenKind::LParen if adjacent => {
                    let (arguments, end) = self.parse_call_arguments(token);
                    expression = Expr::Call(Box::new(CallExpr {
                        span: expression.span().join(end),
                        callee: expression,
                        arguments,
                    }));
                }
                _ => break,
            }
        }
        expression
    }

    fn parse_call_arguments(&mut self, open: Token) -> (Vec<Expr>, Span) {
        self.bump(LexMode::Expr);
        let saved = self.no_brace;
        self.no_brace = false;
        let mut arguments = Vec::new();
        let end;
        loop {
            self.skip_newlines(LexMode::ExprOperand);
            let token = self.peek(LexMode::ExprOperand);
            match token.kind {
                TokenKind::RParen => {
                    end = self.bump(LexMode::ExprOperand).span;
                    break;
                }
                TokenKind::Eof => {
                    self.report(Diagnostic::incomplete(
                        open.span,
                        "this `(` is never closed",
                    ));
                    end = token.span;
                    break;
                }
                _ => {}
            }
            let before = self.pos;
            arguments.push(self.parse_expression());
            self.skip_newlines(LexMode::Expr);
            if self.eat(LexMode::Expr, TokenKind::Comma).is_none()
                && !matches!(
                    self.peek(LexMode::Expr).kind,
                    TokenKind::RParen | TokenKind::Eof
                )
            {
                let token = self.peek(LexMode::Expr);
                let description = self.describe(token);
                self.report_unexpected(
                    token,
                    format!("expected `,` or `)` in the argument list, found {description}"),
                );
                self.bump(LexMode::Expr);
            }
            if self.pos == before {
                self.bump(LexMode::Expr);
            }
        }
        self.no_brace = saved;
        (arguments, end)
    }

    /// Consumes the closing delimiter of a bracketed form, or reports that it is missing.
    fn close(&mut self, kind: TokenKind, open: Token, what: &str) -> Option<Span> {
        self.skip_newlines(LexMode::Expr);
        if let Some(token) = self.eat(LexMode::Expr, kind) {
            return Some(token.span);
        }
        let token = self.peek(LexMode::Expr);
        if token.kind == TokenKind::Eof {
            let opener = open.text(self.source);
            self.report(Diagnostic::incomplete(
                open.span,
                format!("this `{opener}` is never closed"),
            ));
        } else {
            let description = self.describe(token);
            self.report_unexpected(token, format!("expected {what}, found {description}"));
        }
        None
    }

    fn parse_primary(&mut self) -> Expr {
        if self.depth >= MAX_DEPTH {
            let token = self.peek(LexMode::ExprOperand);
            self.report(Diagnostic::syntax(
                token.span,
                "this expression nests more deeply than the parser will follow",
            ));
            if !ends_stage(token.kind) {
                self.bump(LexMode::ExprOperand);
            }
            return Expr::Error(Span::at(token.span.start()));
        }
        self.depth += 1;
        let expression = self.parse_primary_inner();
        self.depth -= 1;
        expression
    }

    fn parse_primary_inner(&mut self) -> Expr {
        let token = self.peek(LexMode::ExprOperand);
        match token.kind {
            TokenKind::Int | TokenKind::Float => {
                self.bump(LexMode::ExprOperand);
                Expr::Number(NumberLit {
                    value: self.number_value(token.text(self.source), token.span),
                    span: token.span,
                })
            }
            TokenKind::Unit => {
                self.bump(LexMode::ExprOperand);
                let text = token.text(self.source);
                let (number, unit) = split_unit(text);
                Expr::Unit(UnitLit {
                    value: self.number_value(number, token.span),
                    unit,
                    span: token.span,
                })
            }
            TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr => self.parse_string(token),
            TokenKind::Regex | TokenKind::UnterminatedRegex => {
                self.bump(LexMode::ExprOperand);
                if token.kind == TokenKind::UnterminatedRegex {
                    self.report(Diagnostic::incomplete(
                        token.span,
                        "this regex literal is never closed",
                    ));
                }
                let text = token.text(self.source);
                let body = text.strip_prefix('/').unwrap_or(text);
                let (pattern, flags) = match body.rfind('/') {
                    Some(index) if token.kind == TokenKind::Regex => {
                        (&body[..index], &body[index + 1..])
                    }
                    _ => (body, ""),
                };
                Expr::Regex(RegexLit {
                    pattern: pattern.to_owned(),
                    flags: flags.to_owned(),
                    span: token.span,
                })
            }
            TokenKind::Variable => {
                self.bump(LexMode::ExprOperand);
                let text = token.text(self.source);
                Expr::Variable(Variable {
                    name: text.strip_prefix('$').unwrap_or(text).to_owned(),
                    span: token.span,
                })
            }
            TokenKind::CurrentValue => {
                self.bump(LexMode::ExprOperand);
                Expr::CurrentValue(CurrentValue {
                    selector: current_selector(token.text(self.source)),
                    span: token.span,
                })
            }
            TokenKind::Ident => {
                self.bump(LexMode::ExprOperand);
                match token.text(self.source) {
                    "true" => Expr::Bool(true, token.span),
                    "false" => Expr::Bool(false, token.span),
                    "null" => Expr::Null(token.span),
                    name => Expr::Path(FieldPath {
                        name: name.to_owned(),
                        span: token.span,
                    }),
                }
            }
            TokenKind::LBracket => self.parse_list(),
            TokenKind::LBrace if !self.no_brace => self.parse_brace(token),
            TokenKind::LParen => {
                let paren = self.parse_paren_value();
                Expr::Paren(Box::new(paren))
            }
            _ => {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected a value, found {description}"));
                Expr::Error(Span::at(token.span.start()))
            }
        }
    }

    fn number_value(&mut self, text: &str, span: Span) -> NumberValue {
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

    fn parse_list(&mut self) -> Expr {
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
    fn is_record_brace(&self, open: Token) -> bool {
        let first = self.peek_after(LexMode::ExprOperand, open);
        match first.kind {
            TokenKind::RBrace => true,
            TokenKind::Ident | TokenKind::Str | TokenKind::RawStr => {
                self.peek_after(LexMode::Expr, first).kind == TokenKind::Colon
            }
            _ => false,
        }
    }

    fn parse_brace(&mut self, open: Token) -> Expr {
        if self.is_record_brace(open) {
            self.parse_record(open)
        } else {
            Expr::Block(self.parse_block())
        }
    }

    fn parse_record(&mut self, open: Token) -> Expr {
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

    fn parse_record_field(&mut self) -> RecordField {
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

    /// Parses `( … )`, deciding between a nested pipeline and a grouped expression.
    ///
    /// The decision is made from the first three tokens inside the parentheses, so it costs the
    /// same whatever the content is and never re-parses: a bare name (`(ls)`), a name followed
    /// by another argument (`(get process | count)`), or an operator glued to its right operand
    /// with whitespace on its left (`(ls -la)`) is a command invocation. Everything else —
    /// a literal, a variable, a call, a field access, an operator with room around it — is an
    /// expression, so `(a + b)` and `(now() - 7d)` group values.
    fn parse_paren_value(&mut self) -> ParenValue {
        let open = self.bump(LexMode::Words);
        let saved = self.no_brace;
        self.no_brace = false;
        let inner = if self.parens_hold_expression(open) {
            let expression = self.parse_expression();
            ParenInner::Expr(expression)
        } else {
            self.skip_newlines(LexMode::Words);
            ParenInner::Pipeline(self.parse_pipeline())
        };
        self.skip_newlines(LexMode::Words);
        let end = if let Some(close) = self.eat(LexMode::Words, TokenKind::RParen) {
            close.span
        } else {
            let token = self.peek(LexMode::Words);
            if token.kind == TokenKind::Eof {
                self.report(Diagnostic::incomplete(
                    open.span,
                    "this `(` is never closed",
                ));
            } else {
                let description = self.describe(token);
                self.report_unexpected(token, format!("expected `)`, found {description}"));
            }
            token.span
        };
        self.no_brace = saved;
        let inner_span = match &inner {
            ParenInner::Expr(expression) => expression.span(),
            ParenInner::Pipeline(pipeline) => pipeline.span,
        };
        ParenValue {
            inner,
            span: open.span.join(inner_span).join(end),
        }
    }

    /// Whether `( … )` opened at `open` groups a value rather than running a command.
    fn parens_hold_expression(&self, open: Token) -> bool {
        let first = self.peek_after(LexMode::ExprOperand, open);
        if first.kind != TokenKind::Ident {
            return !matches!(first.kind, TokenKind::RParen | TokenKind::Eof);
        }
        if matches!(first.text(self.source), "true" | "false" | "null" | "not") {
            return true;
        }
        let second = self.peek_after(LexMode::Expr, first);
        let adjacent = first.span.end() == second.span.start();
        match second.kind {
            TokenKind::Dot | TokenKind::QuestionDot => true,
            TokenKind::LParen | TokenKind::LBracket => adjacent,
            TokenKind::EqEq
            | TokenKind::BangEq
            | TokenKind::Lt
            | TokenKind::LtEq
            | TokenKind::Gt
            | TokenKind::GtEq
            | TokenKind::Match
            | TokenKind::NotMatch
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::Star
            | TokenKind::Slash
            | TokenKind::Percent => {
                // `ls -la` glues the operator to its right operand and leaves room on its left;
                // arithmetic a person writes does not.
                let third = self.peek_after(LexMode::ExprOperand, second);
                !(second.span.end() == third.span.start() && first.span.end() < second.span.start())
            }
            TokenKind::Ident => matches!(second.text(self.source), "and" | "or" | "in"),
            _ => false,
        }
    }
}

/// Wraps a bare expression as the one-stage pipeline the AST models a binding's value as.
///
/// `let name = "world"` and `let hot = get process | …` then have one shape, so the evaluator has
/// one path rather than two that can drift apart.
fn expression_as_pipeline(expression: Expr) -> Pipeline {
    let span = expression.span();
    Pipeline {
        head: StageList {
            stages: vec![Stage {
                head: StageHead::Value(expression),
                mode: ArgMode::Expression,
                arguments: Vec::new(),
                redirections: Vec::new(),
                span,
            }],
            span,
        },
        tail: Vec::new(),
        background: false,
        span,
    }
}

fn binary(op: BinaryOp, op_span: Span, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Binary(Box::new(BinaryExpr {
        op,
        op_span,
        span: lhs.span().join(rhs.span()),
        lhs,
        rhs,
    }))
}

/// Splits `512MiB` into its numeric part and its unit.
fn split_unit(text: &str) -> (&str, crate::ast::Unit) {
    for (suffix, unit) in crate::lexer::UNIT_SUFFIXES {
        if let Some(number) = text.strip_suffix(suffix) {
            return (number, *unit);
        }
    }
    (text, crate::ast::Unit::Percent)
}

impl Parser<'_> {
    // --- string literals ------------------------------------------------------------------

    /// Reads a string literal and decodes it into literal text and interpolated expressions.
    fn parse_string(&mut self, token: Token) -> Expr {
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

    fn decode_interpolated(&mut self, start: u32, end: u32) -> Vec<StrPart> {
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
    fn interpolated_variable(
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
    fn interpolated_pipeline(
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
    fn decode_escape(&mut self, index: usize, end: usize) -> (Option<char>, usize) {
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

fn flush(parts: &mut Vec<StrPart>, text: &mut String, start: u32, end: u32) {
    if !text.is_empty() {
        parts.push(StrPart::Text {
            text: std::mem::take(text),
            span: Span::new(start, end),
        });
    }
}
