//! Completion candidates from metadata (spec §15.1).
//!
//! Spec §34 budgets 50 ms for the first results, so everything here is a lookup in the registry
//! and nothing is a search. What the registry cannot know — the users on this machine, the
//! services of this host — is not guessed at: [`ValueCompleter`] is the hook the caller fills in,
//! and without it the candidate list is honestly empty rather than plausibly wrong.
//!
//! Completion first decides what can stand at the cursor and only then asks whoever knows it
//! (v0.6.1 §10). In words mode the words decide: a verb, a target, an option, a selector's value.
//! In an expression-mode argument the canonical lexer's tokens decide, read as the grammar reads
//! them — a field, then an operator its declared type supports, then a comparand, then `and` or
//! `or` — so `size>5G` is read the same way `size > 5G` is (ADR-0860). Whether a filesystem path
//! can stand at the cursor is part of that same decision, [`accepts_path`], and never the fallback
//! for an empty answer.

use std::sync::Arc;

use ono_parser::{ArgMode, BinaryOp, TokenKind};
use ono_value::{FieldType, Schema};

use crate::contract::{ArgumentMode, CommandContract, DeclaredType, ParameterSpec};
use crate::operators::{comparisons_for, operator_doc, operator_symbol, units_for};
use crate::registry::CommandRegistry;

/// What kind of thing a candidate is, so the editor can present it accordingly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CandidateKind {
    /// A verb in command position.
    Verb,
    /// A target word after a verb.
    Target,
    /// A `--named` option.
    Option,
    /// A value for a selector or an option.
    Value,
    /// A field of the schema flowing into the stage (spec §15.1).
    Field,
    /// An operator or a connector inside an expression (v0.6.1 §13, §15).
    Operator,
}

/// One completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    text: String,
    kind: CandidateKind,
    doc: Option<String>,
}

impl Candidate {
    /// A candidate of an explicit kind.
    #[must_use]
    pub fn new(text: impl Into<String>, kind: CandidateKind) -> Self {
        Self {
            text: text.into(),
            kind,
            doc: None,
        }
    }

    /// A verb candidate.
    #[must_use]
    pub fn verb(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Verb)
    }

    /// A target candidate.
    #[must_use]
    pub fn target(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Target)
    }

    /// An option candidate, written as it would be typed.
    #[must_use]
    pub fn option(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Option)
    }

    /// A value candidate.
    #[must_use]
    pub fn value(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Value)
    }

    /// A field candidate.
    #[must_use]
    pub fn field(text: impl Into<String>) -> Self {
        Self::new(text, CandidateKind::Field)
    }

    /// Attaches the one line the editor shows beside the candidate.
    #[must_use]
    pub fn with_doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = Some(doc.into());
        self
    }

    /// The text that replaces the token being typed.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// What kind of thing it is.
    #[must_use]
    pub fn kind(&self) -> CandidateKind {
        self.kind
    }

    /// The line shown beside it, where the registry documents one.
    #[must_use]
    pub fn doc(&self) -> Option<&str> {
        self.doc.as_deref()
    }

    fn documented(self, doc: Option<&str>) -> Self {
        match doc {
            Some(doc) => self.with_doc(doc),
            None => self,
        }
    }
}

/// The stage the cursor is in: what has been typed so far, and what is being typed now.
#[derive(Debug, Clone, Default)]
pub struct StageContext {
    head: Option<String>,
    words: Vec<String>,
    prefix: String,
    /// The pipeline text before the `|` that opened this stage, when there is one: what
    /// decides which schema's fields `where` and `select` complete (spec §15.1, ADR-0074).
    upstream: Option<String>,
    /// The tokens of an expression-mode argument typed before the one under the cursor, as the
    /// canonical lexer reads them. `None` in words mode, and inside an open block, where a
    /// statement of its own begins.
    expression: Option<Vec<Lexeme>>,
    /// The schema flowing into the stage, where the caller has planned the pipeline itself.
    schema: Option<Arc<Schema>>,
}

