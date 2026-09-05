//! The syntax tree of the Ono language, as fixed by ADR-0009 and `docs/contracts/grammar.ebnf`.
//!
//! Every node carries a [`Span`] into the source it was parsed from, so the editor can map a
//! cursor position to a node and a diagnostic to the text that caused it (spec §16.3, §24.4).
//! The tree is total: a construct the parser could not read becomes an explicit error node
//! rather than a missing branch, which is what lets a half-typed line still be highlighted.

use ono_core::Span;

/// How a stage's arguments are lexed and parsed, selected by its head word (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgMode {
    /// External commands and verb-target commands: `<` `>` `>>` redirect, bare tokens are words.
    Words,
    /// Transforms such as `where`: `<` `>` compare, bare identifiers are field paths.
    Expression,
}

/// The heads whose arguments are expressions rather than words (ADR-0009).
const EXPRESSION_HEADS: &[&str] = &[
    "count", "diff", "each", "elif", "group", "if", "join", "let", "match", "measure", "reduce",
    "return", "select", "skip", "sort", "take", "until", "where", "while",
];

/// The `(head, option)` pairs whose option value is a predicate expression rather than a word.
///
/// ADR-0138. `find place --where state == "running"` and `find place --where pid > 1` are
/// written in words mode, where `>` is a redirection and a bare identifier is a word — so the
/// value of `--where` would be unreadable as the predicate v0.4 §6.8 says it is. The table is
/// static and registry-free for the same reason [`EXPRESSION_HEADS`] is (ADR-0009): the editor
/// classifies a line at keystroke time.
const EXPRESSION_OPTIONS: &[(&str, &str)] = &[("find", "where")];

impl ArgMode {
    /// Whether the bare option `--<option>` of the stage head `<head>` takes an expression as its
    /// value, rather than the next word (ADR-0138).
    ///
    /// ```
    /// use ono_parser::ArgMode;
    /// assert!(ArgMode::option_takes_expression("find", "where"));
    /// assert!(!ArgMode::option_takes_expression("find", "name"));
    /// assert!(!ArgMode::option_takes_expression("grep", "where"));
    /// ```
    #[must_use]
    pub fn option_takes_expression(head: &str, option: &str) -> bool {
        EXPRESSION_OPTIONS
            .iter()
            .any(|(known_head, known_option)| *known_head == head && *known_option == option)
    }

    /// Every `(head, option)` pair whose option value is read as an expression (ADR-0138).
    ///
    /// `cargo run -p xtask -- spec-check` holds this against `docs/contracts/language.yaml`, so the
    /// table and the documented language cannot drift apart.
    #[must_use]
    pub fn expression_options() -> &'static [(&'static str, &'static str)] {
        EXPRESSION_OPTIONS
    }

    /// The argument mode a head word selects.
    ///
    /// The table is static and needs no command registry, so the editor can classify a line at
    /// keystroke time (ADR-0009). A namespaced head is classified by its bare name.
    ///
    /// ```
    /// use ono_parser::ArgMode;
    /// assert_eq!(ArgMode::for_head("where"), ArgMode::Expression);
    /// assert_eq!(ArgMode::for_head("ls"), ArgMode::Words);
    /// ```
    #[must_use]
    pub fn for_head(name: &str) -> Self {
        if EXPRESSION_HEADS.binary_search(&name).is_ok() {
            ArgMode::Expression
        } else {
            ArgMode::Words
        }
    }
}

/// A whole parsed source: the statements it contains, in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Program {
    /// The statements, in source order.
    pub statements: Vec<Statement>,
    /// The span covering the whole source that was parsed.
    pub span: Span,
}

/// One statement of a program or block.
#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// A pipeline, possibly chained on exit status and possibly backgrounded.
    Pipeline(Pipeline),
    /// `let name = pipeline`, the only binding form (ADR-0009).
    Let(LetStmt),
    /// `fn name(params) -> Type { … }`.
    Fn(FnDecl),
    /// `alias name = pipeline`.
    Alias(AliasStmt),
    /// `if … { … } else if … { … } else { … }`.
    If(IfStmt),
    /// `for name in expr { … }`.
    For(ForStmt),
    /// `while expr { … }`.
    While(WhileStmt),
    /// `match expr { pattern => … }`.
    Match(MatchStmt),
    /// `try { … } catch name { … }`.
    Try(TryStmt),
    /// `return expr?`.
    Return(ReturnStmt),
    /// `break`.
    Break(Span),
    /// `continue`.
    Continue(Span),
    /// `use module`.
    Use(UseStmt),
    /// A statement the parser could not read; a diagnostic describes why.
    Error(Span),
}

