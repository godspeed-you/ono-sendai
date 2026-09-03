//! Running the object pipeline of spec §5 in the shell itself.
//!
//! A pipeline is a sequence of stages, and each one is either a child process or a native command
//! the registry declares. This module decides which is which, then runs the native ones and hands
//! the rest to [`ono_process`], threading the boundary of spec §12.3 between them.
//!
//! The boundary is explicit in both directions, as spec §12.3 requires. Bytes become objects only
//! through a command that says so — `from json` — and objects become bytes only through `to` or
//! `format`. A structured stream aimed at a child process is a type error naming `to json`
//! (ADR-0013), never a silent rendering the receiving program would have to parse back.

use std::io::Write;
use std::sync::OnceLock;

use ono_adapter::OutputDemand;
use ono_command::{CommandRegistry, CommandTable};
use ono_core::{ErrorCode, ExitStatus};
use ono_parser::StageList;
use ono_pipeline::{StreamEvent, ValueStream};
use ono_value::{ErrorValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

mod bind;
mod drive;
mod external;
mod foreground;
mod remote;
mod result;
mod segment;

pub(crate) use bind::stream_segment;
pub(crate) use drive::run_background;
pub(crate) use remote::{literal_argv, remote_decision};
pub(crate) use result::live_geometry;
pub(crate) use segment::{adapts_at_terminal, claims, continuable_body, continuable_list};

use self::external::{
    decode_adapted, external_demand, negotiate_stage, report_fallback, run_streamed_segment,
    seed_bytes,
};
use self::foreground::run_native_segment;
use self::remote::{RemoteRun, remote_argv, run_remote_adapted};
use self::result::{write_failed, write_result};
use self::segment::{Segment, consumer_needing_objects, segments, unstructured_refusal};

/// The command contracts, parsed once from the copies embedded at compile time.
///
/// # Errors
///
/// The structured error the registry raises when an embedded contract cannot be read. That is a
/// build-time mistake rather than a user's, but it is reported rather than panicked over: a shell
/// that aborts on startup teaches nobody anything.
pub fn registry() -> Result<&'static CommandRegistry, ErrorValue> {
    crate::plugin_registry::registry()
}

/// The native implementations, built once against the registry and the shell's own providers.
///
/// A mutating command is bound when a local provider advertises its capability (ADR-0068 §3).
/// The table is built from the local providers whatever frame the shell is in, so a link frame
/// neither adds nor removes commands; the mutation asks the provider that actually acts.
fn implementations(session: &mut Session) -> Result<&'static CommandTable, ErrorValue> {
    static TABLE: OnceLock<CommandTable> = OnceLock::new();
    if let Some(table) = TABLE.get() {
        return Ok(table);
    }
    // §26.3 and §34.2 make the landmark thresholds and the map's node budget configurable, and
    // §47 names the settings. They are read here, once, because this is the only point where the
    // shell's resolved configuration and the process-wide spatial state (§29.2) meet.
    crate::spatial::configure_from(session.settings());
    let mut built = ono_command::builtin_commands_for(registry()?, session.providers());
    // The spatial commands of v0.4 §6 are the shell's to dispatch (§45.6): they need the host and
    // boot the session belongs to, which no library crate can know. Selection, ranking and
    // identity stay in `ono-spatial-query` and `ono-spatial-index`, where §45.2 and §45.3 put
    // them (ADR-0141).
    built.register(std::sync::Arc::new(crate::spatial::FindPlace::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Look::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Near::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Enter::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Follow::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Map::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::MapLinks::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Home));
    built.register(std::sync::Arc::new(crate::spatial::Back));
    built.register(std::sync::Arc::new(crate::spatial::Up));
    built.register(std::sync::Arc::new(crate::spatial::Jump::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::Trail));
    built.register(std::sync::Arc::new(crate::spatial::PinPlace::new(
        crate::spatial::PinStore::of(session),
    )));
    built.register(std::sync::Arc::new(crate::spatial::UnpinPlace::new(
        crate::spatial::PinStore::of(session),
    )));
    Ok(TABLE.get_or_init(|| built))
}

