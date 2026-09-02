//! A stage that runs on a remote agent (spec v0.4 §31, ADR-0118).

use ono_adapter::OutputDemand;
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::{Stage, StageHead, StageList};
use ono_pipeline::StreamEvent;
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::resolve::Namespace;
use crate::session::Session;

use super::foreground::run_native_segment;
use super::segment::adaptable_program;
use super::{Seed, registry};

/// Negotiates a stage from its source text alone, for decisions made before anything runs.
///
/// Segmentation and the terminal check cannot evaluate arguments — a `$(…)` argument would run
/// twice — so they ask with the words as written. Execution asks again with the expanded words;
/// where the two differ, the run's answer is the one that counts.
/// What the remote side of a link frame would do with an invocation (spec v0.3 §1.54).
pub(crate) struct RemoteDecision {
    pub(crate) adapted: bool,
    pub(crate) state: String,
}

/// Asks the innermost link's agent to negotiate `argv` for `demand` without running it.
///
/// `None` when the session is not inside a link frame or the agent cannot answer — an older
/// agent, a lost link — in which case the caller falls back to local semantics and says so.
pub(crate) fn remote_decision(
    session: &mut Session,
    argv: &[String],
    demand: &OutputDemand,
) -> Option<RemoteDecision> {
    let demand_name = match demand {
        OutputDemand::Structured { .. } => "structured",
        OutputDemand::Interactive => "interactive",
        _ => return None,
    };
    let handle = session.runtime()?.handle().clone();
    let link = session.remote_link()?.agent_link()?;
    let mut stream = link.adapt(argv, demand_name, true).ok()?;
    handle.block_on(async {
        while let Some(message) = stream.recv().await {
            match message {
                ono_protocol::RemoteMessage::Value(Value::Map(map)) => {
                    return Some(RemoteDecision {
                        adapted: matches!(map.get("adapted"), Some(Value::Bool(true))),
                        state: map
                            .get("state")
                            .and_then(|state| state.as_str().ok())
                            .unwrap_or("unknown")
                            .to_owned(),
                    });
                }
                ono_protocol::RemoteMessage::Failure(error) => {
                    return Some(RemoteDecision {
                        adapted: false,
                        state: format!("raw ({})", error.message()),
                    });
                }
                _ => {}
            }
        }
        None
    })
}

/// The words a stage would hand its program, from the source alone, program first.
pub(crate) fn literal_argv(stage: &Stage) -> Option<Vec<String>> {
    let StageHead::Command(name) = &stage.head else {
        return None;
    };
    let words = literal_words(stage);
    if ono_command::is_adapt(stage) {
        let mut iter = words.into_iter();
        let program = iter.next()?;
        let mut argv = vec![program];
        argv.extend(iter);
        return Some(argv);
    }
    let mut argv = vec![name.name.clone()];
    argv.extend(words);
    Some(argv)
}

pub(super) fn negotiate_literally(
    session: &mut Session,
    stage: &Stage,
    demand: &OutputDemand,
) -> Option<ono_adapter::Negotiation> {
    // Inside a link frame the remote decides (spec v0.3 §1.54); its answer is folded into the
    // two states the callers here distinguish — a plan, or not.
    if session.link_host().is_some()
        && !ono_command::is_raw(stage)
        && let Some(argv) = literal_argv(stage)
        && let Some(decision) = remote_decision(session, &argv, demand)
    {
        return Some(if decision.adapted {
            ono_adapter::Negotiation::RemoteAdapted {
                state: decision.state,
            }
        } else {
            ono_adapter::Negotiation::RawPreferred {
                reason: decision.state,
            }
        });
    }
    let (name, path) = adaptable_program(session, stage)?;
    let mut argv = vec![name];
    let words = literal_words(stage);
    argv.extend(if ono_command::is_adapt(stage) {
        words.into_iter().skip(1).collect::<Vec<String>>()
    } else {
        words
    });
    Some(session.adapters().negotiate(&path, &argv, demand))
}

/// A stage's arguments as written, where the source text is not at hand: words and options
/// are themselves; an expression is a placeholder the matcher treats as a positional.
pub(super) fn literal_words(stage: &Stage) -> Vec<String> {
    stage
        .arguments
        .iter()
        .map(|argument| match argument {
            ono_parser::Argument::Word(word) => word.text.clone(),
            ono_parser::Argument::Option(option) => match &option.value {
                Some(_) => format!("--{}=", option.name),
                None => format!("--{}", option.name),
            },
            _ => "$expression".to_owned(),
        })
        .collect()
}

/// What a remote adaptation came to.
pub(super) enum RemoteRun {
    /// The remote adapted the invocation and its records were consumed; the status stands.
    Adapted(ExitStatus),
    /// The remote has no adapter for it, with the reason it gave.
    NotAdapted(String),
}

