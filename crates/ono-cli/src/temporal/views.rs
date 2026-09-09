//! The event views: `timeline`, `changes`, `why`, `find event` and `inspect event`
//! (spec v0.5 §11, §13, §16, §20.3, §11.6).
//!
//! Each command here is thin by design. It reads its arguments, borrows the session's coordinate
//! and ledger, hands a request to `ono-temporal-query`, and turns the answer into the records the
//! schemas of §35 declare. Planning, ranking, grouping and causal reasoning are the query crate's
//! (§39, §55.7); drawing them is `ono-temporal-render`'s, reached through `crate::sink` (§39.3).

use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_temporal_core::{EventKind, TemporalContext, TimeRange, error};
use ono_temporal_query::causal::{CausalContext, CausalEngine, WhyOptions, WhyRequest};
use ono_temporal_query::changes::ChangesRequest;
use ono_temporal_query::relevance::Horizon;
use ono_temporal_query::search::{EventReferences, SearchHints};
use ono_temporal_query::timeline::TimelineRequest;
use ono_value::{ErrorValue, Value};

use super::session::{TemporalState, temporal_session};

/// How many events `find event` answers with when the user names no limit.
const FIND_LIMIT: usize = 500;

/// Reads a `--since`/`--until` option: a duration before the coordinate, or a time selector.
///
/// §11.2 writes `--since 30m` and §13.1 writes `--since 12:00`, so both have to work. A bare
/// duration is measured back from `anchor`; anything else is one of §4.4's five selector forms,
/// resolved by the same code `at` uses, because §4.5 forbids a second historical code path.
fn instant_of(
    state: &TemporalState,
    given: Option<&Value>,
    anchor: Timestamp,
) -> Result<Option<Timestamp>, ErrorValue> {
    let Some(value) = given else {
        return Ok(None);
    };
    let text = match value {
        Value::Null => return Ok(None),
        Value::Duration(span) => {
            let magnitude = span.nanoseconds().abs();
            return Timestamp::from_nanosecond(anchor.as_nanosecond().saturating_sub(magnitude))
                .map(Some)
                .map_err(|_| {
                    error::invalid_time(
                        &span.to_string(),
                        "that span reaches outside recorded time",
                    )
                });
        }
        Value::String(text) => text.to_string(),
        other => ono_value::canonical_text(other).unwrap_or_else(|_| other.type_name().to_owned()),
    };
    // `--since 30m` arrives as a word when the option is typed as a string. A duration written
    // without a sign means "this long ago" here, which is what §11.2's `--since 30m` says.
    if let Ok(span) = ono_value::Duration::parse(text.trim()) {
        let magnitude = span.nanoseconds().abs();
        return Timestamp::from_nanosecond(anchor.as_nanosecond().saturating_sub(magnitude))
            .map(Some)
            .map_err(|_| error::invalid_time(&text, "that span reaches outside recorded time"));
    }
    super::coordinate::resolve_instant(state, &text, anchor).map(Some)
}

/// The relevance horizon a command with no selector uses: the current place and its neighbours
/// (§11.3).
async fn horizon_of(all: bool) -> Horizon {
    let scope = crate::spatial::local_scope();
    if all {
        return Horizon::everything(scope);
    }
    let spatial = crate::spatial::spatial_session().await;
    let place = spatial.current_place().clone();
    // §11.3: "At the root system place, it shows high-significance events and current-session
    // actions rather than dumping every event from every object." The root horizon is what says
    // that, and it is keyed on significance rather than on identity — which is also what lets an
    // action with no spatial target appear at all.
    if place == ono_spatial_core::space::root().spatial_id_in(Some(&scope)) {
        return Horizon::at_root(scope);
    }
    let neighbours = ono_spatial_query::resolve::parent_of(spatial.index(), &place)
        .into_iter()
        .collect();
    Horizon::at_place(scope, place, neighbours)
}

/// `timeline` (§11).
#[derive(Debug)]
pub struct Timeline;

impl CommandImpl for Timeline {
    fn id(&self) -> &str {
        "ono.temporal.timeline"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("timeline"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let now = Timestamp::now();
            let mut state = temporal_session().await;
            let context = state.context().clone();
            let anchor = context.instant().unwrap_or(now);

            let mut request = TimelineRequest::new(horizon_of(arguments.flag("all")).await);
            request.since = instant_of(&state, arguments.option("since"), anchor)?;
            request.until = instant_of(&state, arguments.option("until"), anchor)?;
            for kind in kinds_of(&arguments)? {
                request.kinds.push(kind);
            }

            let timeline = {
                let ledger = state.ledger_handle();
                ono_temporal_query::timeline::timeline(ledger.as_ref(), &request, &context, now)?
            };
            let timeline = timeline.with_references(state.references());
            let record = timeline.to_record()?;
            // §11.5's default rendering is a row per event, so `RenderOptions::group_repeats`
            // stays off in `crate::sink`. §19.4's grouping belongs to the full-screen timeline of
            // §19, which has its own invocation and its own key handling.
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(record),
            )])))
        })
    }
}