/// Checks every expression in `pipeline` against the schema that would reach it.
///
/// Spec §11.3: a typo in a field name is caught before process enumeration begins, because the
/// contracts declare what flows where. Everything here is declarative — nothing is enumerated and
/// nothing is spawned — so the check costs nothing when the pipeline is sound. A pipeline whose
/// schemas are unknown is not checked rather than guessed at.
///
/// # Errors
///
/// `type.unknown_field` naming the field, the schema, and the nearest declared field.
pub fn check(
    session: &mut Session,
    pipeline: &ono_parser::Pipeline,
    source: &str,
) -> Result<(), ErrorValue> {
    let Ok(registry) = registry() else {
        // An unreadable registry is reported where a native stage actually runs; the pre-flight
        // check is an optimisation of the failure path, not a second gate.
        return Ok(());
    };
    // A pipeline of external programs alone has no contract to check against, so it is not
    // planned: planning asks for the providers, and building those starts the runtime and
    // connects to the service manager — most of what `ono -c 'echo ready'` cost (spec §34).
    let has_native_stage = std::iter::once(&pipeline.head)
        .chain(pipeline.tail.iter().map(|chained| &chained.list))
        .flat_map(|list| list.stages.iter())
        .any(|stage| {
            stage
                .head
                .name()
                .is_some_and(|head| registry.resolve(head, &stage.arguments).is_ok())
        });
    if !has_native_stage {
        return Ok(());
    }
    let schemas: Vec<_> = ono_value::builtin_schemas().schemas().cloned().collect();
    // A program an adapter gives a schema is a producer like any other (spec v0.3 §1.61,
    // ADR-0067): the plan says which stages those are, so the check reaches the stages after
    // them. Inside a link frame the remote decides, and nothing is known here.
    let adapted = if session.link_host().is_some() {
        Vec::new()
    } else {
        let materialization = crate::eval::materialize::limits(session);
        let resolve = crate::resolve::resolver(session);
        let executables = |name: &str| resolve(name);
        let (providers, adapters) = session.registries();
        ono_command::plan_with(
            registry,
            Some(providers),
            pipeline,
            source,
            &ono_command::PlanContext {
                stdout: ono_adapter::Stdout::Stream,
                adapters: Some(adapters),
                executables: Some(&executables),
                context: &[],
                limits: materialization,
            },
        )
        .adapted_schemas()
    };
    ono_command::check_pipeline_with(registry, &schemas, pipeline, &adapted)
}

/// Runs a stage list that contains at least one native command.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub fn run(session: &mut Session, list: &StageList, source: &str) -> Eval<ExitStatus> {
    run_from(session, list, source, 0, Start::Nothing)
}

/// Runs the stages of `list` after the first over a stream a function body is already producing.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub(crate) fn run_piped(
    session: &mut Session,
    list: &StageList,
    source: &str,
    stream: ValueStream,
    failed_rows: bool,
) -> Eval<ExitStatus> {
    run_from(
        session,
        list,
        source,
        1,
        Start::Pipe {
            stream,
            failed_rows,
        },
    )
}

/// Runs a native pipeline and answers its values instead of showing them.
///
/// What `… | enter socket` needs (spec §14.3, ADR-0075): the object the stages before `enter`
/// produced, without a table on the way. Nothing is retained for `@-1` either — a result that
/// was never shown is not one the user can point at.
///
/// # Errors
///
/// Exactly what [`run`] reports.
pub fn run_collecting(session: &mut Session, list: &StageList, source: &str) -> Eval<Vec<Value>> {
    session.begin_capture();
    let outcome = run_from(session, list, source, 0, Start::Nothing);
    let captured = session.end_capture();
    outcome?;
    Ok(captured)
}

/// Runs a stage list whose head has already produced values.
///
/// A pipeline may start with a value instead of a command — `$hot | where …`, `@-1 | count` —
/// and a list splices because it *is* several values (ADR-0019). The evaluator has already
/// turned the head into `seed`; everything after it runs exactly as if a native producer had
/// streamed those values.
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub fn run_seeded(
    session: &mut Session,
    list: &StageList,
    source: &str,
    seed: Vec<Value>,
) -> Eval<ExitStatus> {
    run_from(session, list, source, 1, Start::Values(seed))
}

/// Runs the stages of `list` from `start` on, seeded with values the evaluator already has —
/// what an `each { … }` block produced for the stages after it (ADR-0071 §1).
///
/// # Errors
///
/// The structured error of whichever stage could not be resolved, bound, or run.
pub fn run_seeded_from(
    session: &mut Session,
    list: &StageList,
    source: &str,
    start: usize,
    seed: Vec<Value>,
) -> Eval<ExitStatus> {
    run_from(session, list, source, start, Start::Values(seed))
}

/// What a run of `list` starts from, when it does not start from its own head.
pub(crate) enum Start {
    /// Nothing: the head stage is a producer.
    Nothing,
    /// Values the evaluator already has — a retained result, a plugin's answer.
    Values(Vec<Value>),
    /// A stream another pipeline is already producing into: v0.4.1 §26.2's streaming
    /// continuation, where a function body is a stage of the caller's pipeline rather than a
    /// collection in front of it (ADR-0481).
    Pipe {
        /// The assembled but undrained stream of the pipeline this run continues.
        stream: ValueStream,
        /// Whether a mutation in that pipeline reported a failed row (spec §16.5, ADR-0006).
        failed_rows: bool,
    },
}

