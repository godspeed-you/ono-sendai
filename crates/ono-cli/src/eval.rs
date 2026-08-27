//! The evaluator: from a parsed program to what actually happens.
//!
//! The execution model is ADR-0013's. Phase A carries only external stages, so a pipeline becomes
//! an `ono_process::Pipeline` and runs in the foreground; the native stages of phase B slot in
//! beside them without changing the shape of anything here.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use ono_core::{ErrorCode, ExitStatus, Span};
use ono_parser::{
    Argument, BinaryOp, Block, Expr, NumberValue, Pipeline, RedirectOp, RedirectTarget,
    Redirection, Stage, StageHead, StageList, Statement, StrPart, UnaryOp, Unit,
};
use ono_process::{Command, Fd, Redirect};
use ono_value::{ByteSize, Duration as OnoDuration, ErrorValue, MapValue, Percent, Value};

use crate::builtin;
use crate::expand;
use crate::resolve::{self, Namespace, Resolution};
use crate::session::{Alias, Definition, Function, Mode, Session};

/// Why evaluation of a statement stopped early.
#[derive(Debug)]
pub enum Flow {
    /// A structured error, to be reported and to set a failing status.
    Failed(ErrorValue),
    /// A structured error whose status is already known — an external command that could not be
    /// started reports both, and ADR-0008 requires its own status to be the one that survives.
    FailedWith(ErrorValue, ExitStatus),
    /// `break` inside a loop.
    Break,
    /// `continue` inside a loop.
    Continue,
    /// `return` from a function body, carrying its value.
    Return(Value),
    /// `exit`, which unwinds to the top without ending the process abruptly.
    Exit(ExitStatus),
}

impl From<ErrorValue> for Flow {
    fn from(error: ErrorValue) -> Self {
        Flow::Failed(error)
    }
}

/// The result of evaluating something that can fail or jump.
pub type Eval<T> = Result<T, Flow>;

/// Runs a whole program, returning the status of the last statement it reached.
pub fn run_program(
    session: &mut Session,
    program: &ono_parser::Program,
    source: &str,
    report: &mut dyn FnMut(&ErrorValue),
) -> ExitStatus {
    for statement in &program.statements {
        match run_statement(session, statement, source) {
            Ok(status) => session.set_status(status),
            Err(Flow::Failed(error)) => {
                report(&error);
                session.set_status(status_for(&error));
            }
            Err(Flow::FailedWith(error, status)) => {
                report(&error);
                session.set_status(status);
            }
            Err(Flow::Exit(status)) => {
                session.leave(status);
                return status;
            }
            // A jump outside any construct that could catch it ends the program quietly, which
            // is what a `return` at the top level of a script means.
            Err(Flow::Return(_) | Flow::Break | Flow::Continue) => return session.status(),
        }
        if let Some(status) = session.leaving() {
            return status;
        }
        // Reap whatever finished while that statement ran. Without this a script that backgrounds
        // work accumulates a zombie per job for the life of the process — the interactive loop
        // polls between prompts, and a script has no prompts. `waitpid` with `WNOHANG` costs one
        // syscall that finds nothing when there is nothing.
        let _ = session.executor().poll_jobs();
    }
    session.status()
}

/// The exit status an error implies (ADR-0008).
#[must_use]
pub fn status_for(error: &ErrorValue) -> ExitStatus {
    match error.code() {
        ErrorCode::ResolveCommandNotFound => ExitStatus::NOT_FOUND,
        ErrorCode::ParseSyntax | ErrorCode::ParseIncomplete => ExitStatus::USAGE,
        _ => ExitStatus::FAILURE,
    }
}

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
            session.bind(binding.name.clone(), value);
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

// --- kill %N (spec §18.1, §18.4, ADR-0071 §4) --------------------------------------------------

/// Whether a stage is `kill %N …`: the bare `kill` with a job specifier as its first word.
fn is_job_kill(stage: &Stage) -> bool {
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

// --- each { … } (spec §19.4, ADR-0071 §1) -----------------------------------------------------

/// The index of the `each` stage whose body is a block, if `list` has one.
fn each_block_stage(list: &StageList) -> Option<usize> {
    list.stages.iter().position(|stage| {
        let StageHead::Command(name) = &stage.head else {
            return false;
        };
        matches!(name.namespace.as_deref(), None | Some("ono"))
            && name.name == "each"
            && matches!(
                stage.arguments.as_slice(),
                [Argument::Value(Expr::Block(_))]
            )
    })
}

/// Runs the stages before `index` for their values, the block once per value with `@` bound to
/// it, and the stages after it over what the blocks produced.
fn run_each_block(
    session: &mut Session,
    list: &StageList,
    source: &str,
    index: usize,
) -> Eval<ExitStatus> {
    let stage = &list.stages[index];
    let Some(Argument::Value(Expr::Block(block))) = stage.arguments.first() else {
        return Ok(ExitStatus::SUCCESS);
    };
    if index == 0 {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                "`each` needs a stream of values to run its block over, and none reaches it",
            )
            .with_help("put a producer in front of it: `get service | where … | each { … }`"),
        ));
    }

    let upstream = StageList {
        stages: list.stages[..index].to_vec(),
        span: list.stages[0].span.join(list.stages[index - 1].span),
    };
    session.begin_capture();
    let outcome = run_stage_list(session, &upstream, source, false);
    let items = session.end_capture();
    outcome?;

    // The block runs in the caller's output context (ADR-0070 point 3): captured when stages
    // follow, shown as it goes when nothing does.
    let consumed = index + 1 < list.stages.len();
    let mut produced = Vec::new();
    for item in items {
        session.push_scope();
        session.bind("@", item);
        if consumed {
            session.begin_capture();
        }
        let outcome = run_block(session, block, source);
        if consumed {
            produced.extend(session.end_capture());
        }
        session.pop_scope();
        match outcome {
            Ok(_) | Err(Flow::Continue) => {}
            Err(Flow::Break) => break,
            Err(other) => return Err(other),
        }
    }
    if consumed {
        return crate::native::run_seeded_from(session, list, source, index + 1, produced);
    }
    Ok(ExitStatus::SUCCESS)
}

// --- prefix assignment (spec §54, ADR-0071 §2) ------------------------------------------------