/// The `--kind` values, refused by name rather than silently ignored (§6.1).
fn kinds_of(arguments: &ono_command::BoundArguments) -> Result<Vec<EventKind>, ErrorValue> {
    let mut kinds = Vec::new();
    let given = match arguments.option("kind") {
        Some(Value::List(items)) => items.to_vec(),
        Some(Value::Null) | None => Vec::new(),
        Some(other) => vec![other.clone()],
    };
    for value in given {
        let Ok(name) = value.as_str() else {
            continue;
        };
        let kind = EventKind::from_name(name).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("`{name}` is not one of v0.5 §6.1's event kinds"),
            )
            .with_help(format!(
                "the kinds are {}",
                EventKind::ALL
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        kinds.push(kind);
    }
    Ok(kinds)
}

/// `changes` (§13).
#[derive(Debug)]
pub struct Changes;

impl CommandImpl for Changes {
    fn id(&self) -> &str {
        "ono.temporal.changes"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("changes"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let now = Timestamp::now();
            let state = temporal_session().await;
            let anchor = state.context().instant().unwrap_or(now);

            // §13.1 makes `--since` required: a comparison needs two ends, and defaulting one of
            // them would make the answer depend on a figure nobody named.
            let since =
                instant_of(&state, arguments.option("since"), anchor)?.ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        "`changes` needs `--since`, the instant to compare against",
                    )
                    .with_help("`changes --since 10m` or `changes --since 12:00` (v0.5 §13.1)")
                })?;
            let mut request = ChangesRequest::new(crate::spatial::local_scope(), since);
            request.until = instant_of(&state, arguments.option("until"), anchor)?;

            let ledger = state.ledger_handle();
            let changes = ono_temporal_query::changes::changes(ledger.as_ref(), &request, anchor)?;
            let mut values = Vec::with_capacity(changes.len());
            for change in &changes {
                values.push(Value::Record(Arc::new(change.to_record()?)));
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// `why` (§16).
#[derive(Debug)]
pub struct Why;

impl CommandImpl for Why {
    fn id(&self) -> &str {
        "ono.temporal.why"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("why"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let now = Timestamp::now();
            let subject = words_of(&arguments, "subject");
            if subject.is_empty() {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    "`why` needs what to explain",
                )
                .with_help(
                    "§16.2 defines `why <target> <selector>`, `why event @e42` and `why field \
                     <name>`",
                ));
            }
            let mut state = temporal_session().await;
            let anchor = state.context().instant().unwrap_or(now);
            let request = why_request(&mut state, &subject).await?;

            let ledger = state.ledger_handle();
            let window = TimeRange::until(anchor);
            let events = ledger.events(&ono_temporal_core::EventQuery {
                scope: Some(crate::spatial::local_scope()),
                subjects: Vec::new(),
                kinds: Vec::new(),
                range: window,
                limit: None,
                order: ono_temporal_core::QueryOrder::Ascending,
            })?;
            let evidence_ids: Vec<ono_temporal_core::EvidenceId> = events
                .iter()
                .flat_map(|event| event.evidence.iter().cloned())
                .collect();
            let causal = CausalContext::new(ledger.evidence(&evidence_ids)?);
            let mut options = WhyOptions::at(anchor);
            if let Some(Value::Int(depth)) = arguments.option("depth") {
                options = options.with_depth(usize::try_from(*depth).unwrap_or(usize::MAX));
            }
            let explanation =
                CausalEngine::builtin().explain(&request, &events, &causal, &options)?;
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(explanation.to_record()?),
            )])))
        })
    }
}

/// §16.2's three forms, read from the words after `why`.
async fn why_request(
    state: &mut TemporalState,
    subject: &[String],
) -> Result<WhyRequest, ErrorValue> {
    match subject.split_first() {
        Some((head, rest)) if head == "event" => {
            let reference = rest.first().map(String::as_str).unwrap_or_default();
            let ledger = state.ledger_handle();
            let event = state
                .references()
                .resolve(reference, ledger.as_ref())?
                .ok_or_else(|| {
                    error::invalid_time(reference, "no event with that reference is retained")
                })?;
            Ok(WhyRequest::event(&event.event_id))
        }
        Some((head, rest)) if head == "field" => {
            let field = rest.first().map(String::as_str).unwrap_or_default();
            let spatial = crate::spatial::spatial_session().await;
            let place = spatial.current_place().clone();
            let label = ono_spatial_query::resolve::concise_path(spatial.index(), &place);
            Ok(WhyRequest::field(&place, &label, field))
        }
        Some((_, _)) => {
            // `why service nginx`: the words are a spatial selector, and resolving one is v0.4's.
            let spelling = subject.join(" ");
            let spatial = crate::spatial::spatial_session().await;
            let place = spatial.current_place().clone();
            let label = subject.last().cloned().unwrap_or(spelling);
            Ok(WhyRequest::target(&place, &label))
        }
        None => Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            "`why` needs what to explain",
        )),
    }
}

