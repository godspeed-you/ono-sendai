//! The control constructs: `if`, `while`, `for`, `match` and `try`.
//!
//! Each of them reads [`Flow`](super::Flow) rather than a status code, which is what keeps
//! `break`, `continue`, `return` and `exit` explicit (v0.4.1 §30.3).

use ono_core::{ErrorCode, ExitStatus};
use ono_value::{ErrorValue, Value};

use crate::session::Session;

use super::block::run_block;
use super::expression::{equals, eval_expr, truthy};
use super::pipeline::is_type_name;
use super::{Eval, Flow};

pub(super) fn run_if(
    session: &mut Session,
    branch: &ono_parser::IfStmt,
    source: &str,
) -> Eval<ExitStatus> {
    for arm in &branch.branches {
        if truthy(&eval_expr(session, &arm.condition, source)?) {
            return run_block(session, &arm.block, source);
        }
    }
    match &branch.else_block {
        Some(body) => run_block(session, body, source),
        None => Ok(ExitStatus::SUCCESS),
    }
}

pub(super) fn run_while(
    session: &mut Session,
    loop_: &ono_parser::WhileStmt,
    source: &str,
) -> Eval<ExitStatus> {
    let mut status = ExitStatus::SUCCESS;
    while truthy(&eval_expr(session, &loop_.condition, source)?) {
        match run_block(session, &loop_.body, source) {
            Ok(reached) => status = reached,
            Err(Flow::Break) => break,
            Err(Flow::Continue) => continue,
            Err(other) => return Err(other),
        }
    }
    Ok(status)
}

pub(super) fn run_for(
    session: &mut Session,
    loop_: &ono_parser::ForStmt,
    source: &str,
) -> Eval<ExitStatus> {
    let subject = eval_expr(session, &loop_.iterable, source)?;
    let items: Vec<Value> = match &subject {
        Value::List(items) => items.to_vec(),
        Value::Null => Vec::new(),
        single => vec![single.clone()],
    };

    let mut status = ExitStatus::SUCCESS;
    for item in items {
        session.push_scope();
        session.bind(loop_.binding.clone(), item);
        let outcome = run_block(session, &loop_.body, source);
        session.pop_scope();
        match outcome {
            Ok(reached) => status = reached,
            Err(Flow::Break) => break,
            Err(Flow::Continue) => continue,
            Err(other) => return Err(other),
        }
    }
    Ok(status)
}

pub(super) fn run_match(
    session: &mut Session,
    match_: &ono_parser::MatchStmt,
    source: &str,
) -> Eval<ExitStatus> {
    let subject = eval_expr(session, &match_.subject, source)?;
    for arm in &match_.arms {
        if !pattern_matches(session, &arm.pattern, &subject, source)? {
            continue;
        }
        return match &arm.body {
            ono_parser::MatchArmBody::Block(block) => run_block(session, block, source),
            ono_parser::MatchArmBody::Expr(expression) => {
                eval_expr(session, expression, source)?;
                Ok(ExitStatus::SUCCESS)
            }
        };
    }
    Ok(ExitStatus::SUCCESS)
}

pub(super) fn pattern_matches(
    session: &mut Session,
    pattern: &ono_parser::Pattern,
    subject: &Value,
    source: &str,
) -> Eval<bool> {
    match pattern {
        ono_parser::Pattern::Wildcard(_) => Ok(true),
        ono_parser::Pattern::Literal(expression) => {
            let expected = eval_expr(session, expression, source)?;
            Ok(equals(subject, &expected))
        }
        ono_parser::Pattern::Binding { name, .. } => {
            // A pattern name that spells a type tests the subject's type; anything else binds it.
            // Both readings are useful and neither can be confused for the other in practice,
            // because no type name is a plausible binding name.
            if name == subject.type_name() {
                return Ok(true);
            }
            if is_type_name(name) {
                return Ok(false);
            }
            session.bind(name.clone(), subject.clone());
            Ok(true)
        }
        ono_parser::Pattern::Error(span) => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("this pattern could not be read at {span}"),
        ))),
    }
}

pub(super) fn run_try(
    session: &mut Session,
    try_: &ono_parser::TryStmt,
    source: &str,
) -> Eval<ExitStatus> {
    match run_block(session, &try_.body, source) {
        Err(Flow::Failed(error) | Flow::FailedWith(error, _)) => match &try_.catch {
            Some(catch) => {
                session.push_scope();
                if let Some(name) = &catch.binding {
                    session.bind(name.clone(), Value::Error(std::sync::Arc::new(error)));
                }
                let outcome = run_block(session, &catch.body, source);
                session.pop_scope();
                outcome
            }
            None => Ok(ExitStatus::FAILURE),
        },
        other => other,
    }
}
