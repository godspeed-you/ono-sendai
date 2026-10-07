//! `explain`: the one plan builder behind every answer `explain` gives (ADR-0942).
//!
//! Spec §15.3 and §42 want the resolution and the execution plan of a whole pipeline: which
//! command each stage resolves to, which provider and capability it will use, its input and
//! output schemas, whether it streams, and what it would change. ADR-0011 requires the order to be
//! reported by the code that performs it rather than described somewhere that could drift from it,
//! so everything the shell knows and the registry does not — aliases, prefix assignments, user
//! functions, the link a frame stands in, which program a word finds on `PATH` — is folded into the
//! same `ono.execution-plan/1` record here. A terminal shows that record's rendering; a pipeline
//! receives the record itself.

use ono_core::ErrorCode;
use ono_parser::{Argument, Expr, Stage, StageList};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

/// Whether the subject of an `explain` stage is delimited — one quoted string, one `{ … }` block
/// or one variable — so that `explain` is a producer whose plan flows into the stages after it.
///
/// Anything else is spec §11.3's unquoted form, whose subject is the rest of the pipeline.
#[must_use]
pub fn is_delimited(stage: &Stage) -> bool {
    matches!(
        stage.arguments.as_slice(),
        [Argument::Value(
            Expr::Str(_) | Expr::Block(_) | Expr::Variable(_)
        )]
    )
}

/// The subject of an `explain` stage that stands at the head of `list`, as text.
///
/// A block is its source between the braces; a string or a variable is what it evaluates to, as
/// any argument is. The unquoted form of a single stage is its words as the shell expands them,
/// globs included; across pipes it is the source from `explain`'s first argument to the end of
/// the list, handed over verbatim — never re-rendered from the AST, which would explain a
/// normalisation of what the user typed rather than what they typed.
///
/// # Errors
///
/// The error evaluating a string or a variable produced.
pub fn subject(session: &mut Session, list: &StageList, source: &str) -> Eval<String> {
    let Some(stage) = list.stages.first() else {
        return Ok(String::new());
    };
    if let [Argument::Value(Expr::Block(block))] = stage.arguments.as_slice() {
        let text = block.span.of(source).trim();
        let inner = text
            .strip_prefix('{')
            .and_then(|text| text.strip_suffix('}'))
            .unwrap_or(text);
        return Ok(inner.trim().to_owned());
    }
    // A subject with no pipe of its own is its words as the shell expands them, so a glob names
    // the files it resolves to — spec §17.3's "knows its exact targets before mutating" is what
    // the plan of `explain remove file *.tmp` must show.
    if is_delimited(stage) || list.stages.len() == 1 {
        let words = crate::eval::stage_arguments(session, stage, source)?;
        return Ok(words
            .iter()
            .map(|word| word.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" "));
    }
    let start = stage
        .arguments
        .first()
        .map_or(stage.span.end(), |argument| argument.span().start());
    let end = list
        .stages
        .last()
        .map_or(stage.span.end(), |last| last.span.end());
    Ok(source
        .get(start as usize..end as usize)
        .unwrap_or_default()
        .trim()
        .to_owned())
}

/// The plan of `subject`, as `ono.execution-plan/1`, without running any part of it.
///
/// # Errors
///
/// An empty subject, one that is not a pipeline, a stage of a tier this build was compiled
/// without (ADR-0911), a sealed plan that does not exist (ADR-0814), and whatever evaluating a
/// prefix assignment's value refuses.
pub fn plan_value(session: &mut Session, subject: &str) -> Eval<Value> {
    plan(session, subject).map(|plan| plan.to_value())
}