impl Statement {
    /// The source range this statement covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Statement::Pipeline(node) => node.span,
            Statement::Let(node) => node.span,
            Statement::Fn(node) => node.span,
            Statement::If(node) => node.span,
            Statement::For(node) => node.span,
            Statement::While(node) => node.span,
            Statement::Match(node) => node.span,
            Statement::Try(node) => node.span,
            Statement::Return(node) => node.span,
            Statement::Break(span) | Statement::Continue(span) | Statement::Error(span) => *span,
            Statement::Use(node) => node.span,
            Statement::Alias(node) => node.span,
        }
    }

    /// The pipeline this statement runs, if it is a pipeline statement.
    #[must_use]
    pub fn as_pipeline(&self) -> Option<&Pipeline> {
        match self {
            Statement::Pipeline(pipeline) => Some(pipeline),
            _ => None,
        }
    }
}

/// A pipeline: one or more stage lists chained on exit status, optionally detached.
#[derive(Debug, Clone, PartialEq)]
pub struct Pipeline {
    /// The first stage list, always present.
    pub head: StageList,
    /// Further stage lists, each with the operator that chained it to the previous one.
    pub tail: Vec<ChainedList>,
    /// Whether the pipeline ended with `&` and detaches into a job (spec §18.4).
    pub background: bool,
    /// The source range the pipeline covers.
    pub span: Span,
}

/// A stage list attached to the previous one by `&&` or `||`.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainedList {
    /// The operator that decides whether this list runs.
    pub op: ChainOp,
    /// The span of the operator itself.
    pub op_span: Span,
    /// The stage list to run.
    pub list: StageList,
}

/// The exit-status chaining operators. They combine pipelines, not truth values (ADR-0009).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChainOp {
    /// `&&` — run the right side only if the left side succeeded.
    And,
    /// `||` — run the right side only if the left side failed.
    Or,
}

/// Stages joined by `|`, through which values flow.
#[derive(Debug, Clone, PartialEq)]
pub struct StageList {
    /// The stages, in source order; never empty.
    pub stages: Vec<Stage>,
    /// The source range the list covers.
    pub span: Span,
}

/// One stage: a head, its arguments, and any redirections attached to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    /// What the stage invokes.
    pub head: StageHead,
    /// How the arguments were read, decided by the head (ADR-0009).
    pub mode: ArgMode,
    /// The arguments, in source order.
    pub arguments: Vec<Argument>,
    /// The redirections, in source order. Always empty in [`ArgMode::Expression`].
    pub redirections: Vec<Redirection>,
    /// The source range the stage covers.
    pub span: Span,
}

/// What a stage invokes.
#[derive(Debug, Clone, PartialEq)]
pub enum StageHead {
    /// A possibly namespaced command name, such as `get` or `exec:ls`.
    Command(QualifiedName),
    /// A value that produces the stage's input: a variable such as `$hot`, a field access into
    /// one, or a parenthesised pipeline.
    Value(Expr),
    /// No readable head; a diagnostic describes why.
    Error(Span),
}

impl StageHead {
    /// The source range the head covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            StageHead::Command(name) => name.span,
            StageHead::Value(expression) => expression.span(),
            StageHead::Error(span) => *span,
        }
    }

    /// The bare command name, if the head is a command.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            StageHead::Command(name) => Some(&name.name),
            _ => None,
        }
    }

    /// The qualified command name, if the head is a command.
    #[must_use]
    pub fn command(&self) -> Option<&QualifiedName> {
        match self {
            StageHead::Command(name) => Some(name),
            _ => None,
        }
    }

    /// The variable, if the head is a plain variable reference.
    #[must_use]
    pub fn variable(&self) -> Option<&Variable> {
        match self {
            StageHead::Value(Expr::Variable(variable)) => Some(variable),
            _ => None,
        }
    }

    /// The parenthesised pipeline or expression, if the head is one.
    #[must_use]
    pub fn paren(&self) -> Option<&ParenValue> {
        match self {
            StageHead::Value(Expr::Paren(paren)) => Some(paren),
            _ => None,
        }
    }
}