/// The invocation a stage would run, with its arguments expanded, program first; `None` for a
/// stage that is not a program.
pub(super) fn remote_argv(
    session: &mut Session,
    stage: &Stage,
    source: &str,
) -> Eval<Option<Vec<String>>> {
    let StageHead::Command(name) = &stage.head else {
        return Ok(None);
    };
    if !matches!(
        Namespace::from_prefix(name.namespace.as_deref()),
        Some(Namespace::Any | Namespace::External)
    ) {
        return Ok(None);
    }
    let forced = ono_command::is_adapt(stage);
    let words: Vec<String> = crate::eval::stage_arguments(session, stage, source)?
        .into_iter()
        .map(|word| word.to_string_lossy().into_owned())
        .collect();
    let mut argv = Vec::new();
    if forced {
        let Some(program) = words.first() else {
            return Ok(None);
        };
        argv.push(program.clone());
        argv.extend(words.into_iter().skip(1));
    } else {
        argv.push(name.name.clone());
        argv.extend(words);
    }
    Ok(Some(argv))
}

/// Runs an invocation through the remote's adapter (spec v0.3 §1.54): the records stream from
/// the agent into the following native segment — or the renderer — exactly as a locally
/// streamed adaptation would (ADR-0059), already marked with the host.
///
/// # Errors
///
/// The remote's refusal under a structured demand, as the structured error it sent.
pub(super) fn run_remote_adapted(
    session: &mut Session,
    list: &StageList,
    following: &[usize],
    source: &str,
    argv: &[String],
    demand: &OutputDemand,
    last: bool,
) -> Eval<RemoteRun> {
    let registry = registry().map_err(Flow::Failed)?;
    let demand_name = match demand {
        OutputDemand::Interactive => "interactive",
        _ => "structured",
    };
    // Ask first without running: the remote's own decision is what decides whether the
    // program runs there adapted or here raw.
    let Some(decision) = remote_decision(session, argv, demand) else {
        let reason = if session
            .remote_link()
            .is_some_and(crate::session::LinkConnection::is_agentless)
        {
            // Spec §21.3: the reduction stays visible wherever it changes what a command means.
            "this link is agentless: there is no agent over there to negotiate adapters"
        } else {
            "the remote agent cannot negotiate adapters"
        };
        return Ok(RemoteRun::NotAdapted(reason.to_owned()));
    };
    if !decision.adapted {
        if matches!(demand, OutputDemand::Structured { .. }) {
            let host = session.link_host().unwrap_or_default();
            return Err(Flow::Failed(
                ErrorValue::new(
                    ErrorCode::AdapterNotAvailable,
                    format!("{host}: {}", decision.state),
                )
                .with_help(format!(
                    "`raw {}` runs the program as typed; a structured consumer needs an \
                     adapter on the host the frame stands on (spec v0.3 §1.54)",
                    argv.join(" ")
                ))
                .with_metadata("invocation", Value::string(&argv.join(" ")))
                .with_metadata("raw_fallback_safe", Value::Bool(true)),
            ));
        }
        return Ok(RemoteRun::NotAdapted(decision.state));
    }
    let Some(runtime_handle) = session
        .runtime()
        .map(tokio::runtime::Runtime::handle)
        .cloned()
    else {
        return Err(Flow::Failed(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the pipeline runtime",
        )));
    };
    let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(256);
    let link = session
        .remote_link()
        .and_then(crate::session::LinkConnection::agent_link)
        .ok_or_else(|| {
            Flow::Failed(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                "the link is gone",
            ))
        })?;
    let mut stream = link.adapt(argv, demand_name, false).map_err(Flow::Failed)?;
    let host: std::sync::Arc<str> = std::sync::Arc::from(link.host());
    runtime_handle.spawn(async move {
        while let Some(message) = stream.recv().await {
            let event = match message {
                ono_protocol::RemoteMessage::Value(value) => {
                    StreamEvent::Value(ono_remote::retag_value(value, &host))
                }
                ono_protocol::RemoteMessage::Failure(error) => StreamEvent::Failure(error),
                ono_protocol::RemoteMessage::Event(_) => continue,
            };
            if sender.send(event).await.is_err() {
                break;
            }
        }
    });
    let (_, status) = run_native_segment(
        session,
        registry,
        list,
        following,
        source,
        None,
        Seed::Stream {
            receiver,
            boundedness: ono_pipeline::Boundedness::Bounded,
        },
        false,
        last,
    )?;
    let host = session.link_host().unwrap_or_default();
    session.note_adaptation(format!("{} on {host}", decision.state), argv.join(" "));
    Ok(RemoteRun::Adapted(status))
}