/// The plan of `subject`, made with everything the session knows.
fn plan(session: &mut Session, subject: &str) -> Eval<ono_command::ExecutionPlan> {
    if subject.trim().is_empty() {
        return Err(Flow::Failed(
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "`explain` needs something to explain",
            )
            .with_help("`explain \"get process | where cpu > 20\"` — quote the pipeline"),
        ));
    }

    // v0.6 §5's list of what an operator may ask includes "why does it say that", and a sealed
    // plan is where that question lands: `explain plan a82f` shows the provider bindings §4.4
    // sealed, the risk rules §19.2 fired with the sentence each carries, and the coverage
    // reasoning Appendix A produced (ADR-0814).
    if let Some(lines) = crate::change::explanation(session, subject).map_err(Flow::Failed)? {
        return Ok(ono_command::ExecutionPlan::of_change_plan(subject, lines));
    }

    // The order execution resolves a stage list in (ADR-0011, ADR-0070): a prefix assignment is
    // stripped by the function running the line strips it with (spec §54, ADR-0943), then an
    // alias is expanded once and the expansion resolved again from the top.
    let mut source = subject.to_owned();
    let mut aliases: Vec<(String, String)> = Vec::new();
    let mut environment: Vec<(String, String)> = Vec::new();
    let pipeline = loop {
        let parsed = ono_parser::parse(&source);
        let Some(mut pipeline) = parsed
            .program()
            .statements
            .first()
            .and_then(ono_parser::Statement::as_pipeline)
            .cloned()
        else {
            return Err(Flow::Failed(ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("`{source}` is not a pipeline"),
            )));
        };
        if let Some((assignments, stripped)) =
            crate::eval::prefix_assignments(session, &pipeline.head, &source)?
        {
            environment.extend(
                assignments
                    .into_iter()
                    .map(|(name, value)| (name, value.to_string_lossy().into_owned())),
            );
            pipeline.head = stripped;
        }
        if let Some((name, text)) = crate::eval::expand_alias(session, &pipeline.head, &source)
            && !aliases.iter().any(|(expanded, _)| *expanded == name)
        {
            let expansion = session
                .alias(&name)
                .map(|alias| alias.expansion.clone())
                .unwrap_or_default();
            aliases.push((name, expansion));
            source = text;
            continue;
        }
        break pipeline;
    };

    // A stage of a compiled-out tier has no plan to report: `explain` says what running it would
    // say (ADR-0911).
    if let Some(refusal) = pipeline.head.stages.iter().find_map(crate::absent::claims) {
        return Err(Flow::Failed(refusal));
    }

    let registry = crate::eval::native::registry().map_err(Flow::Failed)?;
    // The last stage's consumer is whatever the shell's stdout is, and a plan that assumed a
    // terminal would promise interactive rendering to a script (spec v0.3 §1.4).
    let stdout = if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        ono_adapter::Stdout::Terminal
    } else {
        ono_adapter::Stdout::Stream
    };
    // What each program resolves to is looked up once, up front: the plan needs it to ask the
    // adapter registry (spec v0.3 §1.23), and the plan quotes it.
    let resolved: std::collections::BTreeMap<String, Option<std::path::PathBuf>> = pipeline
        .head
        .stages
        .iter()
        .filter_map(program_word)
        .map(|name| (name.to_owned(), crate::resolve::find_on_path(session, name)))
        .collect();
    let executables = |name: &str| resolved.get(name).cloned().flatten();
    // Step 2 of the resolution order outranks the registry, so the plan is told which heads are
    // user functions and whether a call of each can stream where it stands (ADR-0951).
    let functions: std::collections::BTreeMap<String, ono_command::FunctionPlan> = pipeline
        .head
        .stages
        .iter()
        .filter_map(|stage| stage.head.name())
        .filter_map(|name| {
            let function = session.function(name)?;
            Some((
                name.to_owned(),
                ono_command::FunctionPlan {
                    declared: function.declaration.span.to_string(),
                    at_head: crate::eval::native::function_shape(session, &function, false),
                    with_input: crate::eval::native::function_shape(session, &function, true),
                },
            ))
        })
        .collect();
    let function_plans = |name: &str| functions.get(name).cloned();
    // Inside a link frame the remote negotiates (spec v0.3 §1.54): the local registry is not
    // consulted for the plan, and the remote's answer is reported per stage below.
    let remote_host = session.link_host();
    // Everything a frame contributes has an explicit spelling, and `explain` is where it is
    // written (spec §14.5, ADR-0023, ADR-0225).
    let frames = session.context();
    let limits = crate::limits::materialization(session.settings());
    let (providers, adapters) = session.registries();
    let adapters = remote_host.is_none().then_some(adapters);
    let mut plan = ono_command::plan_with(
        registry,
        Some(providers),
        &pipeline,
        &source,
        &ono_command::PlanContext {
            stdout,
            adapters,
            executables: Some(&executables),
            context: &frames,
            // v0.4.1 §22.4: the plan shows the budget the pipeline would really run under, so a
            // user who narrowed `limits.materialize_bytes` sees their own figure.
            limits,
            functions: Some(&function_plans),
        },
    );
    plan.set_subject(subject.trim());
    for (name, expansion) in aliases {
        plan.push_alias(name, expansion);
    }
    for (name, value) in environment {
        plan.push_environment(name, value);
    }

    // Spec §42.2: while connected, the plan shows the execution context, so the risk of acting
    // on the wrong machine is inspectable (ADR-0106).
    #[cfg(feature = "remote")]
    if let Some(host) = &remote_host
        && let Some(link) = session.link(host)
    {
        let answers = link.agentless.then(|| {
            let answered = link
                .connection
                .as_ref()
                .map(crate::session::LinkConnection::targets)
                .unwrap_or_default();
            if answered.is_empty() {
                "nothing — this link was never established".to_owned()
            } else {
                answered.join(" ")
            }
        });
        plan.set_context(vec![
            ("host".to_owned(), Some(host.clone())),
            ("link".to_owned(), Some(format!("{host} (remote)"))),
            ("transport".to_owned(), Some(link.transport.clone())),
            (
                "mode".to_owned(),
                Some(if link.agentless {
                    "agentless — a reduced provider set over standard commands (spec §21.3)"
                        .to_owned()
                } else {
                    "agent".to_owned()
                }),
            ),
            // Spec §21.3: what a reduced link *can* answer is the visible half of what it cannot.
            ("answers".to_owned(), answers),
            (
                "identity".to_owned(),
                Some(session.env_var("USER").map_or_else(
                    || "unknown".to_owned(),
                    |user| user.to_string_lossy().into_owned(),
                )),
            ),
        ]);
    }

    // A mutation says what it does, not only which capability it needs (spec §42.2).
    for (stage, planned) in pipeline.head.stages.iter().zip(plan.stages_mut()) {
        if planned.risk().is_some_and(|risk| risk.changes_the_world()) {
            planned.set_operation(mutation_operation(registry, stage));
        }
    }

    #[cfg(feature = "remote")]
    if let Some(host) = &remote_host {
        let agentless = session.link(host).is_some_and(|link| link.agentless);
        let demands: Vec<Option<ono_adapter::OutputDemand>> = plan
            .stages()
            .iter()
            .map(|planned| planned.demand().cloned())
            .collect();
        for (index, stage) in pipeline.head.stages.iter().enumerate() {
            let Some(Some(demand)) = demands.get(index) else {
                continue;
            };
            if ono_command::is_raw(stage) {
                continue;
            }
            let Some(argv) = crate::eval::native::literal_argv(stage) else {
                continue;
            };
            let state = crate::eval::native::remote_decision(session, &argv, demand).map_or_else(
                || {
                    if agentless {
                        "raw (this link is agentless: there is no agent over there to negotiate \
                         adapters)"
                            .to_owned()
                    } else {
                        "raw (the remote agent cannot negotiate adapters)".to_owned()
                    }
                },
                |decision| decision.state,
            );
            if let Some(planned) = plan.stages_mut().get_mut(index) {
                planned.set_remote_adaptation(state);
            }
        }
    }

    // A stage the registry does not know is an external program, and which one it will be is the
    // half of the answer the registry cannot give (ADR-0011 T11: a shadowing binary is only
    // defensible if the shell will say which one it picked).
    for (position, stage) in pipeline.head.stages.iter().enumerate() {
        let Some(name) = program_word(stage) else {
            continue;
        };
        // A head the registry knows as a verb or as a command id is native, and the plan
        // already says everything there is to say about it.
        if registry.verb(name).is_some() || registry.get(name).is_some() {
            continue;
        }
        // A user function is step 2 of the order (ADR-0011, ADR-0070): it wins over the
        // registry and over PATH, and the plan says so instead of describing what it shadows.
        if let Some(function) = session.function(name) {
            // v0.4.1 §26.2: where a call cannot be continued as a stage of the pipeline it stands
            // in, "that limitation MUST be explicit in `explain`" (ADR-0481).
            let continuation = if position == 0 {
                match crate::eval::native::function_shape(session, &function, false) {
                    Ok(()) => "its body streams into the stages after the call".to_owned(),
                    Err(_) => "its result is collected before the stages after the call run, so its input must be finite".to_owned(),
                }
            } else {
                match crate::eval::native::function_shape(session, &function, true) {
                    Ok(()) => "its body reads the stream in front of the call and streams into the stages after it".to_owned(),
                    Err(reason) => format!("it cannot read the stream in front of it: {reason}"),
                }
            };
            let declared = function.declaration.span;
            plan.push_note(format!(
                "  `{name}` is a user function declared at {declared} — step 2 of the resolution order; {continuation} (v0.4.1 §26.2)"
            ));
            continue;
        }
        // The registry does not know the shell's own commands, so without this `explain cd`
        // would report that `cd` resolves to nothing — which is both false and exactly the kind
        // of thing `explain` exists to get right (ADR-0011).
        if crate::resolve::BUILTINS.contains(&name) {
            plan.push_note(format!(
                "  `{name}` is a command the shell runs itself — step 4 of the resolution order"
            ));
            continue;
        }
        match resolved.get(name).cloned().flatten() {
            Some(path) => {
                let path = path.display().to_string();
                plan.push_note(format!(
                    "  `{name}` is an external program and resolves to {path}"
                ));
                if let Some(planned) = plan.stages_mut().get_mut(position) {
                    planned.set_path(path);
                }
            }
            None => plan.push_note(format!("  `{name}` resolves to nothing on PATH")),
        }
    }
    Ok(plan)
}