impl PartialEq for StageContext {
    fn eq(&self, other: &Self) -> bool {
        self.head == other.head
            && self.words == other.words
            && self.prefix == other.prefix
            && self.upstream == other.upstream
            && self.expression == other.expression
            && match (&self.schema, &other.schema) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

impl Eq for StageContext {}

/// One token of an expression-mode argument: its class and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Lexeme {
    kind: TokenKind,
    text: String,
}

impl StageContext {
    /// A context assembled from parts, for a caller that already has a parsed stage.
    #[must_use]
    pub fn new(head: Option<&str>, words: &[&str], prefix: &str) -> Self {
        Self {
            head: head.map(str::to_owned),
            words: words.iter().map(|word| (*word).to_owned()).collect(),
            prefix: prefix.to_owned(),
            upstream: None,
            expression: None,
            schema: None,
        }
    }

    /// The context at `cursor` in `line`.
    ///
    /// Only the stage the cursor is in matters, so everything up to the last `|`, `;`, `&&` or
    /// `||` is discarded. The token under the cursor is the prefix; everything before it is
    /// already typed. In an expression-mode stage the token is the lexer's, not the word's:
    /// in `where size>5G` it is `5G`.
    #[must_use]
    pub fn from_line(line: &str, cursor: usize) -> Self {
        let typed = &line[..cursor.min(line.len())];
        let cut = typed.rfind(['|', ';', '&']).map_or(0, |index| index + 1);
        let stage = &typed[cut..];
        // Only a pipe hands a schema on; a `;` or an `&&` starts the stage from nothing.
        let upstream = cut
            .checked_sub(1)
            .filter(|index| typed.as_bytes().get(*index) == Some(&b'|'))
            .map(|index| typed[..index].to_owned());

        let mut tokens: Vec<String> = stage.split_whitespace().map(str::to_owned).collect();
        let prefix = if stage.ends_with(char::is_whitespace) || stage.is_empty() {
            String::new()
        } else {
            tokens.pop().unwrap_or_default()
        };
        let head = if tokens.is_empty() {
            None
        } else {
            Some(tokens.remove(0))
        };
        let expression = head
            .as_deref()
            .filter(|head| ArgMode::for_head(head) == ArgMode::Expression)
            .and_then(|_| expression_lexemes(stage));
        let (expression, prefix) = match expression {
            Some((lexemes, prefix)) => (Some(lexemes), prefix),
            None => (None, prefix),
        };
        Self {
            head,
            words: tokens,
            prefix,
            upstream,
            expression,
            schema: None,
        }
    }

    /// The same context, with the schema the caller planned for the pipeline before the stage.
    ///
    /// The contracts alone know what `get process |` hands on; only a caller that plans the
    /// pipeline knows what an adapted `ps aux |` does (spec v0.3 §1.59). Where it is given, it
    /// is the one completion reads.
    #[must_use]
    pub fn with_schema(mut self, schema: Option<Arc<Schema>>) -> Self {
        self.schema = schema;
        self
    }