/// A name with an optional resolution namespace, such as `ono:get` (spec §6.5).
#[derive(Debug, Clone, PartialEq)]
pub struct QualifiedName {
    /// The namespace before the `:`, if one was written.
    pub namespace: Option<String>,
    /// The bare name.
    pub name: String,
    /// The source range the whole name covers.
    pub span: Span,
}

/// One argument of a stage.
#[derive(Debug, Clone, PartialEq)]
pub enum Argument {
    /// A bare word, retained exactly as typed so external argv stays byte-exact (ADR-0009).
    Word(WordArg),
    /// A long option, `--name` or `--name=value`.
    Option(OptionArg),
    /// A value: a literal, a variable, a nested pipeline, a list, a record, a block, or —
    /// in [`ArgMode::Expression`] — a whole expression.
    Value(Expr),
    /// An argument the parser could not read; a diagnostic describes why.
    Error(Span),
}

impl Argument {
    /// The source range the argument covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Argument::Word(word) => word.span,
            Argument::Option(option) => option.span,
            Argument::Value(expression) => expression.span(),
            Argument::Error(span) => *span,
        }
    }

    /// The exact source text of the argument, if it is a bare word.
    #[must_use]
    pub fn as_word(&self) -> Option<&str> {
        match self {
            Argument::Word(word) => Some(&word.text),
            _ => None,
        }
    }

    /// The expression, if the argument is a value.
    #[must_use]
    pub fn as_value(&self) -> Option<&Expr> {
        match self {
            Argument::Value(expression) => Some(expression),
            _ => None,
        }
    }
}

/// A bare word argument, with the exact text that was typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordArg {
    /// The source text, verbatim.
    pub text: String,
    /// The source range the word covers.
    pub span: Span,
}

/// A long option, `--name` or `--name=value`.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionArg {
    /// The option name, without the leading `--`.
    pub name: String,
    /// The value, if the option was written with one.
    pub value: Option<Expr>,
    /// The source range the option covers, including any value.
    pub span: Span,
}

/// A redirection attached to a words-mode stage (ADR-0009).
#[derive(Debug, Clone, PartialEq)]
pub struct Redirection {
    /// The file descriptor written before the operator, if one was.
    pub fd: Option<u32>,
    /// Which direction the redirection goes.
    pub op: RedirectOp,
    /// Where it goes.
    pub target: RedirectTarget,
    /// The source range the redirection covers, including the descriptor and the target.
    pub span: Span,
}

/// The direction and mode of a redirection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedirectOp {
    /// `>` — truncate and write.
    Write,
    /// `>>` — append.
    Append,
    /// `<` — read.
    Read,
    /// `>&` — duplicate an output descriptor.
    DupWrite,
    /// `<&` — duplicate an input descriptor.
    DupRead,
}

/// Where a redirection points.
#[derive(Debug, Clone, PartialEq)]
pub enum RedirectTarget {
    /// A bare word, typically a path.
    Word(WordArg),
    /// A string or a variable holding the path.
    Value(Expr),
    /// Another file descriptor, as in `2>&1`.
    Fd(u32),
    /// No readable target; a diagnostic describes why.
    Error(Span),
}

/// An expression, or a value used in words-mode argument position.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// An integer or floating-point literal.
    Number(NumberLit),
    /// A numeric literal with an adjacent unit suffix, such as `512MiB` (spec §10.6).
    Unit(UnitLit),
    /// A string literal, possibly interpolating.
    Str(StrLit),
    /// A regex literal, `/…/flags`.
    Regex(RegexLit),
    /// An IP address literal, in either family (spec §10.2).
    Ip(IpLit),
    /// A timestamp literal, `2000-01-01T00:00:00Z` (spec §6.3, ADR-0071).
    Timestamp(TimestampLit),
    /// `true` or `false`, with the span of the keyword.
    Bool(bool, Span),
    /// `null`, with the span of the keyword.
    Null(Span),
    /// A variable reference, `$name`.
    Variable(Variable),
    /// The current value, `@`, `@-1` or `@3` (spec §6.4).
    CurrentValue(CurrentValue),
    /// A bare field path step, such as `cpu`; further steps are [`Expr::Field`] nodes.
    Path(FieldPath),
    /// A list literal, `[a, b]`.
    List(ListExpr),
    /// A record literal, `{k: v}`.
    Record(RecordExpr),
    /// A block, `{ statements }`.
    Block(Block),
    /// A parenthesised pipeline or expression.
    Paren(Box<ParenValue>),
    /// A prefix operator applied to an operand.
    Unary(Box<UnaryExpr>),
    /// An infix operator applied to two operands.
    Binary(Box<BinaryExpr>),
    /// Field access, `base.field` or `base?.field`.
    Field(Box<FieldAccess>),
    /// Index access, `base[index]`.
    Index(Box<IndexExpr>),
    /// A call, `callee(a, b)`.
    Call(Box<CallExpr>),
    /// An expression the parser could not read; a diagnostic describes why.
    Error(Span),
}

