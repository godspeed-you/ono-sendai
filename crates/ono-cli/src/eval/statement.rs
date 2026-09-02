//! One statement, and the forms that decide what a statement means before it runs.
//!
//! Prefix assignments, alias expansion and the `kill %N` special case all rewrite or redirect a
//! statement; the rest of the statement forms are in `control`, `block` and `pipeline`.

use std::ffi::OsString;

use ono_core::{ErrorCode, ExitStatus, Span};
use ono_parser::{Argument, Stage, StageHead, StageList, Statement};
use ono_value::{ErrorValue, Value};

use crate::expand;
use crate::session::{Alias, Definition, Function, Session};

use super::control::{run_for, run_if, run_match, run_try, run_while};
use super::expression::{eval_expr, text_of};
use super::materialize::binding_value;
use super::pipeline::run_pipeline;
use super::{Eval, Flow};

/// Runs one statement.
pub fn run_statement(
    session: &mut Session,
    statement: &Statement,
    source: &str,
) -> Eval<ExitStatus> {
    match statement {
        Statement::Pipeline(pipeline) => run_pipeline(session, pipeline, source),
        Statement::Let(binding) => {
            let (value, status) = binding_value(session, &binding.value, source)?;
            session.assign(binding.name.clone(), value);
            Ok(status)
        }
        Statement::If(branch) => run_if(session, branch, source),
        Statement::While(loop_) => run_while(session, loop_, source),
        Statement::For(loop_) => run_for(session, loop_, source),
        Statement::Match(match_) => run_match(session, match_, source),
        Statement::Try(try_) => run_try(session, try_, source),
        Statement::Fn(declaration) => {
            // A function lives in the scope chain beside the `let` bindings, so a call finds the
            // innermost scope that defines one (ADR-0011 step 2, ADR-0070).
            session.define(
                declaration.name.clone(),
                Definition::Function(std::sync::Arc::new(Function {
                    declaration: declaration.clone(),
                    source: source.into(),
                })),
            );
            Ok(ExitStatus::SUCCESS)
        }
        Statement::Alias(alias) => {
            // An alias is its text: expansion re-parses it in place of the head word, so the
            // pipeline is kept as written rather than as a tree (ADR-0070).
            session.define(
                alias.name.clone(),
                Definition::Alias(std::sync::Arc::new(Alias {
                    expansion: alias.value.span.of(source).to_owned(),
                })),
            );
            Ok(ExitStatus::SUCCESS)
        }
        Statement::Return(jump) => {
            let value = match &jump.value {
                Some(expression) => eval_expr(session, expression, source)?,
                None => Value::Null,
            };
            Err(Flow::Return(value))
        }
        Statement::Break(_) => Err(Flow::Break),
        Statement::Continue(_) => Err(Flow::Continue),
        Statement::Use(_) => Ok(ExitStatus::SUCCESS),
        Statement::Error(span) => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("this statement could not be read at {span}"),
        ))),
    }
}

/// Whether a stage is `kill %N …`: the bare `kill` with a job specifier as its first word.
pub(super) fn is_job_kill(stage: &Stage) -> bool {
    let StageHead::Command(name) = &stage.head else {
        return false;
    };
    name.namespace.is_none()
        && name.name == "kill"
        && matches!(
            stage.arguments.first(),
            Some(Argument::Word(word)) if word.text.starts_with('%')
        )
}

/// The `NAME=value` words that lead a stage of `list`, with the list as it reads without them.
///
/// `None` when no stage starts with an assignment. A stage that is nothing but assignments is
/// refused: a lasting binding has two explicit spellings already.
pub(super) fn prefix_assignments(
    session: &mut Session,
    list: &StageList,
    source: &str,
) -> Eval<Option<(Vec<(String, OsString)>, StageList)>> {
    let Some((index, stage)) = list
        .stages
        .iter()
        .enumerate()
        .find(|(_, stage)| stage.head.name().is_some_and(is_assignment_word))
    else {
        return Ok(None);
    };
    let StageHead::Command(head) = &stage.head else {
        return Ok(None);
    };
    if head.namespace.is_some() {
        return Ok(None);
    }

    let mut assignments = Vec::new();
    let mut arguments = stage.arguments.iter().peekable();
    let mut pending: Option<(String, Span)> = Some((head.name.clone(), head.span));
    loop {
        let Some((word, span)) = pending.take() else {
            break;
        };
        let (name, value) = word.split_once('=').unwrap_or((&word, ""));
        let value = if value.is_empty()
            && let Some(Argument::Value(expression)) = arguments.peek()
            && expression.span().start() == span.end()
        {
            // `NAME="a b"`: the lexer ends the word at the quote, so the string that follows
            // without a gap is the value.
            let expression = expression.clone();
            arguments.next();
            OsString::from(text_of(&eval_expr(session, &expression, source)?)?)
        } else {
            expand::expand_to_one(session, value)?
        };
        assignments.push((name.to_owned(), value));
        if let Some(Argument::Word(next)) = arguments.peek()
            && is_assignment_word(&next.text)
        {
            pending = Some((next.text.clone(), next.span));
            arguments.next();
        }
    }

    let Some(Argument::Word(command)) = arguments.next() else {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!(
                    "`{}` names no command to run with that environment",
                    head.name
                ),
            )
            .with_help(
                "a prefix assignment is scoped to the command after it (spec §54); for a lasting \
                 binding write `set env NAME = value` or `let name = value`",
            ),
        ));
    };
    let (namespace, name) = match command.text.split_once(':') {
        Some((namespace, name))
            if !namespace.is_empty()
                && !name.is_empty()
                && !name.contains(['/', ':'])
                && namespace
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') =>
        {
            (Some(namespace.to_owned()), name.to_owned())
        }
        _ => (None, command.text.clone()),
    };
    let mut rewritten = stage.clone();
    rewritten.head = StageHead::Command(ono_parser::QualifiedName {
        namespace,
        name,
        span: command.span,
    });
    rewritten.arguments = arguments.cloned().collect();
    let mut stripped = list.clone();
    stripped.stages[index] = rewritten;
    Ok(Some((assignments, stripped)))
}

/// Whether a word spells `NAME=…` with an environment variable's name before the `=`.
pub(super) fn is_assignment_word(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// The text `list` becomes when its head word is an alias: the alias's expansion, then the rest
/// of the list exactly as written. `None` when the head is not an alias, or is the alias being
/// expanded right now.
#[must_use]
pub fn expand_alias(session: &Session, list: &StageList, source: &str) -> Option<(String, String)> {
    let stage = list.stages.first()?;
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if name.namespace.is_some() {
        return None;
    }
    let alias = session.alias(&name.name)?;
    let rest = source
        .get(name.span.end() as usize..list.span.end() as usize)
        .unwrap_or_default();
    Some((name.name.clone(), format!("{}{rest}", alias.expansion)))
}