    /// The stage's head, once it has been typed in full.
    #[must_use]
    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    /// The arguments already typed in full.
    #[must_use]
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// The token under the cursor.
    #[must_use]
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Whether the cursor is inside an expression-mode argument, where the token under it is
    /// the lexer's and may be shorter than the whitespace-separated word around it.
    #[must_use]
    pub fn in_expression(&self) -> bool {
        self.expression.is_some()
    }
}

/// The tokens of an expression-mode stage before the one under the cursor, and that one.
///
/// The head is dropped. Inside an open `{` a statement of its own begins, which this reading
/// does not follow, so there is no answer. The token under the cursor is the one that ends at
/// it, joined with what a user types in pieces: `!~` on its way to `!~=`, and `5G`, a number
/// and the start of its unit.
fn expression_lexemes(stage: &str) -> Option<(Vec<Lexeme>, String)> {
    let tokens: Vec<ono_parser::Token> = ono_parser::tokens(stage)
        .into_iter()
        .filter(|token| {
            !matches!(
                token.kind,
                TokenKind::Eof | TokenKind::Comment | TokenKind::Newline
            )
        })
        .skip(1)
        .collect();
    let opened = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::LBrace)
        .count();
    let closed = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::RBrace)
        .count();
    if opened > closed {
        return None;
    }

    let end_of = |token: &ono_parser::Token| token.span.end() as usize;
    let start_of = |token: &ono_parser::Token| token.span.start() as usize;
    let mut first = tokens.len();
    if let Some(last) = tokens.last()
        && end_of(last) == stage.len()
        && can_be_extended(last.kind, last.text(stage))
    {
        first -= 1;
        if is_operator_piece(last.kind, last.text(stage)) {
            while first > 0
                && is_operator_piece(tokens[first - 1].kind, tokens[first - 1].text(stage))
                && end_of(&tokens[first - 1]) == start_of(&tokens[first])
            {
                first -= 1;
            }
        } else if last.kind == TokenKind::Ident
            && first > 0
            && matches!(tokens[first - 1].kind, TokenKind::Int | TokenKind::Float)
            && end_of(&tokens[first - 1]) == start_of(last)
        {
            first -= 1;
        }
    }
    let prefix = tokens
        .get(first)
        .map_or(String::new(), |token| stage[start_of(token)..].to_owned());
    let lexemes = tokens[..first]
        .iter()
        .map(|token| Lexeme {
            kind: token.kind,
            text: token.text(stage).to_owned(),
        })
        .collect();
    Some((lexemes, prefix))
}

/// Whether typing on could still change what a token is: a word, a number, a literal or an
/// operator. Punctuation — `[`, `(`, `.`, `,` — is finished the moment it is typed.
fn can_be_extended(kind: TokenKind, text: &str) -> bool {
    matches!(
        kind,
        TokenKind::Ident
            | TokenKind::Word
            | TokenKind::Int
            | TokenKind::Float
            | TokenKind::Unit
            | TokenKind::Str
            | TokenKind::RawStr
            | TokenKind::UnterminatedStr
            | TokenKind::UnterminatedRawStr
            | TokenKind::Regex
            | TokenKind::UnterminatedRegex
            | TokenKind::Variable
            | TokenKind::Ip
            | TokenKind::Timestamp
    ) || is_operator_piece(kind, text)
}

/// Whether a token is part of a comparison operator, whole or on its way to one.
fn is_operator_piece(kind: TokenKind, text: &str) -> bool {
    match kind {
        TokenKind::Eq
        | TokenKind::EqEq
        | TokenKind::BangEq
        | TokenKind::Lt
        | TokenKind::LtEq
        | TokenKind::Gt
        | TokenKind::GtEq
        | TokenKind::Match
        | TokenKind::NotMatch => true,
        TokenKind::Unknown => text
            .chars()
            .all(|c| matches!(c, '!' | '~' | '=' | '<' | '>')),
        _ => false,
    }
}

/// What only a provider can complete: the users on this machine, the services of this host.
///
/// The registry offers what metadata knows and stops there; this hook is where the caller adds
/// the rest. Spec §15.1 wants completion to be provider-aware, and this is the seam.
pub trait ValueCompleter {
    /// The values for `parameter` of `command` that begin with `prefix`.
    fn complete(
        &self,
        command: &CommandContract,
        parameter: &ParameterSpec,
        prefix: &str,
    ) -> Vec<Candidate>;
}

/// The candidates for the token under the cursor, sorted and without repeats.
///
/// ```
/// use ono_command::{CommandRegistry, StageContext};
///
/// let registry = CommandRegistry::embedded()?;
/// let candidates = ono_command::complete(registry, &StageContext::from_line("get pro", 7), None);
/// assert_eq!(candidates[0].text(), "process");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn complete(
    registry: &CommandRegistry,
    context: &StageContext,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let mut candidates = gather(registry, context, values);
    candidates.sort_by(|left, right| left.text.cmp(&right.text));
    candidates.dedup_by(|left, right| left.text == right.text);
    candidates
}