impl Expr {
    /// The source range the expression covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Expr::Number(node) => node.span,
            Expr::Unit(node) => node.span,
            Expr::Str(node) => node.span,
            Expr::Regex(node) => node.span,
            Expr::Timestamp(node) => node.span,
            Expr::Ip(node) => node.span,
            Expr::Bool(_, span) | Expr::Null(span) | Expr::Error(span) => *span,
            Expr::Variable(node) => node.span,
            Expr::CurrentValue(node) => node.span,
            Expr::Path(node) => node.span,
            Expr::List(node) => node.span,
            Expr::Record(node) => node.span,
            Expr::Block(node) => node.span,
            Expr::Paren(node) => node.span,
            Expr::Unary(node) => node.span,
            Expr::Binary(node) => node.span,
            Expr::Field(node) => node.span,
            Expr::Index(node) => node.span,
            Expr::Call(node) => node.span,
        }
    }
}

/// The value of a numeric literal, before any unit is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NumberValue {
    /// A literal that fits in a signed 64-bit integer.
    Int(i64),
    /// A literal with a fractional part, an exponent, or a magnitude beyond `i64`.
    Float(f64),
}

/// An integer or floating-point literal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NumberLit {
    /// The value the literal denotes.
    pub value: NumberValue,
    /// The source range the literal covers.
    pub span: Span,
}

/// A numeric literal with an adjacent unit suffix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitLit {
    /// The numeric part.
    pub value: NumberValue,
    /// The unit the suffix names.
    pub unit: Unit,
    /// The source range the literal covers, including the suffix.
    pub span: Span,
}

/// The unit suffixes a numeric literal may carry (ADR-0009, spec §10.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(non_camel_case_types)]
pub enum Unit {
    /// Bytes.
    B,
    /// Kibibytes, 1024 bytes.
    KiB,
    /// Mebibytes.
    MiB,
    /// Gibibytes.
    GiB,
    /// Tebibytes.
    TiB,
    /// Pebibytes.
    PiB,
    /// Kilobytes, 1000 bytes.
    KB,
    /// Megabytes.
    MB,
    /// Gigabytes.
    GB,
    /// Terabytes.
    TB,
    /// Petabytes.
    PB,
    /// Nanoseconds.
    Ns,
    /// Microseconds.
    Us,
    /// Milliseconds.
    Ms,
    /// Seconds.
    S,
    /// Minutes.
    M,
    /// Hours.
    H,
    /// Days.
    D,
    /// Weeks.
    W,
    /// A percentage.
    Percent,
}

impl Unit {
    /// The suffix as it is written in source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::B => "B",
            Unit::KiB => "KiB",
            Unit::MiB => "MiB",
            Unit::GiB => "GiB",
            Unit::TiB => "TiB",
            Unit::PiB => "PiB",
            Unit::KB => "KB",
            Unit::MB => "MB",
            Unit::GB => "GB",
            Unit::TB => "TB",
            Unit::PB => "PB",
            Unit::Ns => "ns",
            Unit::Us => "us",
            Unit::Ms => "ms",
            Unit::S => "s",
            Unit::M => "m",
            Unit::H => "h",
            Unit::D => "d",
            Unit::W => "w",
            Unit::Percent => "%",
        }
    }

    /// Whether the unit measures an amount of data.
    #[must_use]
    pub const fn is_bytesize(self) -> bool {
        matches!(
            self,
            Unit::B
                | Unit::KiB
                | Unit::MiB
                | Unit::GiB
                | Unit::TiB
                | Unit::PiB
                | Unit::KB
                | Unit::MB
                | Unit::GB
                | Unit::TB
                | Unit::PB
        )
    }

    /// Whether the unit measures a span of time.
    #[must_use]
    pub const fn is_duration(self) -> bool {
        matches!(
            self,
            Unit::Ns | Unit::Us | Unit::Ms | Unit::S | Unit::M | Unit::H | Unit::D | Unit::W
        )
    }
}

