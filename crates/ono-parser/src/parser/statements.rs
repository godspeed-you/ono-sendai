//! The program and the statement forms that are not control constructs.
//!
//! `parse_program` is the top of the descent: it reads statements until the source ends, and
//! leans on `recovery` whenever one of them cannot be finished.

use ono_core::Span;

use crate::ast::{AliasStmt, LetStmt, Program, QualifiedName, ReturnStmt, Statement, UseStmt};
use crate::lexer::{LexMode, TokenKind};

use super::pipelines::expression_as_pipeline;
use super::state::Parser;

impl Parser<'_> {
    pub(super) fn parse_program(&mut self) -> Program {
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

    /// Parses one statement and the terminator that follows it.
    pub(super) fn parse_statement_into(&mut self, statements: &mut Vec<Statement>) {
        let (statement, block_terminated) = self.parse_statement();
        statements.push(statement);
        self.finish_statement(block_terminated);
    }

    /// Parses one statement. The flag says whether it ended with a block, in which case no
    /// terminator is required before the next statement.
    pub(super) fn parse_statement(&mut self) -> (Statement, bool) {
        let token = self.peek(LexMode::Words);
        if token.kind == TokenKind::Word {
            match token.text(self.source) {
                "let" => return (Statement::Let(self.parse_let()), false),
                "fn" => return (Statement::Fn(self.parse_fn()), true),
                "alias" => return (Statement::Alias(self.parse_alias()), false),
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

    pub(super) fn parse_let(&mut self) -> LetStmt {
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

    pub(super) fn parse_alias(&mut self) -> AliasStmt {
        let keyword = self.bump(LexMode::Words);
        let (name, name_span) = self.expect_ident("a name for the alias");
        let assign = self.peek(LexMode::Expr);
        if assign.kind == TokenKind::Eq {
            self.bump(LexMode::Expr);
        } else {
            let description = self.describe(assign);
            self.report_unexpected(assign, format!("expected `=`, found {description}"));
        }
        let value = self.parse_pipeline();
        let span = keyword.span.join(value.span);
        AliasStmt {
            name,
            name_span,
            value,
            span,
        }
    }

    pub(super) fn parse_return(&mut self) -> ReturnStmt {
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

    pub(super) fn parse_use(&mut self) -> UseStmt {
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
}