/// Whether a filesystem path can stand at the cursor (v0.6.1 §16).
///
/// A path is offered where the syntax admits one: an argument of a program the registry does not
/// describe, a selector whose declared type is open, a statement inside a block. It is not
/// offered in command position, in place of a verb's target, for a closed set, or anywhere in a
/// command's expression-mode argument, where a bare word is a field and a path would have to be
/// quoted. That the registry had nothing to offer is never a reason on its own (issue #133).
///
/// ```
/// use ono_command::{CommandRegistry, StageContext, accepts_path};
///
/// let registry = CommandRegistry::embedded()?;
/// let at = |line: &str| accepts_path(registry, &StageContext::from_line(line, line.len()));
/// assert!(at("get file sr"));
/// assert!(!at("get process | where cpu > "));
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn accepts_path(registry: &CommandRegistry, context: &StageContext) -> bool {
    let Some(head) = context.head() else {
        return false;
    };
    if context.prefix().starts_with('-') {
        return false;
    }
    let command = resolve(registry, head, context.words());
    if ArgMode::for_head(head) == ArgMode::Expression {
        // Inside a block a statement of its own begins, and a keyword such as `let` introduces a
        // pipeline; only a command's own expression argument is known to hold no path.
        return !context.in_expression() || command.is_none();
    }
    if context.words().is_empty() && !registry.targets_for_verb(head).is_empty() {
        return false;
    }
    let Some(command) = command else {
        return true;
    };
    if command.id() == "ono.meta.help" {
        return false;
    }
    next_selector(command, context)
        .is_none_or(|selector| selector.declared_type().closed_set().is_none())
}

fn gather(
    registry: &CommandRegistry,
    context: &StageContext,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let Some(head) = context.head() else {
        return registry
            .verbs()
            .iter()
            .filter(|verb| verb.verb().starts_with(context.prefix()))
            .map(|verb| Candidate::verb(verb.verb()).with_doc(verb.semantics()))
            .collect();
    };

    let command = resolve(registry, head, context.words());

    if let Some(rest) = context.prefix().strip_prefix("--") {
        let Some(command) = command else {
            return Vec::new();
        };
        return match rest.split_once('=') {
            Some((name, written)) => option_values(command, name, written, values),
            None => command
                .options()
                .iter()
                .filter(|option| option.name().starts_with(rest))
                .map(|option| {
                    Candidate::option(format!("--{}", option.name())).with_doc(option.doc())
                })
                .collect(),
        };
    }

    // A target word is still expected while nothing but the verb has been typed.
    if context.words().is_empty() {
        let targets = registry.targets_for_verb(head);
        if !targets.is_empty() {
            return targets
                .into_iter()
                .filter(|target| target.starts_with(context.prefix()))
                .map(|target| {
                    let doc = registry.target(target).map(|entry| entry.summary());
                    Candidate::target(target).documented(doc)
                })
                .collect();
        }
    }

    let Some(command) = command else {
        return Vec::new();
    };
    // A help topic is a vocabulary of `help` alone: the browsing pages, every verb, every target
    // and every command spelling. No contract can carry it, because it is the set of things the
    // help system itself answers for (spec §15.1, v0.4 §38.2).
    if command.id() == "ono.meta.help" {
        return help_topics(registry, context.prefix());
    }
    if command.argument_mode() == ArgumentMode::Expression {
        return match &context.expression {
            Some(lexemes) => expression_candidates(registry, command, context, lexemes),
            None => Vec::new(),
        };
    }
    match next_selector(command, context) {
        Some(selector) => selector_values(command, selector, context.prefix(), values),
        None => Vec::new(),
    }
}

/// Whether an argument bound to `selector` reads fields of the stream: an expression-mode
/// parameter that carries values. A string parameter — `sort cpu desc`'s direction — is
/// vocabulary, exactly as the pre-flight check treats it.
fn reads_fields(command: &CommandContract, selector: &ParameterSpec) -> bool {
    command.argument_mode() == ArgumentMode::Expression
        && selector.declared_type() != &DeclaredType::String
}

