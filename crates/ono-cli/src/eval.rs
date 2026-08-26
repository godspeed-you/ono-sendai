//! The evaluator: from a parsed program to what actually happens.
//!
//! The execution model is ADR-0013's. Phase A carries only external stages, so a pipeline becomes
//! an `ono_process::Pipeline` and runs in the foreground; the native stages of phase B slot in
//! beside them without changing the shape of anything here.

use std::ffi::OsString;
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
use crate::session::{Mode, Session};

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
            let value = value_of_pipeline(session, &binding.value, source)?;
            session.bind(binding.name.clone(), value);
            Ok(ExitStatus::SUCCESS)
        }
        Statement::If(branch) => run_if(session, branch, source),
        Statement::While(loop_) => run_while(session, loop_, source),
        Statement::For(loop_) => run_for(session, loop_, source),
        Statement::Match(match_) => run_match(session, match_, source),
        Statement::Try(try_) => run_try(session, try_, source),
        Statement::Fn(declaration) => {
            // A function is a binding whose value is its body, so it lives in the same scope
            // chain as everything else and `fn:` resolution has one place to look.
            session.bind(
                format!("fn:{}", declaration.name),
                Value::String(source_of(source, declaration.span).into()),
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

fn source_of(source: &str, span: Span) -> &str {
    span.of(source)
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

    if session.mode() == Mode::Config {
        return Err(Flow::Failed(config_refusal("this command")));
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
        Ok(Resolution::Builtin(builtin)) => Some(builtin),
        _ => None,
    }
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

    let namespace = Namespace::from_prefix(name.namespace.as_deref()).ok_or_else(|| {
        Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!(
                    "unknown namespace `{}`",
                    name.namespace.as_deref().unwrap_or_default()
                ),
            )
            .with_help("the namespaces are `ono:`, `exec:` and `fn:` (ADR-0011)"),
        )
    })?;

    let namespace =
        if namespace == Namespace::Any && resolve::BUILTINS.contains(&name.name.as_str()) {
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

    let mut command = Command::new(resolve::program_of(&resolution))
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

/// The value a pipeline produces when it is used as one, as in `let x = …`.
fn value_of_pipeline(session: &mut Session, pipeline: &Pipeline, source: &str) -> Eval<Value> {
    // A pipeline whose only stage is a bare value is that value: `let name = "world"`.
    if pipeline.tail.is_empty()
        && pipeline.head.stages.len() == 1
        && let Some(stage) = pipeline.head.stages.first()
        && stage.arguments.is_empty()
        && stage.redirections.is_empty()
        && let StageHead::Value(expression) = &stage.head
    {
        return eval_expr(session, expression, source);
    }
    // An expression-mode stage with no arguments and a bare head is a field path or a literal
    // read as a command; `let n = 3` arrives this way.
    if pipeline.tail.is_empty()
        && pipeline.head.stages.len() == 1
        && let Some(stage) = pipeline.head.stages.first()
        && stage.redirections.is_empty()
        && stage.arguments.len() == 1
        && let StageHead::Error(_) = &stage.head
        && let Some(Argument::Value(expression)) = stage.arguments.first()
    {
        return eval_expr(session, expression, source);
    }

    let status = run_pipeline(session, pipeline, source)?;
    Ok(Value::Int(i128::from(status.code())))
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
        Expr::Call(call) => Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("no function to call at {}", call.span),
            )
            .with_help("user functions arrive with the module system of spec §19.6"),
        )),
        Expr::CurrentValue(current) => Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("there is no current value at {}", current.span),
            )
            .with_help(
                "`@` names the item a block is iterating, or the interactive selection; neither \
                 exists here (spec §6.4, §19.4)",
            ),
        )),
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