impl Start {
    /// Whether structure — rather than the head stage's own output — reaches the first stage.
    fn is_some(&self) -> bool {
        !matches!(self, Self::Nothing)
    }

    /// Takes the start, leaving nothing behind.
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Nothing)
    }
}

fn run_from(
    session: &mut Session,
    list: &StageList,
    source: &str,
    start: usize,
    seed: Start,
) -> Eval<ExitStatus> {
    let registry = registry().map_err(Flow::Failed)?;
    let segments = segments(session, list, start, seed.is_some()).ok_or_else(|| {
        Flow::Failed(ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "the command registry could not be read",
        ))
    })?;

    let mut carried: Option<Vec<u8>> = None;
    let mut seed = seed;
    let mut status = ExitStatus::SUCCESS;

    // A seeded list with nothing after the seed shows the seed itself: `@-1` alone re-renders
    // the retained result.
    if segments.is_empty() {
        if let Start::Values(values) = seed.take()
            && let Some(stage) = list.stages.first()
        {
            write_result(session, stage, &values, false, source)?;
        }
        return Ok(status);
    }

    let mut position = 0;
    while position < segments.len() {
        let segment = &segments[position];
        let last = position + 1 == segments.len();
        match segment {
            Segment::External(indices) => {
                match seed.take() {
                    // Spec §12.3: objects reach a child process only through an explicit
                    // representation. Text and bytes already are one.
                    Start::Values(values) => carried = Some(seed_bytes(values)?),
                    Start::Pipe { .. } => {
                        return Err(Flow::Failed(
                            ErrorValue::new(
                                ErrorCode::TypeMismatch,
                                "this stage produces objects, and the next one is a program that \
                                 reads bytes",
                            )
                            .with_help("choose the representation: `… | to json | …` (spec §12.3)"),
                        ));
                    }
                    Start::Nothing => {}
                }
                let demand = external_demand(
                    session,
                    registry,
                    list,
                    indices,
                    segments.get(position + 1),
                    last,
                );
                if let Some(demand_kind) = demand.as_ref()
                    && session.link_host().is_some()
                    && matches!(
                        demand_kind,
                        OutputDemand::Structured { .. } | OutputDemand::Interactive
                    )
                    && let Some(stage) = indices.last().map(|index| &list.stages[*index])
                    && !ono_command::is_raw(stage)
                    && let Some(argv) = remote_argv(session, stage, source)?
                {
                    // Inside a link frame the adapter and the executable are the remote's
                    // (spec v0.3 §1.54, ADR-0066): the agent negotiates, runs and decodes,
                    // and the records arrive already marked with the host.
                    let following: &[usize] = match segments.get(position + 1) {
                        Some(Segment::Native(next)) => next,
                        _ => &[],
                    };
                    let consumed_next = !following.is_empty();
                    match run_remote_adapted(
                        session,
                        list,
                        following,
                        source,
                        &argv,
                        demand_kind,
                        last || (consumed_next && position + 2 == segments.len()),
                    )? {
                        RemoteRun::Adapted(status_after) => {
                            status = status_after;
                            carried = None;
                            position += if consumed_next { 2 } else { 1 };
                            continue;
                        }
                        RemoteRun::NotAdapted(reason) => {
                            // The remote has nothing for this invocation: say so, and let the
                            // program run as it always has — locally, raw.
                            if matches!(demand_kind, OutputDemand::Interactive) {
                                eprintln!(
                                    "{}",
                                    ono_render::sanitise(&format!(
                                        "{}: {reason}; running `{}` locally",
                                        session.link_host().unwrap_or_default(),
                                        argv.join(" ")
                                    ))
                                );
                            }
                        }
                    }
                }
                if let Some((plan, argv, demand)) =
                    negotiate_stage(session, list, indices, source, demand)?
                {
                    session.note_adaptation(plan.adapter().full_id(), plan.argv().join(" "));
                    if plan.adapter().decoder().kind().streams() {
                        // Records flow while the child runs (ADAPT-005, ADR-0059): the
                        // following native segment — or the renderer — consumes them as they
                        // arrive, so both segments are run here and the loop skips ahead.
                        let following: &[usize] = match segments.get(position + 1) {
                            Some(Segment::Native(next)) => next,
                            _ => &[],
                        };
                        let consumed_next = !following.is_empty();
                        status = run_streamed_segment(
                            session,
                            registry,
                            list,
                            indices,
                            following,
                            source,
                            carried.take(),
                            &plan,
                            &argv,
                            last || (consumed_next && position + 2 == segments.len()),
                        )?;
                        carried = None;
                        position += if consumed_next { 2 } else { 1 };
                        continue;
                    }
                    let (bytes, external_status) = crate::eval::run_adapted_segment(
                        session,
                        list,
                        indices,
                        source,
                        carried.take(),
                        &plan,
                    )?;
                    status = external_status;
                    let stage = &list.stages[*indices.last().unwrap_or(&0)];
                    // A child that failed has failed, whatever it wrote (spec v0.3 §1.20,
                    // ADR-0057 point 3): its output is not decoded, its status stands.
                    if external_status != ExitStatus::SUCCESS {
                        let program = plan
                            .executable()
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        return Err(Flow::FailedWith(
                            ErrorValue::new(
                                ErrorCode::ExternalExitNonzero,
                                format!("{program} exited with status {}", external_status.code()),
                            )
                            .with_help(format!(
                                "its stderr above says why; `raw {}` runs it as typed",
                                argv.join(" ")
                            )),
                            external_status,
                        ));
                    }
                    match decode_adapted(session, &plan, &argv, &bytes) {
                        Ok(values) => {
                            // v0.4 §37: a typed object an adapter decoded may contribute to the
                            // spatial model. The batch is in hand here and nowhere else, so this
                            // is where it is offered (ADR-0193).
                            if !crate::spatial::disabled(session)
                                && let Some((runtime, _)) = session.pipeline_context()
                            {
                                runtime.block_on(crate::spatial::observe_adapted(&values));
                            }
                            if last {
                                write_result(session, stage, &values, false, source)?;
                            } else {
                                seed = Start::Values(values);
                            }
                        }
                        Err(error) => {
                            // At the terminal a decode failure falls back to the bytes the
                            // tool already wrote, never to a second run (spec v0.3 §1.57,
                            // ADR-0057 point 7). A structured consumer gets the error.
                            if demand == OutputDemand::Interactive
                                && plan.adapter().fallback() == ono_adapter::Fallback::Raw
                            {
                                report_fallback(&plan, &error);
                                let mut out = std::io::stdout().lock();
                                out.write_all(&bytes).map_err(write_failed)?;
                                out.flush().map_err(write_failed)?;
                            } else {
                                return Err(Flow::Failed(error));
                            }
                        }
                    }
                    position += 1;
                    continue;
                }
                // Spec §12.3 in its other direction: a stage declared over objects cannot be
                // fed the bytes a program wrote. No adapter turned this invocation into
                // objects, so there are none — and the contracts say so before anything is
                // spawned, which is why `yes | take 1` answers at all (ADR-0376).
                if let Some(consumer) =
                    consumer_needing_objects(session, registry, list, segments.get(position + 1))
                {
                    return Err(Flow::Failed(unstructured_refusal(
                        list, indices, source, consumer,
                    )));
                }
                let (bytes, external_status) = crate::eval::run_external_segment(
                    session, list, indices, source, carried, last,
                )?;
                carried = bytes;
                status = external_status;
            }
            Segment::Native(indices) => {
                let (bytes, native_status) = run_native_segment(
                    session,
                    registry,
                    list,
                    indices,
                    source,
                    carried,
                    match seed.take() {
                        Start::Nothing => Seed::None,
                        Start::Values(values) => Seed::Values(values),
                        Start::Pipe {
                            stream,
                            failed_rows,
                        } => Seed::Pipe {
                            stream,
                            failed_rows,
                        },
                    },
                    position == 0,
                    last,
                )?;
                carried = bytes;
                status = native_status;
            }
        }
        position += 1;
    }
    Ok(status)
}

/// What starts a native segment's stream besides carried bytes.
enum Seed {
    /// Nothing: the head stage is a producer, or reads the carried bytes.
    None,
    /// Values already in hand — a retained result, a plugin's answer, a decoded document.
    Values(Vec<Value>),
    /// A stream another pipeline assembled: v0.4.1 §26.2's streaming continuation (ADR-0481).
    Pipe {
        stream: ValueStream,
        failed_rows: bool,
    },
    /// Values arriving from a reader thread while the child that produces them still runs.
    Stream {
        receiver: tokio::sync::mpsc::Receiver<StreamEvent>,
        boundedness: ono_pipeline::Boundedness,
    },
}

impl Seed {
    fn is_some(&self) -> bool {
        !matches!(self, Self::None)
    }
}