/// The schema flowing into the stage under the cursor, where the pipeline before it declares one.
fn upstream_schema(registry: &CommandRegistry, context: &StageContext) -> Option<Arc<Schema>> {
    if let Some(schema) = &context.schema {
        return Some(Arc::clone(schema));
    }
    let upstream = context.upstream.as_deref()?;
    let parsed = ono_parser::parse(upstream);
    let pipeline = parsed.program().statements.last()?.as_pipeline()?;
    let schemas: Vec<Arc<Schema>> = ono_value::builtin_schemas().schemas().cloned().collect();
    crate::check::schema_after(registry, &schemas, pipeline)
}

/// The command a head and the words typed after it name, if the registry has one.
fn resolve<'a>(
    registry: &'a CommandRegistry,
    head: &str,
    words: &[String],
) -> Option<&'a CommandContract> {
    words
        .first()
        .and_then(|target| registry.find(head, Some(target)))
        .or_else(|| registry.find(head, None))
}

/// The selector the next positional word would bind to.
fn next_selector<'a>(
    command: &'a CommandContract,
    context: &StageContext,
) -> Option<&'a ParameterSpec> {
    let target_words = usize::from(command.target().is_some());
    let mut positional = context
        .words()
        .iter()
        .filter(|word| !word.starts_with("--"))
        .count();
    positional = positional.saturating_sub(target_words);
    command.selectors().get(positional).or_else(|| {
        command
            .selectors()
            .last()
            .filter(|last| last.is_repeatable())
    })
}

fn selector_values(
    command: &CommandContract,
    selector: &ParameterSpec,
    prefix: &str,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let closed = selector.closed_set();
    if !closed.is_empty() {
        return closed
            .into_iter()
            .filter(|value| value.starts_with(prefix))
            .map(|value| Candidate::value(value).with_doc(selector.doc()))
            .collect();
    }
    values.map_or_else(Vec::new, |hook| hook.complete(command, selector, prefix))
}

fn option_values(
    command: &CommandContract,
    name: &str,
    written: &str,
    values: Option<&dyn ValueCompleter>,
) -> Vec<Candidate> {
    let Some(option) = command.option(name) else {
        return Vec::new();
    };
    let closed = option.closed_set();
    let offered = if closed.is_empty() {
        values.map_or_else(Vec::new, |hook| hook.complete(command, option, written))
    } else {
        closed
            .into_iter()
            .filter(|value| value.starts_with(written))
            .map(|value| Candidate::value(value).with_doc(option.doc()))
            .collect()
    };
    offered
        .into_iter()
        .map(|candidate| Candidate {
            text: format!("--{name}={}", candidate.text),
            kind: CandidateKind::Value,
            doc: candidate.doc,
        })
        .collect()
}

/// Every topic `help` answers for, narrowed to `prefix` (spec §15.1).
fn help_topics(registry: &CommandRegistry, prefix: &str) -> Vec<Candidate> {
    let browsing = crate::help::topics()
        .iter()
        .map(|(name, doc)| Candidate::value(*name).with_doc(*doc));
    let verbs = registry
        .verbs()
        .iter()
        .map(|verb| Candidate::verb(verb.verb()).with_doc(verb.semantics()));
    let commands = registry
        .commands()
        .iter()
        .map(|command| Candidate::value(command.spelling()).with_doc(command.summary()));
    browsing
        .chain(verbs)
        .chain(commands)
        .filter(|candidate| candidate.text().starts_with(prefix))
        .collect()
}

// --- expression-mode arguments (issues #133–#135, ADR-0860) ------------------------------------

/// What the first selector of an expression-mode command reads, which decides what may follow a
/// complete operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavor {
    /// A predicate, `where`'s: comparisons joined by `and` and `or`.
    Predicate,
    /// Field paths one after another, `select`'s.
    Fields,
    /// One expression whose value is used, `sort`'s and `group`'s key: a field starts it.
    Key,
}