/// The `NAME=value` words that lead a stage of `list`, with the list as it reads without them.
///
/// `None` when no stage starts with an assignment. A stage that is nothing but assignments is
/// refused: a lasting binding has two explicit spellings already.
fn prefix_assignments(
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
fn is_assignment_word(word: &str) -> bool {
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

// --- aliases (spec §6.5, ADR-0070) ------------------------------------------------------------

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

// --- functions (spec §19.3, ADR-0070) ---------------------------------------------------------

/// The user function a stage's head names, if that is what the head resolves to.
///
/// Step 2 of the resolution order (ADR-0011): a bare head or a `fn:` head, before an alias, the
/// native registry and `PATH`.
fn called_function(session: &Session, stage: &Stage) -> Option<std::sync::Arc<Function>> {
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
fn call_function(
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
fn call_arguments(session: &mut Session, stage: &Stage, source: &str) -> Eval<Vec<CallArgument>> {
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
enum CallArgument {
    /// A bare word: a string, unless the parameter declares a type to read it as.
    Word(String),
    /// A value, bound as it is.
    Value(Value),
}

/// Binds the parameters in the callee's (already pushed) scope: an argument in order, else the
/// default, else `null` (ADR-0070).
fn bind_parameters(
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
fn coerce_word(word: String, ty: Option<&ono_parser::TypeRef>, parameter: &str) -> Eval<Value> {
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
fn run_function_body(
    session: &mut Session,
    declaration: &ono_parser::FnDecl,
    list: &StageList,
    source: &str,
    body_source: &str,
) -> Eval<ExitStatus> {
    let consumed = list.stages.len() > 1;
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
        return crate::native::run_seeded(session, list, source, values);
    }
    if !values.is_empty() {
        if session.capturing() {
            session.capture(&values);
        } else {
            return crate::native::run_seeded(session, list, source, values);
        }
    }
    Ok(status)
}

fn run_block(session: &mut Session, block: &Block, source: &str) -> Eval<ExitStatus> {
    session.push_scope();
    let mut status = ExitStatus::SUCCESS;
    let mut outcome = Ok(());
    for statement in &block.statements {
        match run_statement(session, statement, source) {
            Ok(reached) => status = reached,
            Err(flow) => {
                outcome = Err(flow);
                break;
            }
        }
    }
    session.pop_scope();
    outcome.map(|()| status)
}

fn run_if(session: &mut Session, branch: &ono_parser::IfStmt, source: &str) -> Eval<ExitStatus> {
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

fn run_while(
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

fn run_for(session: &mut Session, loop_: &ono_parser::ForStmt, source: &str) -> Eval<ExitStatus> {
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

fn run_match(
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

fn pattern_matches(
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

fn run_try(session: &mut Session, try_: &ono_parser::TryStmt, source: &str) -> Eval<ExitStatus> {
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

// --- pipelines ---------------------------------------------------------------------------------

/// Runs a pipeline, honouring `&&`, `||` and a trailing `&`.
pub fn run_pipeline(session: &mut Session, pipeline: &Pipeline, source: &str) -> Eval<ExitStatus> {
    // Spec §11.3: field names are checked against the declared schemas before anything runs, so
    // a typo costs one message instead of one per object.
    crate::native::check(session, pipeline, source).map_err(Flow::Failed)?;

    let mut status = run_stage_list(session, &pipeline.head, source, pipeline.background)?;

    for chained in &pipeline.tail {
        let should_run = match chained.op {
            ono_parser::ChainOp::And => status.is_success(),
            ono_parser::ChainOp::Or => !status.is_success(),
        };
        if should_run {
            status = run_stage_list(session, &chained.list, source, pipeline.background)?;
        }
    }
    Ok(status)
}

fn run_stage_list(
    session: &mut Session,
    list: &StageList,
    source: &str,
    background: bool,
) -> Eval<ExitStatus> {
    // Step 2 of the resolution order (ADR-0011): a user function wins over everything but a
    // keyword, and the keywords were the parser's.
    if !background
        && let Some(stage) = list.stages.first()
        && let Some(function) = called_function(session, stage)
    {
        return call_function(session, &function, list, source);
    }

    // Spec §54: `NAME=value command …` sets the variable for this pipeline alone (ADR-0071 §2).
    if let Some((assignments, stripped)) = prefix_assignments(session, list, source)? {
        let previous: Vec<(String, Option<OsString>)> = assignments
            .iter()
            .map(|(name, _)| (name.clone(), session.env_var(name).map(OsStr::to_os_string)))
            .collect();
        for (name, value) in &assignments {
            session.set_env(name.as_str(), value.clone());
        }
        let outcome = run_stage_list(session, &stripped, source, background);
        for (name, value) in previous {
            match value {
                Some(value) => session.set_env(name, value),
                None => session.remove_env(&name),
            }
        }
        return outcome;
    }

    // Step 3: an alias is expanded exactly once and the result resolved again from the top
    // (ADR-0011, ADR-0070).
    if let Some((name, expanded)) = expand_alias(session, list, source) {
        let expanded = if background {
            format!("{expanded} &")
        } else {
            expanded
        };
        let parsed = ono_parser::parse(&expanded);
        let pipeline = parsed
            .program()
            .statements
            .first()
            .and_then(Statement::as_pipeline)
            .cloned()
            .ok_or_else(|| {
                Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ParseSyntax,
                        format!("the expansion of alias `{name}` is not a pipeline"),
                    )
                    .with_help(format!("`{name}` expands to `{expanded}`")),
                )
            })?;
        session.begin_expanding(name);
        let outcome = run_pipeline(session, &pipeline, &expanded);
        session.finish_expanding();
        return outcome;
    }

    // `kill %N` names a job, and a job is the shell's (spec §18.1, §18.4; ADR-0071 §4). Any
    // other `kill` is the program or the native verb, untouched.
    if list.stages.len() == 1
        && let Some(stage) = list.stages.first()
        && is_job_kill(stage)
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("kill")));
        }
        let arguments = stage_arguments(session, stage, source)?;
        return builtin::kill_jobs(session, &arguments);
    }

    // `resolve command`, `get config` and `set config` are answered by the shell, which alone
    // sees every stage of the order and every configuration layer (ADR-0011, ADR-0093,
    // ADR-0094). Their values seed whatever follows, as a producer's stream would.
    if !background
        && let Some(stage) = list.stages.first()
        && let Some(request) = crate::meta::claims(stage)
    {
        let alone = list.stages.len() == 1;
        // A configuration file may set a value and nothing more: `get config` in one would
        // print, and a `set config` with stages after it would run them (ADR-0010).
        if session.mode() == Mode::Config && !(request == crate::meta::Request::SetConfig && alone)
        {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        let values = crate::meta::answer(session, stage, source, request)?;
        // `set config` on its own is as quiet as `set env`: a settings line prints nothing at
        // the prompt or in a file. Its ActionResult flows when something consumes it.
        if request == crate::meta::Request::SetConfig && alone && !session.capturing() {
            return Ok(ExitStatus::SUCCESS);
        }
        return crate::native::run_seeded(session, list, source, values);
    }

    // A single builtin stage runs in the shell itself: `cd` in a child moves a directory nobody
    // is standing in.
    if list.stages.len() == 1
        && let Some(stage) = list.stages.first()
        && let Some(name) = builtin_name(session, stage)
    {
        // The configuration check comes before the command runs, and covers builtins as well as
        // external programs. Only the declarative ones are allowed: ADR-0010 says configuration
        // "sets values, defines functions and aliases", and a check that stopped `touch` while
        // letting `cd`, `exit` and `jobs` through would be a claim the code did not keep.
        if session.mode() == Mode::Config && !builtin::allowed_in_config(name) {
            return Err(Flow::Failed(config_refusal(name)));
        }
        // A builtin writes through the shell's own output, so a redirection has to be applied
        // here rather than by a child that does not exist. Silently ignoring it would send the
        // output to the terminal while the user was told it went to a file.
        if let Some(redirection) = stage.redirections.first() {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{name}` runs in the shell itself and cannot be redirected"),
                )
                .with_help(format!(
                    "`{name}` has no child process to redirect. Send it through a command that \
                     does: `{name} | to text > file`. The redirection at {} was not applied.",
                    redirection.span
                )),
            ));
        }

        let arguments = stage_arguments(session, stage, source)?;
        return builtin::run(session, name, &arguments);
    }

    // `explain` in front of a pipeline explains the whole pipeline, exactly as spec §11.3
    // spells it: `explain get process | where cpu > 20 | stop process`. The pipes belong to the
    // subject, so the subject is the source text from explain's first word to the end of the
    // list, handed over verbatim — never re-rendered from the AST, which would explain a
    // normalisation of what the user typed rather than what they typed.
    if list.stages.len() > 1
        && let Some(first) = list.stages.first()
        && builtin_name(session, first) == Some("explain")
        && let Some(end) = list.stages.last().map(|stage| stage.span.end())
    {
        let start = first
            .arguments
            .first()
            .map_or(first.span.end(), |argument| argument.span().start());
        let subject = source
            .get(start as usize..end as usize)
            .unwrap_or_default()
            .trim();
        return builtin::run(session, "explain", &[OsString::from(subject)]);
    }

    // A builtin in a longer pipeline used to be handed to `exec`, which reported it as not found
    // and then reported the pipeline as successful. Where the name also exists as a program —
    // `true`, `false` — the program is what runs, which is what every other shell does and what
    // keeps `false | true` meaningful. Where it does not, saying so plainly is the least
    // surprising answer: `cd` changes the shell, so there is no process for a pipe to attach to.
    for stage in &list.stages {
        if let Some(name) = builtin_name(session, stage)
            && stage
                .head
                .name()
                .and_then(|head| resolve::find_on_path(session, head))
                .is_none()
        {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`{name}` runs in the shell itself and cannot be a pipeline stage"),
                )
                .with_help(
                    "the shell's own commands change the shell, so there is no process for a pipe \
                     to attach to",
                ),
            ));
        }
    }

    // `… | enter socket`: the object to enter arrives through the pipeline (spec §14.3). The
    // stages before it run as the native pipeline they are, with their result kept for the
    // frame instead of shown, and the last stage pushes the frame (ADR-0075).
    if list.stages.len() > 1
        && let Some(last) = list.stages.last()
        && matches!(
            crate::context::claims(last),
            Some(crate::context::Request::Enter)
        )
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        let head = &list.stages[..list.stages.len() - 1];
        let prefix = StageList {
            stages: head.to_vec(),
            span: Span::new(
                list.span.start(),
                head.last()
                    .map_or(list.span.end(), |stage| stage.span.end()),
            ),
        };
        let values = crate::native::run_collecting(session, &prefix, source)?;
        return crate::context::enter_piped(session, last, source, &values);
    }

    // `enter` and `leave` change what later commands mean, which is session state: the same
    // reason `cd` runs in the shell (spec §14.1, ADR-0023).
    if list.stages.len() == 1
        && let Some(stage) = list.stages.first()
        && let Some(request) = crate::context::claims(stage)
    {
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        return match request {
            crate::context::Request::Enter => crate::context::enter(session, stage, source),
            crate::context::Request::Leave => crate::context::leave(session, stage, source),
            crate::context::Request::Link => crate::context::link(session, stage, source),
            crate::context::Request::GetPlugin => crate::plugins::get_plugin(session),
            crate::context::Request::LoadPlugin => {
                let words: Vec<String> = stage_arguments(session, stage, source)?
                    .iter()
                    .map(|word| word.to_string_lossy().into_owned())
                    .collect();
                let (id, options) = crate::plugins::LoadOptions::from_words(&words);
                match id {
                    Some(id) => crate::plugins::load_plugin_with(session, &id, &options),
                    None => Err(Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            "`load plugin` needs the package to load",
                        )
                        .with_help("`get plugin` lists the installed set (spec §31.8)"),
                    )),
                }
            }
        };
    }

    // `each { … }` with a block runs in the shell: a block holds statements, and a statement may
    // run a command, which the transform engine cannot (spec §19.4, ADR-0071 §1).
    if !background && let Some(index) = each_block_stage(list) {
        return run_each_block(session, list, source, index);
    }

    // A `<package>:command` head invokes a loaded KUANG/11 package's contribution (spec §31.22,
    // ADR-0011's module namespace). The values it streams seed the rest of the pipeline exactly
    // as a native producer's would.
    if !background
        && let Some(stage) = list.stages.first()
        && let StageHead::Command(name) = &stage.head
        && let Some(namespace) = name.namespace.as_deref()
        && Namespace::from_prefix(Some(namespace)).is_none()
        && crate::plugins::loaded_package(session, namespace).is_some()
    {
        let words = stage_arguments(session, stage, source)?;
        let command = name.name.clone();
        let namespace = namespace.to_owned();
        let values = crate::plugins::invoke(session, &namespace, &command, &words)?;
        return crate::native::run_seeded(session, list, source, values);
    }

    // A pipeline may start with a value instead of a command: `$hot | where …`, `@-1 | count`
    // (spec §10.2, §20.2). The head is evaluated once and a list splices, because a list *is*
    // several values (ADR-0019); everything after it runs as if a producer had streamed them.
    if !background
        && let Some(stage) = list.stages.first()
        && let StageHead::Value(expression) = &stage.head
    {
        let expression = expression.clone();
        let value = eval_expr(session, &expression, source)?;
        let seed = match value {
            Value::List(items) => items.to_vec(),
            other => vec![other],
        };
        return crate::native::run_seeded(session, list, source, seed);
    }

    // A pipeline with a native command in it runs through the object pipeline of spec §5, which
    // threads bytes across the boundary of spec §12.3 where a child process sits on one side.
    // So does an all-external pipeline whose last stage an adapter renders at the terminal
    // (spec v0.3 §1.4), which is what makes `lsblk` typed at the prompt a table.
    if crate::native::claims(session, list)
        || (!background && !session.capturing() && crate::native::adapts_at_terminal(session, list))
    {
        // A native command is as much "running something" as a child process is: `set file`
        // reaches the registry now (ADR-0068), and a configuration file that could change a
        // file's mode would be a startup script wearing a settings file's name (ADR-0010).
        if session.mode() == Mode::Config {
            return Err(Flow::Failed(config_refusal("this command")));
        }
        if background {
            // Spec §18.4: a backgrounded native pipeline is a job — listed, addressable,
            // stoppable — never a hidden thread (ADR-0024).
            return crate::native::run_background(session, list, source);
        }
        return crate::native::run(session, list, source);
    }

    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
    }

    // A pipeline being captured hands its stdout to the capture rather than the terminal: the
    // text is the value of `(echo hi)` (ADR-0069).
    if !background && session.capturing() {
        let indices: Vec<usize> = (0..list.stages.len()).collect();
        let (_, status) = run_external_segment(session, list, &indices, source, None, true)?;
        return Ok(status);
    }

    let mut built = ono_process::Pipeline::new();
    for stage in &list.stages {
        built = built.stage(build_command(session, stage, source)?);
    }

    if background {
        let id = session
            .executor()
            .run_background(&built)
            .map_err(process_error)?;
        session.note_job_started(id.number());
        eprintln!("[{id}]");
        return Ok(ExitStatus::SUCCESS);
    }

    let outcome = session
        .executor()
        .run_foreground(&built)
        .map_err(process_error)?;

    // A stage that could not be started reports both a status and a structured reason. The
    // reason is what the user needs — "no such file" beats "exited 1" — so it is raised rather
    // than left on the outcome for nobody to read.
    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }
    Ok(outcome.status())
}

/// Runs a run of adjacent external stages, threading bytes into and out of it.
///
/// Adjacent child processes are joined to each other by real pipes, exactly as before: ADR-0013
/// keeps `yes | head -1` a genuine `SIGPIPE` rather than a buffer the shell drains. Bytes only
/// pass through the shell where a native stage sits on one side of the boundary.
///
/// # Errors
///
/// The structured error of whichever stage could not be built or started.
pub fn run_external_segment(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    last: bool,
) -> Eval<(Option<Vec<u8>>, ExitStatus)> {
    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
    }

    let captured = last && session.capturing();
    let capture = !last || captured;
    let mut built = ono_process::Pipeline::new();
    for (position, index) in indices.iter().enumerate() {
        let mut command = build_command(session, &list.stages[*index], source)?;
        if position == 0
            && let Some(bytes) = input.clone()
        {
            command = command.stdin(ono_process::Input::Bytes(bytes));
        }
        if position + 1 == indices.len() && capture {
            command = command.stdout(ono_process::Output::Capture);
        }
        built = built.stage(command);
    }

    let outcome = session
        .executor()
        .run_foreground(&built)
        .map_err(process_error)?;

    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }

    let bytes = capture.then(|| {
        outcome
            .completed()
            .and_then(|completed| completed.stages().last())
            .map(|stage| stage.stdout.clone())
            .unwrap_or_default()
    });
    if captured {
        session.capture(&[captured_text(bytes.as_deref().unwrap_or_default())]);
        return Ok((None, outcome.status()));
    }
    Ok((bytes, outcome.status()))
}

/// Runs an external segment whose last stage is adapted: the same pipeline, with the last
/// command replaced by the adapter's plan and its stdout captured for decoding (ADR-0057).
///
/// # Errors
///
/// As [`run_external_segment`]: the child's own failure, with its status (spec v0.3 §1.20).
pub fn run_adapted_segment(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &ono_adapter::AdapterPlan,
) -> Eval<(Vec<u8>, ExitStatus)> {
    let built = adapted_pipeline(
        session,
        list,
        indices,
        source,
        input,
        plan,
        ono_process::Output::Capture,
    )?;
    let outcome = session
        .executor()
        .run_foreground(&built)
        .map_err(process_error)?;
    if let ono_process::ForegroundOutcome::Completed(completed) = &outcome
        && let Some(failure) = completed.failure()
    {
        return Err(Flow::FailedWith(
            ErrorValue::new(failure.code(), failure.message().to_owned()),
            outcome.status(),
        ));
    }
    let bytes = outcome
        .completed()
        .and_then(|completed| completed.stages().last())
        .map(|stage| stage.stdout.clone())
        .unwrap_or_default();
    Ok((bytes, outcome.status()))
}

/// Starts an adapted segment whose records are decoded while it runs (ADR-0059): the last
/// stage's stdout is handed back as a pipe, the terminal stays with the shell.
///
/// # Errors
///
/// A stage that could not be started, with its status.
pub fn start_adapted_segment(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &ono_adapter::AdapterPlan,
) -> Eval<ono_process::Foreground> {
    let built = adapted_pipeline(
        session,
        list,
        indices,
        source,
        input,
        plan,
        ono_process::Output::Pipe,
    )?;
    let started = session
        .executor()
        .start_piped(&built)
        .map_err(process_error)?;
    if let Some(failure) = started.failure() {
        let error = ErrorValue::new(failure.code(), failure.message().to_owned());
        let outcome = session
            .executor()
            .finish_foreground(started)
            .map_err(process_error)?;
        return Err(Flow::FailedWith(error, outcome.status()));
    }
    Ok(started)
}

fn adapted_pipeline(
    session: &mut Session,
    list: &StageList,
    indices: &[usize],
    source: &str,
    input: Option<Vec<u8>>,
    plan: &ono_adapter::AdapterPlan,
    stdout: ono_process::Output,
) -> Eval<ono_process::Pipeline> {
    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
    }
    let mut built = ono_process::Pipeline::new();
    for (position, index) in indices.iter().enumerate() {
        let stage = &list.stages[*index];
        let adapted = position + 1 == indices.len();
        let mut command = if adapted {
            adapted_command(session, stage, plan, source)?
        } else {
            build_command(session, stage, source)?
        };
        if position == 0 {
            if let Some(bytes) = input.clone() {
                command = command.stdin(ono_process::Input::Bytes(bytes));
            } else if adapted && plan.stdin() == ono_adapter::StdinMode::Null {
                command = command.stdin(ono_process::Input::Null);
            }
        }
        if adapted {
            command = command.stdout(stdout);
        }
        built = built.stage(command);
    }
    Ok(built)
}

/// The command an adapter's plan describes: the pinned executable, the plan's argv, the plan's
/// environment on top of the session's, and the stage's own redirections (spec v0.3 §1.7).
fn adapted_command(
    session: &mut Session,
    stage: &Stage,
    plan: &ono_adapter::AdapterPlan,
    source: &str,
) -> Eval<Command> {
    let mut command = Command::new(plan.executable().as_os_str())
        .args(plan.argv().iter().skip(1).map(OsString::from))
        .current_dir(session.cwd())
        .env_clear();
    for (name, value) in session.env() {
        command = command.env(name, value);
    }
    for (name, value) in plan.env() {
        command = command.env(OsString::from(name), OsString::from(value));
    }
    for redirection in &stage.redirections {
        command = command.redirect(build_redirect(session, redirection, source)?);
    }
    Ok(command)
}

/// Opens the file a stage's redirections send its output to, if any.
///
/// A native stage writes through the shell rather than through a child, so its redirection has to
/// be applied here. Only the output forms are meaningful: a native producer reads no bytes.
///
/// # Errors
///
/// The structured error of a redirection that cannot be understood or a file that cannot be
/// opened.
pub fn output_destination(
    session: &mut Session,
    stage: &Stage,
    source: &str,
) -> Eval<Option<std::fs::File>> {
    let Some(redirection) = stage.redirections.last() else {
        return Ok(None);
    };
    // The same reading of a redirection a child process gets, so `> f` means one thing in the
    // shell however the stage on its left is run.
    let (path, append) = match build_redirect(session, redirection, source)? {
        Redirect::Write { path, .. } => (path, false),
        Redirect::Append { path, .. } => (path, true),
        Redirect::Read { .. } | Redirect::Duplicate { .. } => {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "a native command's output can only be sent to a file",
                )
                .with_help("send it through `to json` into a program to redirect it any other way"),
            ));
        }
    };

    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true);
    if append {
        options.append(true);
    } else {
        options.truncate(true);
    }
    options.open(&path).map(Some).map_err(|error| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("cannot open {}: {error}", path.display()),
        ))
    })
}

/// A redirection with no descriptor written means the obvious one: 0 for input, 1 for output.
fn descriptor_for(redirection: &Redirection) -> u16 {
    match redirection.fd {
        Some(written) => narrow_fd(written),
        None => match redirection.op {
            RedirectOp::Read | RedirectOp::DupRead => 0,
            _ => 1,
        },
    }
}

/// Descriptor numbers above the process limit cannot exist; saturating keeps a nonsensical one
/// from wrapping into a real descriptor.
fn narrow_fd(number: u32) -> u16 {
    u16::try_from(number).unwrap_or(u16::MAX)
}

/// Whether `name` spells one of the value model's type names.
fn is_type_name(name: &str) -> bool {
    matches!(
        name,
        "null"
            | "bool"
            | "int"
            | "float"
            | "decimal"
            | "string"
            | "bytes"
            | "path"
            | "timestamp"
            | "duration"
            | "bytesize"
            | "percent"
            | "regex"
            | "uuid"
            | "ip"
            | "ipnetwork"
            | "port"
            | "list"
            | "map"
            | "record"
            | "error"
    )
}

/// Why a configuration file may not run `what` (ADR-0010).
fn config_refusal(what: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::SafetyPolicyDenied,
        format!("a configuration file may not run {what}"),
    )
    .with_help(
        "configuration is declarative: it sets values with `set`, removes them with `remove`, and \
         defines functions and aliases. It runs nothing else — not an external command, and not \
         one of the shell's own (ADR-0010).",
    )
}

fn process_error(error: ono_process::Error) -> Flow {
    Flow::Failed(ErrorValue::new(error.code(), error.message().to_owned()))
}

fn builtin_name(session: &Session, stage: &Stage) -> Option<&'static str> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    let namespace = Namespace::from_prefix(name.namespace.as_deref())?;
    if matches!(namespace, Namespace::External) {
        return None;
    }
    match resolve::resolve(session, namespace, &name.name) {
        Ok(Resolution::Builtin(builtin)) => resolve::builtin_for(builtin, first_word(stage)),
        _ => None,
    }
}

/// The literal word after the head, which decides whether `set`/`remove` are the shell's.
fn first_word(stage: &Stage) -> Option<&str> {
    stage
        .arguments
        .first()
        .and_then(ono_parser::Argument::as_word)
}

/// Builds the external command one stage describes.
///
/// Everything that reaches here becomes a child process — a stage the shell runs itself has
/// already been handled — so a name that is both a builtin and a program on `PATH` resolves to
/// the program. That is what keeps `false | true` meaningful.
fn build_command(session: &mut Session, stage: &Stage, source: &str) -> Eval<Command> {
    let StageHead::Command(name) = &stage.head else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "this stage has no command to run",
        )));
    };

    // `raw <program> …` runs the program on PATH and nothing else, with the arguments exactly as
    // typed (spec v0.3 §1.17, ADR-0054). The keyword wins over a program called `raw`, as
    // `explain` does; `exec:raw` reaches such a program.
    if name.namespace.is_none() && name.name == ono_adapter::ADAPT {
        // The native runner claims every `adapt` stage; reaching here means the stage had
        // nothing to adapt — `adapt` alone — or a form the runner does not take (a
        // background job), and a forced adaptation never runs as a plain program.
        let arguments = stage_arguments(session, stage, source)?;
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                if arguments.is_empty() {
                    "`adapt` needs a program to adapt".to_owned()
                } else {
                    "`adapt` cannot run here".to_owned()
                },
            )
            .with_help(
                "`adapt <program> [arguments]` forces the program's output into values \
                 (spec v0.3 §1.18); run it in the foreground",
            ),
        ));
    }
    if name.namespace.is_none() && name.name == ono_adapter::RAW {
        let mut arguments = stage_arguments(session, stage, source)?;
        if arguments.is_empty() {
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    "`raw` needs a program to run",
                )
                .with_help(
                    "`raw <program> [arguments]` runs the program with nothing between it and \
                     the terminal: no argv rewrite, no decoder, no renderer (spec v0.3 §1.17)",
                ),
            ));
        }
        let program = arguments.remove(0).to_string_lossy().into_owned();
        let resolution =
            resolve::resolve(session, Namespace::External, &program).map_err(|error| {
                Flow::Failed(error.with_help(format!(
                    "`raw` runs programs only; `{program}` is not one on PATH"
                )))
            })?;
        return assemble_command(session, &resolution, arguments, stage, source);
    }

    let namespace = Namespace::from_prefix(name.namespace.as_deref()).ok_or_else(|| {
        Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!(
                    "unknown namespace `{}`",
                    name.namespace.as_deref().unwrap_or_default()
                ),
            )
            .with_help(
                "the namespaces are `ono:`, `exec:`, `fn:` and a loaded package's name \
                 (ADR-0011, spec §31.22)",
            ),
        )
    })?;

    let namespace = if namespace == Namespace::Any
        && resolve::builtin_for(&name.name, first_word(stage)).is_some()
    {
        Namespace::External
    } else {
        namespace
    };

    let resolution = resolve::resolve(session, namespace, &name.name).map_err(|error| {
        let suggestions = resolve::suggestions(session, &name.name);
        let error = if suggestions.is_empty() {
            error
        } else {
            error.with_help(format!("did you mean: {}", suggestions.join(", ")))
        };
        Flow::Failed(error)
    })?;

    let arguments = stage_arguments(session, stage, source)?;
    assemble_command(session, &resolution, arguments, stage, source)
}

fn assemble_command(
    session: &mut Session,
    resolution: &resolve::Resolution,
    arguments: Vec<OsString>,
    stage: &Stage,
    source: &str,
) -> Eval<Command> {
    let mut command = Command::new(resolve::program_of(resolution))
        .args(arguments)
        .current_dir(session.cwd())
        .env_clear();
    for (name, value) in session.env() {
        command = command.env(name, value);
    }
    for redirection in &stage.redirections {
        command = command.redirect(build_redirect(session, redirection, source)?);
    }
    Ok(command)
}

fn build_redirect(
    session: &mut Session,
    redirection: &Redirection,
    source: &str,
) -> Eval<Redirect> {
    let target = match &redirection.target {
        RedirectTarget::Word(word) => {
            let expanded = expand::expand_to_one(session, &word.text)?;
            PathBuf::from(expanded)
        }
        RedirectTarget::Value(expression) => {
            let value = eval_expr(session, expression, source)?;
            PathBuf::from(OsString::from(text_of(&value)?))
        }
        RedirectTarget::Fd(descriptor) => {
            let fd = descriptor_for(redirection);
            return Ok(Redirect::Duplicate {
                fd: Fd::new(fd),
                from: Fd::new(narrow_fd(*descriptor)),
            });
        }
        RedirectTarget::Error(_) => {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::ParseSyntax,
                "this redirection has no target",
            )));
        }
    };

    let descriptor = descriptor_for(redirection);

    Ok(match redirection.op {
        RedirectOp::Read => Redirect::read_from(Fd::new(descriptor), target),
        RedirectOp::Write => Redirect::write_to(Fd::new(descriptor), target),
        RedirectOp::Append => Redirect::append_to(Fd::new(descriptor), target),
        RedirectOp::DupRead | RedirectOp::DupWrite => {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::ParseSyntax,
                "a duplicating redirection needs a descriptor on its right",
            )));
        }
    })
}

/// Expands a stage's arguments into the argv an external command receives.
///
/// A list contributes one argument per element, because it *is* several values; nothing else
/// contributes more than one (ADR-0019).
pub fn stage_arguments(session: &mut Session, stage: &Stage, source: &str) -> Eval<Vec<OsString>> {
    let mut argv = Vec::new();
    for argument in &stage.arguments {
        match argument {
            Argument::Word(word) => argv.extend(expand::expand_word(session, &word.text)?),
            Argument::Option(option) => match &option.value {
                Some(value) => {
                    let text = text_of(&eval_expr(session, value, source)?)?;
                    argv.push(OsString::from(format!("--{}={text}", option.name)));
                }
                None => argv.push(OsString::from(format!("--{}", option.name))),
            },
            Argument::Value(expression) => {
                let value = eval_expr(session, expression, source)?;
                match value {
                    Value::List(items) => {
                        for item in items.iter() {
                            argv.push(OsString::from(text_of(item)?));
                        }
                    }
                    single => argv.push(OsString::from(text_of(&single)?)),
                }
            }
            Argument::Error(_) => {
                return Err(Flow::Failed(ErrorValue::new(
                    ErrorCode::ParseSyntax,
                    "this argument could not be read",
                )));
            }
        }
    }
    Ok(argv)
}

/// A value's text form, for handing to a process that speaks bytes (spec §12.3).
///
/// A null becomes the empty string here, and only here. Spec §10.5's rule that absence must stay
/// visible is about showing data to a person: a table cell for an unknown value says `null`. An
/// interpolated argument is not a rendering, and `echo "Hello $NAME"` printing `Hello null` when
/// `NAME` is unset would be a worse lie than printing nothing (ADR-0019).
fn text_of(value: &Value) -> Result<String, ErrorValue> {
    if matches!(value, Value::Null) {
        return Ok(String::new());
    }
    ono_value::canonical_text(value)
}

fn string_of(session: &mut Session, expression: &Expr, source: &str) -> Eval<String> {
    let value = eval_expr(session, expression, source)?;
    Ok(text_of(&value)?)
}

/// What `let` binds, with the status of the pipeline that produced it.
fn binding_value(
    session: &mut Session,
    pipeline: &Pipeline,
    source: &str,
) -> Eval<(Value, ExitStatus)> {
    if let Some(expression) = bare_value(pipeline) {
        return Ok((eval_expr(session, expression, source)?, ExitStatus::SUCCESS));
    }
    captured_value(session, pipeline, source)
}

/// The expression a one-stage pipeline is, when it is one: `let name = "world"`.
fn bare_value(pipeline: &Pipeline) -> Option<&Expr> {
    if !pipeline.tail.is_empty() || pipeline.head.stages.len() != 1 {
        return None;
    }
    let stage = pipeline.head.stages.first()?;
    if !stage.redirections.is_empty() {
        return None;
    }
    match (&stage.head, stage.arguments.as_slice()) {
        (StageHead::Value(expression), []) => Some(expression),
        // An expression-mode stage with no arguments and a bare head is a field path or a
        // literal read as a command; `let n = 3` arrives this way.
        (StageHead::Error(_), [Argument::Value(expression)]) => Some(expression),
        _ => None,
    }
}

/// The value a pipeline produces when it is used as one, as in `( … )`.
fn value_of_pipeline(session: &mut Session, pipeline: &Pipeline, source: &str) -> Eval<Value> {
    if let Some(expression) = bare_value(pipeline) {
        return eval_expr(session, expression, source);
    }
    Ok(captured_value(session, pipeline, source)?.0)
}

/// Runs a pipeline for its value rather than its display (spec §19.2, ADR-0069).
///
/// Everything the pipeline would have shown is collected instead: a native pipeline's values, or
/// the text a program wrote to its stdout. One value is that value; several are a list, because
/// a list splices back into several values when it starts a pipeline (ADR-0019); none is the
/// empty list — the pipeline is known to have produced nothing, which is not the same as not
/// knowing. The status is the pipeline's own, so `$?` after `let x = …` says whether it worked.
fn captured_value(
    session: &mut Session,
    pipeline: &Pipeline,
    source: &str,
) -> Eval<(Value, ExitStatus)> {
    session.begin_capture();
    let outcome = run_pipeline(session, pipeline, source);
    let mut captured = session.end_capture();
    let status = outcome?;
    let value = match captured.len() {
        1 => captured.remove(0),
        _ => Value::list(captured),
    };
    Ok((value, status))
}

/// The value a program's captured stdout stands for: its text, without the newline every
/// line-oriented program ends its output with — `(echo hi)` is `hi`, exactly as `$(echo hi)`
/// has always been.
#[must_use]
pub fn captured_text(bytes: &[u8]) -> Value {
    let text = String::from_utf8_lossy(bytes);
    Value::String(text.trim_end_matches(['\n', '\r']).into())
}

// --- expressions -------------------------------------------------------------------------------

/// Evaluates an expression to a value.
pub fn eval_expr(session: &mut Session, expression: &Expr, source: &str) -> Eval<Value> {
    match expression {
        Expr::Number(literal) => Ok(number_value(literal.value)),
        Expr::Unit(literal) => Ok(unit_value(literal.value, literal.unit)?),
        Expr::Bool(value, _) => Ok(Value::Bool(*value)),
        Expr::Null(_) => Ok(Value::Null),
        Expr::Str(literal) => {
            let mut text = String::new();
            for part in &literal.parts {
                match part {
                    StrPart::Text { text: raw, .. } => text.push_str(raw),
                    StrPart::Expr(inner) => {
                        let value = eval_expr(session, inner, source)?;
                        text.push_str(&text_of(&value)?);
                    }
                }
            }
            Ok(Value::String(text.into()))
        }
        Expr::Ip(literal) => {
            // The lexer keeps the address as written; the value model is what knows how to read
            // one, so a malformed address is a `type.mismatch` here rather than a lexer rule.
            let address: std::net::IpAddr = literal
                .text
                .split('%')
                .next()
                .unwrap_or(&literal.text)
                .parse()
                .map_err(|_| {
                    Flow::Failed(ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("`{}` is not an IP address", literal.text),
                    ))
                })?;
            Ok(Value::Ip(address))
        }
        Expr::Timestamp(literal) => Ok(Value::parse_timestamp(&literal.text)?),
        Expr::Regex(literal) => {
            // Flags become an inline group, which is how the regex engine spells them and keeps
            // the pattern one thing rather than a pattern plus a side channel.
            let pattern = if literal.flags.is_empty() {
                literal.pattern.clone()
            } else {
                format!("(?{}){}", literal.flags, literal.pattern)
            };
            Ok(Value::Regex(std::sync::Arc::new(
                ono_value::RegexValue::new(&pattern)?,
            )))
        }
        Expr::Variable(variable) => Ok(lookup_variable(session, &variable.name)),
        Expr::Path(path) => Ok(lookup_variable(session, &path.name)),
        Expr::List(list) => {
            let mut items = Vec::with_capacity(list.items.len());
            for item in &list.items {
                items.push(eval_expr(session, item, source)?);
            }
            Ok(Value::List(items.into()))
        }
        Expr::Record(record) => {
            let mut map = MapValue::new();
            for field in &record.fields {
                let key = match &field.key {
                    ono_parser::RecordKey::Ident { name, .. } => name.clone(),
                    ono_parser::RecordKey::Str(literal) => {
                        string_of(session, &Expr::Str(literal.clone()), source)?
                    }
                };
                let value = eval_expr(session, &field.value, source)?;
                map.insert(key.into(), value);
            }
            Ok(Value::Map(std::sync::Arc::new(map)))
        }
        Expr::Paren(inner) => match &inner.inner {
            ono_parser::ParenInner::Expr(expression) => eval_expr(session, expression, source),
            ono_parser::ParenInner::Pipeline(pipeline) => {
                value_of_pipeline(session, pipeline, source)
            }
        },
        Expr::Unary(unary) => {
            let operand = eval_expr(session, &unary.operand, source)?;
            Ok(match unary.op {
                UnaryOp::Not => match operand {
                    Value::Null => Value::Null,
                    other => Value::Bool(!truthy(&other)),
                },
                UnaryOp::Neg => Value::Int(0).sub(&operand)?,
            })
        }
        Expr::Binary(binary) => eval_binary(session, binary, source),
        Expr::Field(access) => {
            let base = eval_expr(session, &access.base, source)?;
            let step = if access.optional {
                ono_value::FieldStep::optional(&access.field)
            } else {
                ono_value::FieldStep::required(&access.field)
            };
            Ok(base.follow(&[step])?)
        }
        Expr::Index(index) => {
            let base = eval_expr(session, &index.base, source)?;
            let key = eval_expr(session, &index.index, source)?;
            Ok(index_into(&base, &key)?)
        }
        Expr::Call(call) => {
            // `now()` is the one builtin function language.yaml declares (spec §6.3, ADR-0071).
            if ono_command::is_now_call(call) {
                return Ok(Value::now());
            }
            Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::ResolveCommandNotFound,
                    format!("no function to call at {}", call.span),
                )
                .with_help(
                    "`now()` is the only function an expression can call; a user function is \
                     called as a command (spec §19.3, ADR-0070)",
                ),
            ))
        }
        Expr::CurrentValue(current) => match current.selector {
            // Spec §20.2: previous structured results are reusable without screen scraping. A
            // list splices when it starts a pipeline (ADR-0019), so `@-1 | where …` streams the
            // retained rows.
            ono_parser::CurrentSelector::Previous(n) => session
                .previous_result(n)
                .map(|values| Value::list(values.to_vec()))
                .ok_or_else(|| {
                    Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            format!("no result to reuse at {}", current.span),
                        )
                        .with_help(
                            "`@-1` names the previous pipeline's values (spec §20.2), \
                                    and nothing has produced any yet",
                        ),
                    )
                }),
            ono_parser::CurrentSelector::Item(n) => session
                .previous_result(1)
                .and_then(|values| values.get(n.checked_sub(1)? as usize))
                .cloned()
                .ok_or_else(|| {
                    Flow::Failed(
                        ErrorValue::new(
                            ErrorCode::ResolveTargetNotFound,
                            format!("no item {n} in the current result at {}", current.span),
                        )
                        .with_help("`@N` names row N of the last shown result (spec §6.4)"),
                    )
                }),
            // The item an enclosing block is iterating shadows the interactive selection for
            // the block's duration (spec §19.4, ADR-0071 §1).
            ono_parser::CurrentSelector::Current if session.binding("@").is_some() => {
                Ok(session.binding("@").cloned().unwrap_or(Value::Null))
            }
            ono_parser::CurrentSelector::Current => session.selection().cloned().ok_or_else(|| {
                Flow::Failed(
                    ErrorValue::new(
                        ErrorCode::ResolveTargetNotFound,
                        format!("there is no current value at {}", current.span),
                    )
                    .with_help(
                        "`@` names the item a block is iterating, or the row a view left \
                             selected; neither exists here (spec §6.4, §19.4, ADR-0050)",
                    ),
                )
            }),
        },
        Expr::Block(_) => Ok(Value::Null),
        Expr::Error(span) => Err(Flow::Failed(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("this expression could not be read at {span}"),
        ))),
    }
}

fn lookup_variable(session: &Session, name: &str) -> Value {
    // The status of the last statement, under the name every shell user already knows and under
    // a name they can discover (ADR-0019).
    if name == "?" || name == "status" {
        return Value::Int(i128::from(session.status().code()));
    }
    if let Some(variable) = name.strip_prefix("env.") {
        return session.env_var(variable).map_or(Value::Null, |value| {
            Value::String(value.to_string_lossy().into_owned().into())
        });
    }
    // `$env` is the environment as a record, so `$env.PATH` is an ordinary field access and the
    // environment is inspectable as data rather than only readable one name at a time (ADR-0010).
    if name == "env" && session.binding("env").is_none() {
        let mut map = MapValue::new();
        for (key, value) in session.env() {
            map.insert(
                key.to_string_lossy().into_owned().into(),
                Value::String(value.to_string_lossy().into_owned().into()),
            );
        }
        return Value::Map(std::sync::Arc::new(map));
    }
    if let Some(value) = session.binding(name) {
        return value.clone();
    }
    session.env_var(name).map_or(Value::Null, |value| {
        Value::String(value.to_string_lossy().into_owned().into())
    })
}

fn number_value(value: NumberValue) -> Value {
    match value {
        NumberValue::Int(int) => Value::Int(i128::from(int)),
        NumberValue::Float(float) => Value::Float(float),
    }
}

fn unit_value(value: NumberValue, unit: Unit) -> Result<Value, ErrorValue> {
    let magnitude = match value {
        NumberValue::Int(int) => int as f64,
        NumberValue::Float(float) => float,
    };
    let suffix = unit.as_str();
    if let Some(byte_unit) = ono_value::ByteUnit::from_suffix(suffix) {
        let bytes = magnitude * byte_unit.factor() as f64;
        if !bytes.is_finite() || bytes < 0.0 {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("{magnitude}{suffix} is not a byte size"),
            ));
        }
        return Ok(Value::ByteSize(ByteSize::from_bytes(bytes as u128)));
    }
    if let Some(time_unit) = ono_value::DurationUnit::from_suffix(suffix) {
        let nanoseconds = magnitude * time_unit.nanoseconds() as f64;
        if !nanoseconds.is_finite() {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("{magnitude}{suffix} is not a duration"),
            ));
        }
        return Ok(Value::Duration(OnoDuration::from_nanoseconds(
            nanoseconds as i128,
        )));
    }
    Ok(Value::Percent(Percent::new(magnitude)))
}

/// Evaluates an infix operator with the three-valued semantics ADR-0014 freezes.
fn eval_binary(
    session: &mut Session,
    binary: &ono_parser::BinaryExpr,
    source: &str,
) -> Eval<Value> {
    // `and` and `or` short-circuit, so a decided result is not made unknown by the other operand.
    match binary.op {
        BinaryOp::And => {
            let left = eval_expr(session, &binary.lhs, source)?;
            if matches!(left, Value::Bool(false)) {
                return Ok(Value::Bool(false));
            }
            let right = eval_expr(session, &binary.rhs, source)?;
            return Ok(kleene_and(&left, &right));
        }
        BinaryOp::Or => {
            let left = eval_expr(session, &binary.lhs, source)?;
            if matches!(left, Value::Bool(true)) {
                return Ok(Value::Bool(true));
            }
            let right = eval_expr(session, &binary.rhs, source)?;
            return Ok(kleene_or(&left, &right));
        }
        _ => {}
    }

    let left = eval_expr(session, &binary.lhs, source)?;
    let right = eval_expr(session, &binary.rhs, source)?;

    // `x == null` is an identity test, not a three-valued comparison (ADR-0014): without the
    // exception the commonest question anyone asks would silently match nothing.
    if matches!(binary.op, BinaryOp::Eq | BinaryOp::NotEq)
        && (matches!(&binary.lhs, Expr::Null(_)) || matches!(&binary.rhs, Expr::Null(_)))
    {
        // One side is the literal `null`, so the question is only whether the other side is.
        let other = if matches!(&binary.lhs, Expr::Null(_)) {
            &right
        } else {
            &left
        };
        let is_null = matches!(other, Value::Null);
        return Ok(Value::Bool(match binary.op {
            BinaryOp::Eq => is_null,
            _ => !is_null,
        }));
    }

    if matches!(left, Value::Null) || matches!(right, Value::Null) {
        return Ok(Value::Null);
    }

    Ok(match binary.op {
        BinaryOp::Eq => Value::Bool(equals(&left, &right)),
        BinaryOp::NotEq => Value::Bool(!equals(&left, &right)),
        BinaryOp::Lt => Value::Bool(left.compare_to(&right)?.is_lt()),
        BinaryOp::LtEq => Value::Bool(left.compare_to(&right)?.is_le()),
        BinaryOp::Gt => Value::Bool(left.compare_to(&right)?.is_gt()),
        BinaryOp::GtEq => Value::Bool(left.compare_to(&right)?.is_ge()),
        BinaryOp::In => Value::Bool(contains(&right, &left)),
        BinaryOp::NotIn => Value::Bool(!contains(&right, &left)),
        BinaryOp::Match => Value::Bool(regex_matches(&right, &left)?),
        BinaryOp::NotMatch => Value::Bool(!regex_matches(&right, &left)?),
        BinaryOp::Add => left.add(&right)?,
        BinaryOp::Sub => left.sub(&right)?,
        BinaryOp::Mul => left.mul(&right)?,
        BinaryOp::Div => left.div(&right)?,
        BinaryOp::Rem => remainder(&left, &right)?,
        BinaryOp::And | BinaryOp::Or => Value::Null,
    })
}

fn kleene_and(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Bool(false), _) | (_, Value::Bool(false)) => Value::Bool(false),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Bool(truthy(left) && truthy(right)),
    }
}

fn kleene_or(left: &Value, right: &Value) -> Value {
    match (left, right) {
        (Value::Bool(true), _) | (_, Value::Bool(true)) => Value::Bool(true),
        (Value::Null, _) | (_, Value::Null) => Value::Null,
        _ => Value::Bool(truthy(left) || truthy(right)),
    }
}

/// Whether a value counts as true where a condition is wanted.
///
/// Only `true` is true. `null` is unknown and therefore not true, which is what makes `where`
/// admit only decided matches (ADR-0014).
#[must_use]
pub fn truthy(value: &Value) -> bool {
    matches!(value, Value::Bool(true))
}

fn equals(left: &Value, right: &Value) -> bool {
    if left == right {
        return true;
    }
    left.compare_to(right).is_ok_and(std::cmp::Ordering::is_eq)
}

fn contains(haystack: &Value, needle: &Value) -> bool {
    match haystack {
        Value::List(items) => items.iter().any(|item| equals(item, needle)),
        Value::String(text) => {
            ono_value::canonical_text(needle).is_ok_and(|needle| text.contains(&needle))
        }
        _ => false,
    }
}

fn regex_matches(pattern: &Value, subject: &Value) -> Result<bool, ErrorValue> {
    let regex = pattern.as_regex()?;
    let text = ono_value::canonical_text(subject)?;
    Ok(regex.is_match(&text))
}

fn remainder(left: &Value, right: &Value) -> Result<Value, ErrorValue> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) if *b != 0 => Ok(Value::Int(a % b)),
        (Value::Int(_), Value::Int(_)) => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            "the remainder of a division by zero is undefined",
        )),
        _ => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "`%` needs two integers, got {} and {}",
                left.type_name(),
                right.type_name()
            ),
        )),
    }
}

fn index_into(base: &Value, key: &Value) -> Result<Value, ErrorValue> {
    match (base, key) {
        (Value::List(items), Value::Int(index)) => {
            let index = usize::try_from(*index).map_err(|_| {
                ErrorValue::new(ErrorCode::TypeMismatch, "a list index cannot be negative")
            })?;
            Ok(items.get(index).cloned().unwrap_or(Value::Null))
        }
        (Value::Map(map), Value::String(key)) => Ok(map.get(key).cloned().unwrap_or(Value::Null)),
        (Value::Record(record), Value::String(field)) => {
            Ok(record.get(field).cloned().unwrap_or(Value::Null))
        }
        _ => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!(
                "{} cannot be indexed by {}",
                base.type_name(),
                key.type_name()
            ),
        )),
    }
}
