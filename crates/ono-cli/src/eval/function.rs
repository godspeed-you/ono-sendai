//! User functions: resolving the call, binding the parameters, running the body.
//!
//! v0.4.1 §26.2's streaming continuation is decided in `pipeline`; what is left here is the
//! shape of a call the caller cannot be continued through.

use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Argument, Stage, StageHead, StageList};
use ono_value::{ErrorValue, Value};

use crate::expand;
use crate::session::{Function, Session};

use super::block::run_block;
use super::expression::{eval_expr, text_of};
use super::{Eval, Flow};

/// The user function a stage's head names, if that is what the head resolves to.
///
/// Step 2 of the resolution order (ADR-0011): a bare head or a `fn:` head, before an alias, the
/// native registry and `PATH`.
pub(super) fn called_function(
    session: &Session,
    stage: &Stage,
) -> Option<std::sync::Arc<Function>> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    if !matches!(name.namespace.as_deref(), None | Some("fn")) {
        return None;
    }
    session.function(&name.name)
}

/// Calls `function` as the head of `list`: binds the stage's arguments to its parameters and
/// runs its body, whose results are what the rest of the pipeline consumes (ADR-0070).
pub(super) fn call_function(
    session: &mut Session,
    function: &Function,
    list: &StageList,
    source: &str,
) -> Eval<ExitStatus> {
    let stage = &list.stages[0];
    let declaration = &function.declaration;
    let body_source: &str = &function.source;

    // Arguments are read before the callee's scope exists, so `$x` in an argument is the
    // caller's `x`.
    let arguments = call_arguments(session, stage, source)?;
    if arguments.len() > declaration.parameters.len() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "`{}` takes {} argument(s), and {} were given",
                    declaration.name,
                    declaration.parameters.len(),
                    arguments.len()
                ),
            )
            .with_help(format!(
                "declared at {} as `fn {}({})`",
                declaration.span,
                declaration.name,
                declaration
                    .parameters
                    .iter()
                    .map(|parameter| parameter.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        ));
    }

    session.push_scope();
    let bound = bind_parameters(session, declaration, arguments, body_source);
    let outcome = match bound {
        Ok(()) => run_function_body(session, declaration, list, source, body_source),
        Err(flow) => Err(flow),
    };
    session.pop_scope();
    outcome
}

/// The argument values a call site supplies, in order: words expanded and lists spliced as for
/// any command (ADR-0019), values as themselves.
pub(super) fn call_arguments(
    session: &mut Session,
    stage: &Stage,
    source: &str,
) -> Eval<Vec<CallArgument>> {
    let mut arguments = Vec::new();
    for argument in &stage.arguments {
        match argument {
            Argument::Word(word) => {
                for expanded in expand::expand_word(session, &word.text)? {
                    arguments.push(CallArgument::Word(expanded.to_string_lossy().into_owned()));
                }
            }
            Argument::Option(option) => {
                let text = match &option.value {
                    Some(value) => format!(
                        "--{}={}",
                        option.name,
                        text_of(&eval_expr(session, value, source)?)?
                    ),
                    None => format!("--{}", option.name),
                };
                arguments.push(CallArgument::Word(text));
            }
            Argument::Value(expression) => match eval_expr(session, expression, source)? {
                Value::List(items) => {
                    arguments.extend(items.iter().cloned().map(CallArgument::Value));
                }
                single => arguments.push(CallArgument::Value(single)),
            },
            Argument::Error(_) => {
                return Err(Flow::Failed(ErrorValue::new(
                    ErrorCode::ParseSyntax,
                    "this argument could not be read",
                )));
            }
        }
    }
    Ok(arguments)
}

/// One argument at a call site.
pub(super) enum CallArgument {
    /// A bare word: a string, unless the parameter declares a type to read it as.
    Word(String),
    /// A value, bound as it is.
    Value(Value),
}