/// Where in an expression-mode argument the cursor stands.
#[derive(Debug, Clone)]
enum Site {
    /// A new operand begins: at the start, and after `and`, `or`, `not` and `(`.
    Operand,
    /// After a field path: `size ▮`.
    AfterField(FieldType),
    /// After the `.` of a field that is a record: `local.▮`.
    AfterDot(Arc<Schema>),
    /// After `not` following a field: `name not ▮`, which only `in` continues.
    AfterNot(FieldType),
    /// After `field op`: the comparand.
    Value(FieldType, BinaryOp),
    /// Inside the list after `in [`, where an element is due.
    Element(FieldType),
    /// Inside the list, after an element.
    AfterElement(FieldType),
    /// After a whole comparison, a comparand or a closed group.
    Complete,
    /// The tokens cannot be read; nothing is offered rather than a guess (v0.6.1 §20).
    Dead,
}

fn expression_candidates(
    registry: &CommandRegistry,
    command: &CommandContract,
    context: &StageContext,
    lexemes: &[Lexeme],
) -> Vec<Candidate> {
    let Some(selector) = command.selectors().first() else {
        return Vec::new();
    };
    if !reads_fields(command, selector) {
        return Vec::new();
    }
    // An open quote or regex: a string is being typed, which no metadata completes.
    let prefix = context.prefix();
    if prefix.starts_with(['"', '\'', '/']) {
        return Vec::new();
    }
    let Some(schema) = upstream_schema(registry, context) else {
        return Vec::new();
    };
    let flavor = match selector.declared_type() {
        DeclaredType::Bool => Flavor::Predicate,
        DeclaredType::List(_) => Flavor::Fields,
        _ => Flavor::Key,
    };
    let mut depth = 0_usize;
    let mut site = Site::Operand;
    for lexeme in lexemes {
        site = step(site, lexeme, &schema, flavor, &mut depth);
        if matches!(site, Site::Dead) {
            break;
        }
    }
    offer(site, &schema, flavor, prefix)
}

/// The site after `lexeme`, read the way `docs/contracts/grammar.ebnf` reads an expression.
fn step(site: Site, lexeme: &Lexeme, schema: &Schema, flavor: Flavor, depth: &mut usize) -> Site {
    let word = (lexeme.kind == TokenKind::Ident).then_some(lexeme.text.as_str());
    let close = |depth: &mut usize| {
        if lexeme.kind == TokenKind::RParen && *depth > 0 {
            *depth -= 1;
            Site::Complete
        } else {
            Site::Dead
        }
    };
    match site {
        Site::Operand => match word {
            Some("not") if flavor == Flavor::Predicate => Site::Operand,
            Some("and" | "or" | "in") => Site::Dead,
            Some(name) => field_site(schema, name),
            None if lexeme.kind == TokenKind::LParen && flavor == Flavor::Predicate => {
                *depth += 1;
                Site::Operand
            }
            None => Site::Dead,
        },
        Site::AfterField(ty) => {
            if matches!(lexeme.kind, TokenKind::Dot | TokenKind::QuestionDot) {
                return record_site(&ty);
            }
            match (flavor, word) {
                (Flavor::Fields, Some(name)) => field_site(schema, name),
                (Flavor::Predicate, Some("in")) => Site::Value(ty, BinaryOp::In),
                (Flavor::Predicate, Some("not")) => Site::AfterNot(ty),
                (Flavor::Predicate, Some("and" | "or")) if continues(&ty) => Site::Operand,
                (Flavor::Predicate, None) => match comparison(lexeme.kind) {
                    Some(op) => Site::Value(ty, op),
                    None => close(depth),
                },
                _ => Site::Dead,
            }
        }
        Site::AfterDot(record) => word.map_or(Site::Dead, |name| field_site(&record, name)),
        Site::AfterNot(ty) => match word {
            Some("in") => Site::Value(ty, BinaryOp::NotIn),
            _ => Site::Dead,
        },
        Site::Value(ty, op) => match lexeme.kind {
            TokenKind::LBracket if matches!(op, BinaryOp::In | BinaryOp::NotIn) => {
                Site::Element(ty)
            }
            TokenKind::Minus => Site::Value(ty, op),
            kind if is_comparand(kind, word, &ty, schema) => Site::Complete,
            _ => Site::Dead,
        },
        Site::Element(ty) => match lexeme.kind {
            TokenKind::RBracket => Site::Complete,
            TokenKind::Minus => Site::Element(ty),
            kind if is_comparand(kind, word, &ty, schema) => Site::AfterElement(ty),
            _ => Site::Dead,
        },
        Site::AfterElement(ty) => match lexeme.kind {
            TokenKind::Comma => Site::Element(ty),
            TokenKind::RBracket => Site::Complete,
            _ => Site::Dead,
        },
        Site::Complete => match (flavor, word) {
            (Flavor::Predicate, Some("and" | "or")) => Site::Operand,
            (_, None) => close(depth),
            _ => Site::Dead,
        },
        Site::Dead => Site::Dead,
    }
}

