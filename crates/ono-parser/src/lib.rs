//! The Ono language front end: a mode-sensitive lexer, a syntax tree with precise spans, and a
//! recursive-descent parser that recovers instead of failing.
//!
//! The grammar is the one decided in ADR-0009 and written down in `docs/spec/grammar.ebnf`; the
//! narrative specification's §26 sketch is resolved there, not here.
//!
//! # The two argument modes
//!
//! A stage's head word decides how its arguments are read, and the table is static, so a line
//! can be parsed before any command registry exists (spec §24.4):
//!
//! ```
//! use ono_parser::{ArgMode, parse};
//! assert_eq!(ArgMode::for_head("ls"), ArgMode::Words);        // `>` redirects
//! assert_eq!(ArgMode::for_head("where"), ArgMode::Expression); // `>` compares
//!
//! let parsed = parse("cat a.txt > out.txt");
//! let stage = &parsed.program().statements[0].as_pipeline().unwrap().head.stages[0];
//! assert_eq!(stage.redirections.len(), 1);
//! ```
//!
//! # Parsing what is only half typed
//!
//! [`parse`] never panics and always returns a tree. Input that is unfinished is reported as
//! `parse.incomplete`, input that is wrong as `parse.syntax`, and that difference is what lets
//! an editor tell "still typing" from "broken":
//!
//! ```
//! use ono_core::ErrorCode;
//! let parsed = ono_parser::parse("get process | where cpu >");
//! assert!(!parsed.is_complete());
//! assert!(!parsed.has_errors());
//! assert_eq!(parsed.diagnostics()[0].code(), ErrorCode::ParseIncomplete);
//! ```
//!
//! # Where this deviates from `docs/spec/grammar.ebnf`
//!
//! - The EBNF spells a duplication redirection `FD ( ">&" | "<&" ) FD`, requiring the leading
//!   descriptor. `<&0` and `>&2` are accepted without it, because they are the forms every
//!   shell user already types; the missing descriptor means 0 for `<&` and 1 for `>&`, and the
//!   [`Redirection::fd`] field is `None` so the evaluator can apply that default itself.
//! - `( … )` may hold a pipeline or an expression, and the EBNF does not say which wins. The
//!   content is read as an expression when that reading reaches the `)` without complaint and
//!   does not look like a command invocation; otherwise it is a pipeline. So `(a + b)` groups
//!   an expression and `(ls -la)`, `(get process | count)` run commands.
//! - Words mode keeps the exact source text of every word, including a word such as `1.2.3` or
//!   `4419` that could be read as a number: reinterpreting it is the evaluator's job, and an
//!   external command must receive what was typed (ADR-0009).

#![forbid(unsafe_code)]

pub mod ast;
mod diagnostic;
mod lexer;
mod parser;

pub use ast::{
    ArgMode, Argument, BinaryExpr, BinaryOp, Block, CallExpr, CatchClause, ChainOp, ChainedList,
    CurrentSelector, CurrentValue, Expr, FieldAccess, FieldPath, FnDecl, ForStmt, IfBranch, IfStmt,
    IndexExpr, IpLit, LetStmt, ListExpr, MatchArm, MatchArmBody, MatchStmt, NumberLit, NumberValue,
    OptionArg, Param, ParenInner, ParenValue, Pattern, Pipeline, Program, QualifiedName,
    RecordExpr, RecordField, RecordKey, RedirectOp, RedirectTarget, Redirection, RegexLit,
    ReturnStmt, Stage, StageHead, StageList, Statement, StrLit, StrPart, TryStmt, TypeRef,
    UnaryExpr, UnaryOp, Unit, UnitLit, UseStmt, Variable, WhileStmt, WordArg,
};
pub use diagnostic::Diagnostic;
pub use lexer::{Token, TokenKind};
pub use parser::{Parsed, parse, tokens};