/// A string literal, decoded into literal text and interpolated expressions.
#[derive(Debug, Clone, PartialEq)]
pub struct StrLit {
    /// The parts, in source order.
    pub parts: Vec<StrPart>,
    /// Whether the literal was written in raw (single-quoted) form.
    pub raw: bool,
    /// The source range the literal covers, including its quotes.
    pub span: Span,
}

impl StrLit {
    /// The decoded text, if the literal interpolates nothing.
    #[must_use]
    pub fn literal_text(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [] => Some(""),
            [StrPart::Text { text, .. }] => Some(text),
            _ => None,
        }
    }
}

/// One part of a string literal.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    /// Literal text, with escapes already decoded.
    Text {
        /// The decoded text.
        text: String,
        /// The source range the text was decoded from.
        span: Span,
    },
    /// An interpolated `$name`, `$name.field` or `$( pipeline )`.
    Expr(Expr),
}

/// An IP address literal, kept as written so the value model parses it (spec §10.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpLit {
    /// The address exactly as it was written, including any zone identifier.
    pub text: String,
    /// The source range the literal covers.
    pub span: Span,
}

/// A timestamp literal in the RFC 3339 spelling (ADR-0071).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampLit {
    /// The literal as written; the value model reads it.
    pub text: String,
    /// The source range the literal covers.
    pub span: Span,
}

/// A regex literal, `/pattern/flags`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexLit {
    /// The pattern between the delimiters, verbatim.
    pub pattern: String,
    /// The flags after the closing delimiter, verbatim.
    pub flags: String,
    /// The source range the literal covers.
    pub span: Span,
}

/// A variable reference, `$name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variable {
    /// The name, without the `$`.
    pub name: String,
    /// The source range the reference covers.
    pub span: Span,
}

/// Which current value a `@` token names (spec §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurrentSelector {
    /// `@` — the item bound by the enclosing block, or the interactive selection.
    Current,
    /// `@-1` — the nth previous pipeline result.
    Previous(u32),
    /// `@3` — item n of the current result.
    Item(u32),
}

/// A current-value reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurrentValue {
    /// Which value is meant.
    pub selector: CurrentSelector,
    /// The source range the reference covers.
    pub span: Span,
}

/// One step of a field path, such as `cpu` in `where cpu > 20`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPath {
    /// The field name.
    pub name: String,
    /// The source range the name covers.
    pub span: Span,
}

/// A list literal.
#[derive(Debug, Clone, PartialEq)]
pub struct ListExpr {
    /// The items, in source order.
    pub items: Vec<Expr>,
    /// The source range the literal covers, including its brackets.
    pub span: Span,
}

/// A record literal.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordExpr {
    /// The fields, in source order.
    pub fields: Vec<RecordField>,
    /// The source range the literal covers, including its braces.
    pub span: Span,
}

/// One `key: value` pair of a record literal.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordField {
    /// The key.
    pub key: RecordKey,
    /// The value.
    pub value: Expr,
    /// The source range the field covers.
    pub span: Span,
}

/// The key of a record field: a bare identifier or a string literal.
#[derive(Debug, Clone, PartialEq)]
pub enum RecordKey {
    /// A bare identifier key.
    Ident {
        /// The identifier.
        name: String,
        /// The source range it covers.
        span: Span,
    },
    /// A quoted key, for names an identifier cannot spell.
    Str(StrLit),
}

impl RecordKey {
    /// The source range the key covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            RecordKey::Ident { span, .. } => *span,
            RecordKey::Str(literal) => literal.span,
        }
    }

    /// The key's name, unless it is a string literal that interpolates.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        match self {
            RecordKey::Ident { name, .. } => Some(name),
            RecordKey::Str(literal) => literal.literal_text(),
        }
    }
}

/// A block of statements, `{ … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    /// The statements, in source order.
    pub statements: Vec<Statement>,
    /// The source range the block covers, including its braces.
    pub span: Span,
}

/// A parenthesised pipeline or expression used as a value (ADR-0009).
#[derive(Debug, Clone, PartialEq)]
pub struct ParenValue {
    /// What stands between the parentheses.
    pub inner: ParenInner,
    /// The source range the construct covers, including its parentheses.
    pub span: Span,
}