/// What can stand at `site`, narrowed to `prefix`.
fn offer(site: Site, schema: &Schema, flavor: Flavor, prefix: &str) -> Vec<Candidate> {
    match site {
        Site::Operand => fields(schema, prefix),
        Site::AfterField(ty) => match flavor {
            Flavor::Predicate => {
                let mut offered = operators(comparisons_for(&ty), prefix);
                if continues(&ty) {
                    offered.extend(operators(&[BinaryOp::And, BinaryOp::Or], prefix));
                }
                offered
            }
            Flavor::Fields => fields(schema, prefix),
            Flavor::Key => Vec::new(),
        },
        Site::AfterDot(record) => fields(&record, prefix),
        Site::AfterNot(_) => ["in"]
            .into_iter()
            .filter(|word| word.starts_with(prefix))
            .map(|word| {
                Candidate::new(word, CandidateKind::Operator)
                    .documented(operator_doc(BinaryOp::NotIn))
            })
            .collect(),
        Site::Value(_, BinaryOp::In | BinaryOp::NotIn) => {
            if prefix.is_empty() {
                vec![Candidate::value("[")]
            } else {
                Vec::new()
            }
        }
        Site::Value(ty, _) | Site::Element(ty) => comparands(&ty, prefix),
        Site::Complete if flavor == Flavor::Predicate => {
            operators(&[BinaryOp::And, BinaryOp::Or], prefix)
        }
        Site::AfterElement(_) | Site::Complete | Site::Dead => Vec::new(),
    }
}

/// The site after the field `name`, or nowhere when the schema has no such field.
fn field_site(schema: &Schema, name: &str) -> Site {
    schema
        .field(name)
        .map_or(Site::Dead, |field| Site::AfterField(field.ty().clone()))
}

/// The site after `record.`, where the record's own schema is known.
fn record_site(ty: &FieldType) -> Site {
    match ty {
        FieldType::Record(id) => ono_value::builtin_schemas()
            .get(id)
            .map_or(Site::Dead, Site::AfterDot),
        _ => Site::Dead,
    }
}

/// Whether a field is a truth value on its own, so `where read_only and …` continues it.
fn continues(ty: &FieldType) -> bool {
    matches!(ty, FieldType::Bool | FieldType::Any)
}

/// The comparison a token spells, if it spells one.
fn comparison(kind: TokenKind) -> Option<BinaryOp> {
    Some(match kind {
        TokenKind::EqEq => BinaryOp::Eq,
        TokenKind::BangEq => BinaryOp::NotEq,
        TokenKind::Lt => BinaryOp::Lt,
        TokenKind::LtEq => BinaryOp::LtEq,
        TokenKind::Gt => BinaryOp::Gt,
        TokenKind::GtEq => BinaryOp::GtEq,
        TokenKind::Match => BinaryOp::Match,
        TokenKind::NotMatch => BinaryOp::NotMatch,
        _ => return None,
    })
}