/// The repeated words of a selector, as the user wrote them.
fn words_of(arguments: &ono_command::BoundArguments, name: &str) -> Vec<String> {
    match arguments.selector(name) {
        Some(Value::List(items)) => items
            .iter()
            .filter_map(|item| item.as_str().ok().map(str::to_owned))
            .collect(),
        Some(Value::String(text)) => vec![text.to_string()],
        _ => Vec::new(),
    }
}

/// `find event` (§20.3).
#[derive(Debug)]
pub struct FindEvent;

impl CommandImpl for FindEvent {
    fn id(&self) -> &str {
        "ono.event.find"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("find event"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let now = Timestamp::now();
            let mut state = temporal_session().await;
            let anchor = state.context().instant().unwrap_or(now);

            let hints = SearchHints {
                scope: Some(crate::spatial::local_scope()),
                subjects: Vec::new(),
                kinds: Vec::new(),
                range: TimeRange {
                    from: instant_of(&state, arguments.option("since"), anchor)?,
                    until: Some(
                        instant_of(&state, arguments.option("until"), anchor)?.unwrap_or(anchor),
                    ),
                },
                limit: Some(match arguments.option("limit") {
                    Some(Value::Int(limit)) => usize::try_from(*limit).unwrap_or(FIND_LIMIT),
                    _ => FIND_LIMIT,
                }),
                order: ono_temporal_core::QueryOrder::Ascending,
            };
            let events = {
                let ledger = state.ledger_handle();
                ono_temporal_query::search::find_events(ledger.as_ref(), &hints)?
            };

            // §20.3 reuses Ono expression semantics rather than inventing a search language, so
            // the predicate is compiled by the parser and evaluated by `ono-command` against each
            // event record — exactly what `where` would do one stage later.
            let predicate = arguments
                .selector("predicate")
                .and_then(|value| value.as_str().ok())
                .map(str::to_owned);
            let compiled = match predicate.as_deref() {
                Some(text) if !text.trim().is_empty() => Some(compile(text)?),
                _ => None,
            };

            let scope = ono_command::Scope::new();
            let mut values = Vec::with_capacity(events.len());
            for event in &events {
                let reference = state.references().reference(event);
                let record = ono_temporal_core::value::event_record_with_reference(
                    event,
                    Some(reference.as_str()),
                )?;
                let value = Value::Record(Arc::new(record));
                let admitted = match &compiled {
                    Some(expression) => ono_command::is_true(&ono_command::evaluate_to_value(
                        expression, &value, &scope,
                    )),
                    None => true,
                };
                if admitted {
                    values.push(value);
                }
            }
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// Compiles the quoted predicate of `find event` into an expression (§20.3).
fn compile(text: &str) -> Result<ono_parser::Expr, ErrorValue> {
    let source = format!("where {text}");
    let parsed = ono_parser::parse(&source);
    if let Some(first) = parsed.diagnostics().first() {
        return Err(ErrorValue::new(
            ErrorCode::ParseSyntax,
            format!("`{text}` is not an expression: {}", first.message()),
        )
        .with_help("`find event 'kind == \"action.failed\"'` (v0.5 §20.3)"));
    }
    parsed
        .program()
        .statements
        .first()
        .and_then(ono_parser::Statement::as_pipeline)
        .and_then(|pipeline| pipeline.head.stages.first())
        .and_then(|stage| stage.arguments.first())
        .and_then(ono_parser::Argument::as_value)
        .cloned()
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("`{text}` is not an expression over an event"),
            )
        })
}

/// `inspect event` (§11.6, §7.3).
#[derive(Debug)]
pub struct InspectEvent;

impl CommandImpl for InspectEvent {
    fn id(&self) -> &str {
        "ono.event.inspect"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("inspect event"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let reference = arguments
                .selector("reference")
                .and_then(|value| value.as_str().ok())
                .unwrap_or_default()
                .to_owned();
            let mut state = temporal_session().await;
            let ledger = state.ledger_handle();
            let event = state
                .references()
                .resolve(&reference, ledger.as_ref())?
                .ok_or_else(|| {
                    error::invalid_time(&reference, "no event with that reference is retained")
                })?;
            let record = ono_temporal_core::value::event_record_with_reference(
                &event,
                Some(reference.trim_start_matches('@')),
            )?;
            Ok(Outcome::Values(ValueStream::from_values([Value::Record(
                Arc::new(record),
            )])))
        })
    }
}

/// The references a session has issued, for completion (§20.4).
pub async fn issued_references() -> EventReferences {
    let mut state = temporal_session().await;
    state.references().clone()
}

/// Whether the session is standing in the past, for a caller that only needs the fact.
#[must_use]
pub fn is_historical() -> bool {
    matches!(
        super::session::coordinate().as_ref(),
        TemporalContext::Historical { .. }
    )
}