impl ParenValue {
    /// The nested pipeline, if the parentheses hold one.
    #[must_use]
    pub fn pipeline(&self) -> Option<&Pipeline> {
        match &self.inner {
            ParenInner::Pipeline(pipeline) => Some(pipeline),
            ParenInner::Expr(_) => None,
        }
    }

    /// The nested expression, if the parentheses hold one.
    #[must_use]
    pub fn expression(&self) -> Option<&Expr> {
        match &self.inner {
            ParenInner::Expr(expression) => Some(expression),
            ParenInner::Pipeline(_) => None,
        }
    }
}

/// What a `( … )` value holds.
#[derive(Debug, Clone, PartialEq)]
pub enum ParenInner {
    /// A nested pipeline whose result is the value.
    Pipeline(Pipeline),
    /// A grouped expression.
    Expr(Expr),
}

/// The prefix operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `not` — logical negation.
    Not,
    /// `-` — arithmetic negation.
    Neg,
}

/// A prefix operator applied to an operand.
#[derive(Debug, Clone, PartialEq)]
pub struct UnaryExpr {
    /// The operator.
    pub op: UnaryOp,
    /// The span of the operator itself.
    pub op_span: Span,
    /// The operand.
    pub operand: Expr,
    /// The source range the whole expression covers.
    pub span: Span,
}

/// The infix operators, in the precedence order of `docs/contracts/grammar.ebnf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `or` — logical disjunction, the loosest binding operator.
    Or,
    /// `and` — logical conjunction.
    And,
    /// `==`.
    Eq,
    /// `!=`.
    NotEq,
    /// `<`.
    Lt,
    /// `<=`.
    LtEq,
    /// `>`.
    Gt,
    /// `>=`.
    GtEq,
    /// `in` — list or range membership.
    In,
    /// `not in`.
    NotIn,
    /// `~=` — regex match.
    Match,
    /// `!~=` — regex non-match.
    NotMatch,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%` — remainder.
    Rem,
}

/// An infix operator applied to two operands.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    /// The operator.
    pub op: BinaryOp,
    /// The span of the operator itself.
    pub op_span: Span,
    /// The left operand.
    pub lhs: Expr,
    /// The right operand.
    pub rhs: Expr,
    /// The source range the whole expression covers.
    pub span: Span,
}

/// Field access, `base.field` or `base?.field`.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldAccess {
    /// The value the field is read from.
    pub base: Expr,
    /// The field name.
    pub field: String,
    /// The span of the field name.
    pub field_span: Span,
    /// Whether the access short-circuits on null (`?.`).
    pub optional: bool,
    /// The source range the whole access covers.
    pub span: Span,
}

/// Index access, `base[index]`.
#[derive(Debug, Clone, PartialEq)]
pub struct IndexExpr {
    /// The value being indexed.
    pub base: Expr,
    /// The index expression.
    pub index: Expr,
    /// The source range the whole access covers.
    pub span: Span,
}

/// A call, `callee(a, b)`.
#[derive(Debug, Clone, PartialEq)]
pub struct CallExpr {
    /// What is being called.
    pub callee: Expr,
    /// The arguments, in source order.
    pub arguments: Vec<Expr>,
    /// The source range the whole call covers.
    pub span: Span,
}

/// A type annotation, such as `Stream<Process>` or `Int?`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeRef {
    /// The type name.
    pub name: String,
    /// The type arguments between `<` and `>`, if any.
    pub arguments: Vec<TypeRef>,
    /// Whether the type was marked nullable with a trailing `?`.
    pub optional: bool,
    /// The source range the annotation covers.
    pub span: Span,
}

/// `let name = pipeline`.
#[derive(Debug, Clone, PartialEq)]
pub struct LetStmt {
    /// The bound name.
    pub name: String,
    /// The span of the name.
    pub name_span: Span,
    /// The declared type, if one was written.
    pub ty: Option<TypeRef>,
    /// The pipeline whose value is bound.
    pub value: Pipeline,
    /// The source range the statement covers.
    pub span: Span,
}

/// `alias name = pipeline` (ADR-0070).
#[derive(Debug, Clone, PartialEq)]
pub struct AliasStmt {
    /// The alias name.
    pub name: String,
    /// The span of the name.
    pub name_span: Span,
    /// The pipeline the alias stands for. Its span is the text an expansion substitutes.
    pub value: Pipeline,
    /// The source range the statement covers.
    pub span: Span,
}

/// `fn name(params) -> Type { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct FnDecl {
    /// The function name.
    pub name: String,
    /// The span of the name.
    pub name_span: Span,
    /// The parameters, in source order.
    pub parameters: Vec<Param>,
    /// The declared return type, if one was written.
    pub return_type: Option<TypeRef>,
    /// The body.
    pub body: Block,
    /// The source range the declaration covers.
    pub span: Span,
}

/// One parameter of a function declaration.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    /// The parameter name.
    pub name: String,
    /// The span of the name.
    pub name_span: Span,
    /// The declared type, if one was written.
    pub ty: Option<TypeRef>,
    /// The default value, if one was written.
    pub default: Option<Expr>,
    /// The source range the parameter covers.
    pub span: Span,
}