/// Whether a token is a whole comparand: a literal, a variable, one of the field's enum
/// variants written bare (ADR-0096), or another field.
fn is_comparand(kind: TokenKind, word: Option<&str>, ty: &FieldType, schema: &Schema) -> bool {
    match word {
        Some("true" | "false" | "null") => true,
        Some(word) => {
            schema.field(word).is_some()
                || matches!(ty, FieldType::Enum(variants) if variants.iter().any(|variant| &**variant == word))
        }
        None => matches!(
            kind,
            TokenKind::Int
                | TokenKind::Float
                | TokenKind::Unit
                | TokenKind::Str
                | TokenKind::RawStr
                | TokenKind::Ip
                | TokenKind::Timestamp
                | TokenKind::Regex
                | TokenKind::Variable
                | TokenKind::CurrentValue
        ),
    }
}

/// The fields of `schema` that begin with `prefix`, each with its doc.
fn fields(schema: &Schema, prefix: &str) -> Vec<Candidate> {
    schema
        .fields()
        .iter()
        .filter(|field| field.name().starts_with(prefix))
        .map(|field| Candidate::field(field.name()).documented(field.doc()))
        .collect()
}

/// The operators of `ops` that begin with `prefix`, each with the doc the language gives it.
fn operators(ops: &[BinaryOp], prefix: &str) -> Vec<Candidate> {
    ops.iter()
        .filter(|op| operator_symbol(**op).starts_with(prefix))
        .map(|op| {
            Candidate::new(operator_symbol(*op), CandidateKind::Operator)
                .documented(operator_doc(*op))
        })
        .collect()
}

/// What a field of type `ty` can be compared with, where that is a closed set or a unit.
///
/// A closed set is matched without regard to case and inserted as declared, so `ESTA` finds
/// `established`. A quantity needs its number typed first: after `5` come the units of the
/// field's dimension. An open domain — text, addresses — offers nothing, because anything more
/// would be a provider's answer (issue #135).
fn comparands(ty: &FieldType, prefix: &str) -> Vec<Candidate> {
    let typed = prefix.to_lowercase();
    let words = |words: &mut dyn Iterator<Item = &str>| -> Vec<Candidate> {
        words
            .filter(|word| word.to_lowercase().starts_with(&typed))
            .map(Candidate::value)
            .collect()
    };
    match ty {
        FieldType::Bool => words(&mut ["false", "true"].into_iter()),
        FieldType::Enum(variants) => words(&mut variants.iter().map(|variant| &**variant)),
        FieldType::ByteSize | FieldType::Duration | FieldType::Percent => {
            let number_end = prefix
                .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '_'))
                .unwrap_or(prefix.len());
            let (number, unit) = prefix.split_at(number_end);
            if !number.starts_with(|c: char| c.is_ascii_digit()) {
                return Vec::new();
            }
            let unit = unit.to_lowercase();
            units_for(ty)
                .into_iter()
                .filter(|(suffix, _)| suffix.to_lowercase().starts_with(&unit))
                .map(|(suffix, doc)| Candidate::value(format!("{number}{suffix}")).with_doc(doc))
                .collect()
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::StageContext;

    #[test]
    fn should_read_the_stage_under_the_cursor() {
        let context = StageContext::from_line("get process | where cp", 22);
        assert_eq!(context.head(), Some("where"));
        assert_eq!(context.prefix(), "cp");
        assert!(context.words().is_empty());
    }

    #[test]
    fn should_treat_a_trailing_space_as_a_finished_word() {
        let context = StageContext::from_line("get process ", 12);
        assert_eq!(context.head(), Some("get"));
        assert_eq!(context.words(), ["process"]);
        assert_eq!(context.prefix(), "");
    }

    #[test]
    fn should_read_the_token_under_the_cursor_in_an_expression() {
        // `size>5G` is one word and three tokens; the one being typed is the number and the
        // start of its unit.
        let line = "get filesystem | where size>5G";
        assert_eq!(StageContext::from_line(line, line.len()).prefix(), "5G");
        let line = "get process | where name !~";
        assert_eq!(StageContext::from_line(line, line.len()).prefix(), "!~");
    }
}