/// The word a stage runs: the program behind `raw` or `adapt`, or its head.
fn program_word(stage: &Stage) -> Option<&str> {
    ono_command::raw_program(stage)
        .or_else(|| ono_command::adapt_program(stage))
        .or_else(|| stage.head.name())
}

/// What a mutating stage does, in the words of spec §42.2 (`signal TERM`): the effect, not the
/// capability. The signal commands name their signal; any other mutation is its verb and
/// target.
fn mutation_operation(registry: &ono_command::CommandRegistry, stage: &Stage) -> String {
    let Some(verb) = stage.head.name() else {
        return "unknown".to_owned();
    };
    let Ok(resolved) = registry.resolve(verb, &stage.arguments) else {
        return verb.to_owned();
    };
    let contract = resolved.contract;
    let bound = contract.bind(resolved.arguments).ok();
    let signal = |name: &str| {
        bound
            .as_ref()
            .and_then(|bound| bound.option(name).or_else(|| bound.selector(name)))
            .filter(|value| !matches!(value, Value::Null))
            .map(ToString::to_string)
            .or_else(|| {
                contract
                    .option(name)
                    .and_then(|option| option.default_text().map(str::to_owned))
            })
    };
    match contract.id() {
        // `stop` is graceful termination with default TERM semantics (process.yaml).
        "ono.process.stop" => "signal TERM".to_owned(),
        "ono.process.kill" | "ono.signal.send" => format!(
            "signal {}",
            signal("signal").unwrap_or_else(|| "SIGKILL".to_owned())
        ),
        _ => format!(
            "{} {}",
            contract.verb(),
            contract.target().unwrap_or_default()
        ),
    }
}