/// `if … { … } else if … { … } else { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct IfStmt {
    /// The `if` and `else if` branches, in source order; never empty.
    pub branches: Vec<IfBranch>,
    /// The final `else` block, if one was written.
    pub else_block: Option<Block>,
    /// The source range the statement covers.
    pub span: Span,
}

/// One conditional branch of an `if` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct IfBranch {
    /// The condition that guards the block.
    pub condition: Expr,
    /// The block to run when the condition holds.
    pub block: Block,
    /// The source range the branch covers.
    pub span: Span,
}

/// `for name in expr { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct ForStmt {
    /// The name bound to each item.
    pub binding: String,
    /// The span of the binding name.
    pub binding_span: Span,
    /// The expression producing the items.
    pub iterable: Expr,
    /// The loop body.
    pub body: Block,
    /// The source range the statement covers.
    pub span: Span,
}

/// `while expr { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct WhileStmt {
    /// The condition checked before each iteration.
    pub condition: Expr,
    /// The loop body.
    pub body: Block,
    /// The source range the statement covers.
    pub span: Span,
}

/// `match expr { pattern => … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchStmt {
    /// The value being matched.
    pub subject: Expr,
    /// The arms, in source order.
    pub arms: Vec<MatchArm>,
    /// The source range the statement covers.
    pub span: Span,
}

/// One arm of a `match`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// The pattern the subject is tested against.
    pub pattern: Pattern,
    /// What runs when the pattern matches.
    pub body: MatchArmBody,
    /// The source range the arm covers.
    pub span: Span,
}

/// What a match arm runs.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchArmBody {
    /// A block of statements.
    Block(Block),
    /// A single expression.
    Expr(Expr),
}

/// A match pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// A literal the subject must equal.
    Literal(Expr),
    /// A name the subject is bound to, or a type name to test against.
    Binding {
        /// The name.
        name: String,
        /// The span of the name.
        span: Span,
    },
    /// `_` — matches anything.
    Wildcard(Span),
    /// A pattern the parser could not read; a diagnostic describes why.
    Error(Span),
}

impl Pattern {
    /// The source range the pattern covers.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Pattern::Literal(expression) => expression.span(),
            Pattern::Binding { span, .. } | Pattern::Wildcard(span) | Pattern::Error(span) => *span,
        }
    }
}

/// `try { … } catch name { … }`.
#[derive(Debug, Clone, PartialEq)]
pub struct TryStmt {
    /// The guarded block.
    pub body: Block,
    /// The handler, if one was written.
    pub catch: Option<CatchClause>,
    /// The source range the statement covers.
    pub span: Span,
}

/// The `catch` half of a `try` statement.
#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    /// The name bound to the caught error, if one was written.
    pub binding: Option<String>,
    /// The span of the binding name, if one was written.
    pub binding_span: Option<Span>,
    /// The handler block.
    pub body: Block,
    /// The source range the clause covers.
    pub span: Span,
}

/// `return expr?`.
#[derive(Debug, Clone, PartialEq)]
pub struct ReturnStmt {
    /// The returned expression, if one was written.
    pub value: Option<Expr>,
    /// The source range the statement covers.
    pub span: Span,
}

/// `use module`.
#[derive(Debug, Clone, PartialEq)]
pub struct UseStmt {
    /// The imported module name.
    pub module: QualifiedName,
    /// The source range the statement covers.
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_keep_the_expression_head_table_sorted_so_lookup_stays_correct() {
        let mut sorted = EXPRESSION_HEADS.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, EXPRESSION_HEADS);
    }
}