/// Binds the parameters in the callee's (already pushed) scope: an argument in order, else the
/// default, else `null` (ADR-0070).
pub(super) fn bind_parameters(
    session: &mut Session,
    declaration: &ono_parser::FnDecl,
    arguments: Vec<CallArgument>,
    body_source: &str,
) -> Eval<()> {
    let mut arguments = arguments.into_iter();
    for parameter in &declaration.parameters {
        let value = match arguments.next() {
            Some(CallArgument::Value(value)) => value,
            Some(CallArgument::Word(word)) => {
                coerce_word(word, parameter.ty.as_ref(), &parameter.name)?
            }
            None => match &parameter.default {
                Some(default) => eval_expr(session, default, body_source)?,
                None => Value::Null,
            },
        };
        session.bind(parameter.name.clone(), value);
    }
    Ok(())
}

/// Reads a word as the type its parameter declares. Without a declared type the word is a
/// string: a shell never guesses what a word means (ADR-0019, ADR-0070).
pub(super) fn coerce_word(
    word: String,
    ty: Option<&ono_parser::TypeRef>,
    parameter: &str,
) -> Eval<Value> {
    let Some(ty) = ty else {
        return Ok(Value::String(word.into()));
    };
    let mismatch = |wanted: &str| {
        Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{word}` is not {wanted}, which parameter `{parameter}` expects"),
            )
            .with_help(format!(
                "parameter `{parameter}` is declared as `{}`",
                ty.name
            )),
        )
    };
    Ok(match ty.name.to_ascii_lowercase().as_str() {
        "int" => Value::Int(word.trim().parse().map_err(|_| mismatch("an integer"))?),
        "float" | "decimal" => Value::Float(word.trim().parse().map_err(|_| mismatch("a number"))?),
        "bool" => match word.trim() {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(mismatch("`true` or `false`")),
        },
        "path" => Value::Path(std::sync::Arc::from(std::path::Path::new(&word))),
        _ => Value::String(word.into()),
    })
}

/// Runs a function's body in the caller's output context (ADR-0070).
///
/// With stages after the call, the body's results are captured and stream into them, exactly
/// as a producer's would. With nothing after it the body's statements show their results as
/// they would at the prompt, and a `return` value is shown the same way — or handed to the
/// enclosing capture when the call itself is being bound.
pub(super) fn run_function_body(
    session: &mut Session,
    declaration: &ono_parser::FnDecl,
    list: &StageList,
    source: &str,
    body_source: &str,
) -> Eval<ExitStatus> {
    let consumed = list.stages.len() > 1;
    // v0.4.1 §26.2's streaming continuation. A body that is one pipeline is one pipeline: its
    // stages are assembled while the invocation's scope is on the session (§26.3), and the stream
    // they produce is what the stages after the call read — so `watched | take 1` is answered from
    // the first value the body produced rather than from a collection of all of them (ADR-0481).
    //
    // Every other body still collects, which is what it always did: §26.2 permits that where the
    // shape cannot be continued, and `explain` says so of the call (§22.4).
    if consumed
        && let Some(body) = super::native::continuable_body(&declaration.body)
        && let Some((stream, failed_rows)) =
            super::native::stream_segment(session, body, body_source)?
    {
        return super::native::run_piped(session, list, source, stream, failed_rows);
    }
    if consumed {
        session.begin_capture();
    }
    let outcome = run_block(session, &declaration.body, body_source);
    let mut values = if consumed {
        session.end_capture()
    } else {
        Vec::new()
    };
    let status = match outcome {
        Ok(status) => status,
        Err(Flow::Return(value)) => {
            if !matches!(value, Value::Null) {
                values.push(value);
            }
            ExitStatus::SUCCESS
        }
        Err(other) => return Err(other),
    };
    if consumed {
        return super::native::run_seeded(session, list, source, values);
    }
    if !values.is_empty() {
        if session.capturing() {
            session.capture(&values)?;
        } else {
            return super::native::run_seeded(session, list, source, values);
        }
    }
    Ok(status)
}
