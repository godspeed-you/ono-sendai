//! The spatial commands the shell dispatches: `look`, `near`, `enter` and `home`
//! (spec v0.4 §6.1, §6.2, §6.3, §6.6, §29.1, §45.6).
//!
//! Each one is thin on purpose. It reads its arguments, borrows the session's place and index
//! (§46), lets `ono-spatial-query` decide what to show, turns the answer into records of the
//! contracts `docs/spec/schemas/` declares, and stops. Ranking, bounding, selector resolution and
//! identity are the library crates' (§45.2, §45.3); rendering is `ono-spatial-render`'s (§45.4).

use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture};
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_provider_api::ProviderRegistry;
use ono_spatial_core::{Movement, NavigationStep, PermissionState, SpatialType};
use ono_spatial_query::{NeighborhoodRequest, SelectorContext};
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value, builtin_schemas};

use crate::spatial::session::{SpatialSessionState, spatial_session};
use crate::spatial::view;

/// The provider id the spatial layer signs its own composition with (ADR-0140).
pub(crate) const COMPOSER: &str = "ono.spatial";

/// Reads the session's pins from the store, once per command that needs them (§46.1).
pub(crate) async fn with_pins(
    session: &mut SpatialSessionState,
    store: Option<&crate::spatial::PinStore>,
    now: Timestamp,
) -> Result<(), ErrorValue> {
    let Some(store) = store else {
        return Ok(());
    };
    let mut pins = store.load()?;
    for (name, id) in crate::spatial::pins::resolved_pins(&pins, session.index(), now) {
        pins.rebind(&name, id);
    }
    session.set_pins(pins);
    Ok(())
}

/// `look` (spec v0.4 §6.1, §24).
#[derive(Debug)]
pub struct Look {
    pins: Option<crate::spatial::PinStore>,
}

impl Look {
    /// The implementation registered against `ono.place.look`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self { pins }
    }
}

impl CommandImpl for Look {
    fn id(&self) -> &str {
        "ono.place.look"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("look"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            // A words-mode command that reads values resolves its arguments first: `--type
            // ["process"]`, `--limit (1 + 1)` and `--changed (1h)` are expressions until
            // something evaluates them, and an option nobody evaluated reads as an option nobody
            // wrote (ADR-0219, ADR-0556).
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let arguments = &arguments;
            let all = arguments.flag("all");
            let json = arguments.flag("json");
            let now = Timestamp::now();

            let mut session = spatial_session().await;
            let changes = window_of(arguments.option("changes"), session.preferences());
            with_pins(&mut session, self.pins.as_ref(), now).await?;
            // §6.1: `look` describes the place and its immediate horizon. `--all` widens the
            // exits — every group lists the places behind it — rather than dumping the object's
            // properties, which §24.1 reserves for `inspect`.
            let request = NeighborhoodRequest::new().all(all);
            let (neighborhood, permission, cached, whole) = view::neighborhood_and_whole(
                ctx.providers(),
                &mut session,
                &request,
                changes.is_some(),
                now,
            )
            .await?;
            // §10.3: a tombstone shows what took the old object's place. The candidate cannot be
            // known when the object ends — no source that reached it has been observed since — so
            // it is asked for when the tombstone is *rendered*, and therefore after the
            // observation that discovers the place has gone (ADR-0273).
            let here = session.current_place().clone();
            if session.tombstone_of(&here, now).is_some() {
                crate::spatial::relations::resolve_replacement(
                    ctx.providers(),
                    &mut session,
                    &here,
                    now,
                )
                .await;
            }
            // §25.4: where no event stream answers, a change is the difference between two
            // observations — and there is one only from the second `look --changes` of a session
            // onwards. The baseline is taken whether or not the caller asked, so the *next* ask
            // has something to compare to.
            let compared = changes.map(|window| {
                let observed = whole.as_ref().unwrap_or(&neighborhood);
                let snapshot = ono_spatial_events::PlaceSnapshot::of(session.index(), observed);
                let here = session.current_place().clone();
                let before = session.rebase(&here, snapshot.clone());
                (
                    window,
                    before.map(|before| {
                        ono_spatial_events::compare_places(
                            &before,
                            &snapshot,
                            ono_spatial_events::Freshness::Polled,
                        )
                    }),
                )
            });
            let view = place_view(
                &session,
                &neighborhood,
                permission,
                all,
                compared,
                cached,
                now,
            )?;

            if json {
                let document = ono_value::to_json_data(&Value::Record(Arc::new(view)));
                // One document on one line, exactly as `to json` writes one (v0.2 §33.5): a
                // script reads it with `from json`, and `--pretty` is the reader's choice, not
                // the command's.
                let text = serde_json::to_string(&document).map_err(|error| {
                    ErrorValue::new(
                        ErrorCode::TypeMismatch,
                        format!("the place view could not be written as JSON: {error}"),
                    )
                })?;
                return Ok(Outcome::Values(ValueStream::from_values(vec![
                    Value::string(&text),
                ])));
            }
            Ok(Outcome::Values(ValueStream::from_values(vec![
                Value::Record(Arc::new(view)),
            ])))
        })
    }
}

/// The `ono.place-view/1` record of the current place (§6.1, §24.1).
fn place_view(
    session: &SpatialSessionState,
    neighborhood: &ono_spatial_core::Neighborhood,
    permission: PermissionState,
    all: bool,
    changes: Option<(ono_value::Duration, Option<ono_spatial_events::ChangeSet>)>,
    cached: bool,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let here = session.current_place().clone();
    let index = session.index();
    let scope = session.scope();
    let pinned = session.pins().pins().any(|pin| pin.spatial_id() == &here);
    let place = view::place_record_of(
        index,
        &here,
        scope,
        permission,
        pinned,
        session.record_of(&here).map(std::convert::AsRef::as_ref),
        session.tombstone_of(&here, now),
        now,
    )?;

    let mut groups = Vec::with_capacity(neighborhood.groups().len());
    let mut exits = ono_value::MapValue::new();
    for group in neighborhood.groups() {
        let record = view::group_record(index, &here, group, scope, all, now)?;
        exits.insert(
            group.label().into(),
            Value::Record(Arc::new(record.clone())),
        );
        groups.push(Value::Record(Arc::new(record)));
    }
    let mut landmarks = Vec::with_capacity(neighborhood.landmarks().len());
    for landmark in neighborhood.landmarks() {
        landmarks.push(Value::Record(Arc::new(view::landmark_record(
            index, landmark,
        )?)));
    }
    let neighborhood_record =
        view::neighborhood_record(neighborhood, groups.clone(), landmarks.clone())?;

    // §7.1: the root is a `SystemPlace`, and its domains are its exits. Anywhere else a domain is
    // not a neighbour, and claiming one would be geography invented for the occasion.
    let at_root = ono_spatial_query::resolve::space_of(&here)
        .is_some_and(|space| space.kind == ono_spatial_core::SpaceKind::Root);
    let domains = if at_root {
        Value::list(groups.clone())
    } else {
        Value::Null
    };
    // §24.1: the exhaustive view of the object behind a place is not what a default `look` is
    // for. At the root that object is the system of §7.1, and `--all` is where it is described.
    let system = if all && at_root {
        Value::Record(Arc::new(view::system_record(
            scope,
            groups.clone(),
            landmarks.clone(),
            now,
        )?))
    } else {
        Value::Null
    };
    // §19.1: "At local `SYSTEM`, `look` SHOULD expose available links". At the root of a host,
    // the other hosts this session can stand on are among the places reachable from here; deeper
    // in, a link is not a neighbour of a process, so the field is null rather than repeated.
    let links = if at_root {
        Value::list(view::link_records()?)
    } else {
        Value::Null
    };
    let spatial_type = ono_spatial_query::resolve::space_of(&here).map_or_else(
        || {
            index
                .get(&here)
                .map_or(SpatialType::System, |entry| entry.object().object_type())
        },
        |space| space.object_type,
    );
    let label = place.get("display_name").cloned().unwrap_or(Value::Null);

    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.place-view", 1))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                "the `ono.place-view/1` contract is not in this build",
            )
        })?;
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.place-view", 1)),
    )
    .set("id", Value::string(&here.to_string()))?
    .set("type", Value::string(spatial_type.as_str()))?
    .set("label", label)?
    .set(
        "hostname",
        Value::string(session.current_scope().host_scope().id()),
    )?
    .set("place", Value::Record(Arc::new(place)))?
    .set(
        "freshness",
        Value::string(source_freshness(
            neighborhood,
            index,
            &here,
            permission,
            cached,
            now,
        )),
    )?
    .set("groups", Value::list(groups))?
    .set("exits", Value::Map(Arc::new(exits)))?
    .set("domains", domains)?
    .set("links", links)?
    .set("landmarks", Value::list(landmarks))?
    .set("neighborhood", Value::Record(Arc::new(neighborhood_record)))?
    .set("boundary", view::boundary_record(session, &here)?)?
    .set("system", system)?
    .set("changed", change_summary(changes, now)?)?
    .set("generated_at", Value::Timestamp(now))?
    .build())
}

/// How the data behind the place is kept current, in §25.3's vocabulary.
///
/// §33.4 requires the freshness of a source to be visible, and §25.3 fixes the words for it.
/// This build has no subscriptions: every place is read from a provider when it is looked at, so
/// the honest word is `polled` — never `event_driven`, which would promise a liveness nothing
/// delivers (§2.17). A place the index holds past its TTL is `stale`, and a projection that could
/// not read every exit is `partial`.
fn source_freshness(
    neighborhood: &ono_spatial_core::Neighborhood,
    index: &ono_spatial_index::SpatialIndex,
    here: &ono_spatial_core::SpatialId,
    permission: PermissionState,
    cached: bool,
    now: Timestamp,
) -> &'static str {
    // §35.2: a place whose host cannot be reached any more is stale whatever the index says
    // about when it was last read — nothing is refreshing it (§19.1, §25.3).
    if permission == PermissionState::Stale {
        return "stale";
    }
    if index.freshness(here, now) == ono_spatial_core::Freshness::Stale {
        return "stale";
    }
    if neighborhood.completeness() == ono_spatial_core::Completeness::Partial {
        return "partial";
    }
    // §33.1 and §34 make a repeated view a read of the index rather than a second sweep of the
    // providers; §25.3 has the word for a view that was read, and using `polled` for it would
    // claim a freshness this answer does not have (ADR-0186).
    if cached { "cached" } else { "polled" }
}

/// The change section of §24.3, which never invents a change.
///
/// §24.3: "No fake change summary may be generated when no event source or comparison snapshot
/// exists." §25.4 names the one source this build has for a still view — the comparison of two
/// successive observations — and its provenance says so. Three answers are therefore possible and
/// §2.17 requires all three to stay apart:
///
/// - **`unknown`** — this session has not looked at this place before, so there is nothing to
///   compare to. Not "nothing changed".
/// - **`empty`** — there was a snapshot and nothing differs from it.
/// - **`available`** — there was a snapshot and these are the differences.
fn change_summary(
    changes: Option<(ono_value::Duration, Option<ono_spatial_events::ChangeSet>)>,
    now: Timestamp,
) -> Result<Value, ErrorValue> {
    let Some((window, compared)) = changes else {
        return Ok(Value::Null);
    };
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.change-summary", 1))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                "the `ono.change-summary/1` contract is not in this build",
            )
        })?;
    let (state, source, entries) = match &compared {
        None => ("unknown", Value::Null, Vec::new()),
        Some(changes) if changes.is_empty() => (
            "empty",
            Value::string(changes.source().as_str()),
            Vec::new(),
        ),
        Some(changes) => {
            let mut rows = Vec::new();
            for change in changes.changes() {
                rows.push(Value::Record(Arc::new(change_record(
                    change, changes, now,
                )?)));
            }
            ("available", Value::string(changes.source().as_str()), rows)
        }
    };
    let record = RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.change-summary", 1)),
    )
    .set("window", Value::Duration(window))?
    .set("state", Value::string(state))?
    .set("source", source)?
    .set("entries", Value::list(entries))?
    .build();
    Ok(Value::Record(Arc::new(record)))
}

/// One `ono.spatial-change/1` of a change section (§24.3, §25.1).
fn change_record(
    change: &ono_spatial_events::SpatialChange,
    changes: &ono_spatial_events::ChangeSet,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.spatial-change", 1))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                "the `ono.spatial-change/1` contract is not in this build",
            )
        })?;
    let places: Vec<Value> = change
        .places()
        .map(|place| Value::string(&place.to_string()))
        .collect();
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.spatial-change", 1)),
    )
    .set("kind", Value::string(change.kind().as_str()))?
    .set("id", Value::string(change.subject()))?
    .set("observed_at", Value::Timestamp(now))?
    .set("label", Value::string(change.label()))?
    .set(
        "reason",
        change
            .kind()
            .reason()
            .map_or(Value::Null, |reason| Value::string(reason.as_str())),
    )?
    .set("places", Value::list(places))?
    .set("source", Value::string(changes.source().as_str()))?
    .build())
}

/// The window `--changes`/`--changed` names, or the configured default where it names none.
fn window_of(
    option: Option<&Value>,
    preferences: &crate::spatial::session::ViewPreferences,
) -> Option<ono_value::Duration> {
    match option {
        Some(Value::Duration(window)) => Some(*window),
        // The bare spelling of §6.1 and §6.2: the option without its duration means the window
        // §47 configures, `spatial.look.change_window`.
        Some(Value::Bool(true)) => Some(preferences.change_window),
        _ => None,
    }
}

/// A window as the neighbourhood ranking reads it (§6.2's `--changed`).
fn span_of(window: ono_value::Duration) -> jiff::Span {
    let seconds = i64::try_from(window.nanoseconds() / 1_000_000_000).unwrap_or(i64::MAX);
    jiff::Span::new().seconds(seconds)
}

/// Moves the session's place onto an object a v0.2 `enter <target>` resolved (§30.2).
///
/// §30.2 is one sentence — "`enter` changes the spatial place" — and it applies to every spelling
/// of `enter`, including the target forms v0.2 already had. The context frame of v0.2 §14.3 and
/// the place of v0.4 §46 are two different pieces of state (§30.4), and one `enter` sets both:
/// after `enter process 1842`, later commands need no selector *and* `look` describes the
/// process.
pub async fn enter_observed(record: &ono_value::RecordValue) {
    let now = Timestamp::now();
    let mut session = spatial_session().await;
    let Ok(there) = session.projection_of(record) else {
        // A record §7 gives no place — an image, a link, a plugin — is not a place, and entering
        // it is a context push and nothing more (ADR-0133). Refusing here would break `enter`.
        return;
    };
    session.absorb(std::slice::from_ref(record), now);
    let here = session.current_place().clone();
    if here != there {
        session.trail_mut().record(NavigationStep::new(
            now,
            here,
            there.clone(),
            Movement::Enter,
        ));
        session.arrive_at(&there, now);
    }
}

/// Offers what a v0.3 adapter decoded to the spatial index (spec v0.4 §37, §37.1).
///
/// §37: "Adapted external tools may contribute typed objects to the spatial model when their
/// output maps to canonical spatial schemas." An adapter is not a provider: the spatial layer
/// asks its providers whenever it needs them, and never asks a tool. An adapted observation
/// therefore exists only where the user's own command line produced it, and this is where the
/// shell offers it — so that having looked at `lo` through `ip link` is something the place for
/// `lo` records, beside what the netlink provider said about it.
///
/// Only records whose identity the adapter carried in full are offered
/// ([`carries_full_identity`](ono_spatial_index::carries_full_identity)): those reduce to the
/// identity the canonical provider's own record reduces to, so they reconcile into one place
/// rather than becoming the duplicate node §37.1 forbids.
///
/// The shell does not buffer a stream in order to index it (§29.4, §34): a streaming decoder
/// hands its records straight to the consumer, and one of those becomes a place when something
/// places it — `… | enter process` resolves it through the canonical provider.
pub async fn observe_adapted(values: &[ono_value::Value]) {
    let records: Vec<ono_value::RecordValue> = values
        .iter()
        .filter_map(|value| value.as_record().ok().cloned())
        .filter(|record| {
            record.provenance().provider().starts_with("adapter:")
                && ono_spatial_index::carries_full_identity(record)
        })
        .collect();
    if records.is_empty() {
        return;
    }
    let now = Timestamp::now();
    let mut session = spatial_session().await;
    session.absorb(&records, now);
}

/// The refusal for an exit that was named and cannot be read (§35.2, §40, ADR-0275).
///
/// `None` where the group is genuinely empty or was answered: those are answers, not refusals.
fn withheld_exit(group: &ono_spatial_core::NeighborhoodGroup) -> Option<ErrorValue> {
    let (code, why) = match group.state() {
        ono_spatial_core::PermissionState::PermissionDenied => (
            ErrorCode::SpatialPermissionDenied,
            "this user may not read it",
        ),
        ono_spatial_core::PermissionState::Unsupported => (
            ErrorCode::SpatialUnsupported,
            "no provider in this build answers for it",
        ),
        ono_spatial_core::PermissionState::Stale => (
            ErrorCode::SpatialStale,
            "the last answer is older than the caller would accept",
        ),
        _ => return None,
    };
    let error = ErrorValue::new(
        code,
        format!("the `{}` of this place could not be read", group.label()),
    );
    Some(match group.detail() {
        Some(detail) => error.with_help(format!("{why}: {detail} (spec v0.4 §35.2)")),
        None => error.with_help(format!("{why} (spec v0.4 §35.2)")),
    })
}

/// The refusal for a relation word the current place does not offer (§40, ADR-0271).
///
/// The exits are the ones this place actually has right now, read from its own neighbourhood
/// rather than from the global relation vocabulary: `sockets` is an exit of a process and
/// `processes` is an exit of COMPUTE, and a user who typed the wrong one needs the list that
/// applies here.
fn unknown_exit(exits: &[String], relation: &str) -> ErrorValue {
    let error = ErrorValue::new(
        ErrorCode::SpatialNoRelation,
        format!("this place has no `{relation}` to be near"),
    );
    if exits.is_empty() {
        return error.with_help(
            "this place offers no relationships at all — `look` shows what it does offer \
             (spec v0.4 §24.2, §40)",
        );
    }
    error.with_help(format!(
        "its exits are {} — `near` with no relation lists all of them (spec v0.4 §6.2, §40)",
        exits.join(", ")
    ))
}

/// `near` (spec v0.4 §6.2, §29.4).
#[derive(Debug)]
pub struct Near {
    pins: Option<crate::spatial::PinStore>,
}

impl Near {
    /// The implementation registered against `ono.place.near`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self { pins }
    }
}

impl CommandImpl for Near {
    fn id(&self) -> &str {
        "ono.place.near"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("near"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            // A words-mode command that reads values resolves its arguments first: `--type
            // ["process"]`, `--limit (1 + 1)` and `--changed (1h)` are expressions until
            // something evaluates them, and an option nobody evaluated reads as an option nobody
            // wrote (ADR-0219, ADR-0556).
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let arguments = &arguments;
            let mut request = NeighborhoodRequest::new().all(arguments.flag("all"));
            let named_relation = arguments.selector("relation").and_then(text_of);
            if let Some(relation) = named_relation.clone() {
                request = request.along(relation);
            }
            let named_type = arguments.option("type").is_some();
            if let Some(value) = arguments.option("type") {
                request = request.of_type(crate::spatial::spatial_type(value)?);
            }
            // Whether this `near` asked about one exit rather than about the whole horizon.
            // Both spellings narrow, so both owe the caller the §42.4 answer for a group that
            // was named and could not be read.
            let narrowed = named_relation.is_some() || named_type;
            let limit = match arguments.option("limit") {
                Some(Value::Int(limit)) => usize::try_from(*limit).ok(),
                _ => None,
            };
            if let Some(limit) = limit {
                request = request.limit(limit);
            }
            let now = Timestamp::now();

            let mut session = spatial_session().await;
            if let Some(window) = window_of(arguments.option("changed"), session.preferences()) {
                request = request.changed_within(span_of(window));
            }
            with_pins(&mut session, self.pins.as_ref(), now).await?;
            let (neighborhood, _, _) =
                view::neighborhood_here(ctx.providers(), &mut session, &request, now).await?;

            // §40 and §2.17: "this place has no such exit" and "this exit is empty" are
            // different answers, and an empty stream said both. `follow` has always made the
            // distinction; `near <relation>` printed nothing at all with status 0 (ADR-0271).
            // The query layer decides which exits a named relation keeps (§6.2, direction and
            // all); an empty answer to that question is the answer that no exit here is called
            // this. Repeating its rule would give two definitions of one word.
            // §35.2 and §42.4: a group this user may not read, or that nothing serves, is not an
            // empty one. `near sockets` on a process whose descriptors are unreadable answered
            // with an empty stream and status 0, which is the false-empty rendering §42.4
            // forbids — `look` has always said `permission denied` in the same situation.
            // `--type X` is the second spelling of "answer about this one exit", and the guard
            // was written for the first only, so a refused group answered through `--type` fell
            // back through to the empty stream §42.4 forbids (issue #26, ADR-0557).
            if narrowed
                && neighborhood
                    .groups()
                    .iter()
                    .all(|group| group.members().is_empty())
                && let Some(refusal) = neighborhood.groups().iter().find_map(withheld_exit)
            {
                return Err(refusal);
            }
            if let Some(relation) = &named_relation
                && neighborhood.groups().is_empty()
            {
                // What this place does offer is asked for only on the way to the refusal: the
                // answer that was already computed carries only the exit that was asked about.
                let (whole, _, _) = view::neighborhood_here(
                    ctx.providers(),
                    &mut session,
                    &NeighborhoodRequest::new().all(true),
                    now,
                )
                .await?;
                let mut exits: Vec<String> = whole
                    .groups()
                    .iter()
                    .map(|group| group.label().to_owned())
                    .collect();
                exits.sort();
                exits.dedup();
                return Err(unknown_exit(&exits, relation));
            }

            let here = session.current_place().clone();
            let index = session.index();
            let scope = session.scope();
            let mut rows: Vec<Value> = Vec::new();
            for group in neighborhood.groups() {
                for member in group.members() {
                    let pinned = session.pins().pins().any(|pin| pin.spatial_id() == member);
                    rows.push(Value::Record(Arc::new(view::neighbor_record(
                        index,
                        &here,
                        group.label(),
                        group.state(),
                        member,
                        scope,
                        pinned,
                        now,
                    )?)));
                }
            }
            // §6.2: `--limit <n>` bounds the answer, not each exit of it. The ranking that chose
            // which members each group carries is the query layer's and is not redone here.
            if let Some(limit) = limit {
                rows.truncate(limit);
            }
            Ok(Outcome::Values(view::stream(rows)))
        })
    }
}

/// `enter <selector>` (spec v0.4 §6.3, §27.1).
#[derive(Debug)]
pub struct Enter {
    pins: Option<crate::spatial::PinStore>,
}

impl Enter {
    /// The implementation registered against `ono.place.enter`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self { pins }
    }
}

impl CommandImpl for Enter {
    fn id(&self) -> &str {
        "ono.place.enter"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("enter"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let now = Timestamp::now();
            // §28.2's `enter @-1`: the argument is an expression the evaluator resolves against
            // the results this session retained (v0.2 §20.2), not a word. A word binds to a value
            // directly; anything else is evaluated here, once, before the place is resolved.
            let selected = match ctx.arguments().selector("selector").cloned() {
                Some(value) => Some(value),
                None => match ctx.arguments().selector_expression("selector") {
                    Some(expression) => Some(ono_command::evaluate(
                        expression,
                        &Value::Null,
                        ctx.scope(),
                    )?),
                    None => None,
                },
            };
            let piped = ctx.take_input();

            let mut session = spatial_session().await;
            with_pins(&mut session, self.pins.as_ref(), now).await?;
            let here = session.current_place().clone();

            // §28.2: "A structured pipeline result containing spatially identifiable objects MUST
            // be enterable." The object arrives either through the pipe (`… | enter`) or as a
            // value the argument expanded to (`enter @-1`); both are the same movement.
            let delivered = match piped {
                Some(stream) => Some(stream.collect().await.into_values()),
                None => None,
            };
            let arrived = match &delivered {
                Some(values) => first_record(values.clone()),
                None => selected.as_ref().and_then(|value| {
                    first_record(match value {
                        Value::List(items) => items.to_vec(),
                        other => vec![other.clone()],
                    })
                }),
            };
            let there = if let Some(record) = arrived {
                enter_projected(&mut session, &record, now)?
            } else if let Some(values) = delivered.as_ref().filter(|values| !values.is_empty()) {
                // §37.2: "Raw external command output MUST NOT become spatial nodes through
                // generic table heuristics." Something arrived and none of it was an object, so
                // there is nothing here with a spatial identity — and reading a place out of the
                // bytes is exactly the inference that section forbids.
                return Err(not_enterable(values));
            } else {
                let selector = selected.as_ref().and_then(text_of).ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::SpatialNotFound,
                        "`enter` needs a place to move into",
                    )
                    .with_help("`look` lists the exits of the current place (spec v0.4 §24.2)")
                })?;
                resolved_place(ctx.providers(), &mut session, &here, &selector, now).await?
            };

            if here != there {
                let mut step =
                    NavigationStep::new(now, here.clone(), there.clone(), Movement::Enter);
                if let Some(crossing) =
                    crate::spatial::movement::crossing_between(&session, &here, &there)
                {
                    step = step.crossing(crossing);
                }
                session.trail_mut().record(step);
                session.arrive_at(&there, now);
            }
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

/// The §40 refusal for what arrived through the pipe and is not an object (§37.2).
///
/// §37.2 admits "only canonical typed adapter output or explicit plugin schemas" into the
/// spatial index, so a byte stream, a line of text or a number is not a place, and the honest
/// answer names that rather than reporting the place it could not find.
fn not_enterable(values: &[Value]) -> ErrorValue {
    let kind = values.first().map_or("nothing", |value| value.type_name());
    ErrorValue::new(
        ErrorCode::SpatialNotEnterable,
        format!(
            "`{kind}` is not a place: raw command output has no spatial identity (spec v0.4 \
             §37.2)"
        ),
    )
    .with_help(
        "an adapted command answers with typed objects one of which can be entered — `ip link | \
         where name == \"lo\" | enter` — where `raw` deliberately keeps the bytes (spec v0.3 \
         §1.17)",
    )
}

/// The first record among some values.
fn first_record(values: Vec<Value>) -> Option<RecordValue> {
    values.into_iter().find_map(|value| match value {
        Value::Record(record) => Some(RecordValue::clone(&record)),
        _ => None,
    })
}

/// The place a pipeline result is (§28.2), registered so the next command can look at it.
///
/// # Errors
///
/// `spatial.not_enterable` for a value §7 gives no place — a package, a log line, the raw output
/// of an external command. §37.2: raw command output never becomes a place.
fn enter_projected(
    session: &mut SpatialSessionState,
    record: &RecordValue,
    now: Timestamp,
) -> Result<ono_spatial_core::SpatialId, ErrorValue> {
    // A `find place` result and a `near` neighbour are already places (§28.1's "selected objects
    // MUST be exposable as typed values"): they carry the `spatial_id` rather than the object.
    if matches!(
        record.schema().id().name(),
        "ono.spatial-place" | "ono.spatial-neighbor"
    ) && let Some(text) = record
        .get("spatial_id")
        .and_then(|value| value.as_str().ok())
        && let Some(id) = ono_spatial_core::SpatialId::parse(text)
    {
        if session.index().contains(&id) || ono_spatial_query::resolve::space_of(&id).is_some() {
            return Ok(id);
        }
        return Err(ErrorValue::new(
            ErrorCode::SpatialNotFound,
            format!("`{text}` is no longer a place this session knows"),
        ));
    }
    let there = session.projection_of(record).map_err(|_| {
        ErrorValue::new(
            ErrorCode::SpatialNotEnterable,
            format!(
                "`{}` is a value, not a place: the spatial geography holds no {} (spec v0.4 §7, \
                 §37.2)",
                record.schema().id(),
                record.schema().name()
            ),
        )
        .with_help("`find place <text>` finds the places this shell can enter (spec v0.4 §6.8)")
    })?;
    session.absorb(std::slice::from_ref(record), now);
    Ok(there)
}

/// The place a selector names, observing what it implies where the index does not hold it yet.
///
/// # Errors
///
/// The §40 refusal `resolve` produced: `spatial.not_found` for a word nothing answers to, or
/// `spatial.ambiguous_selector` for one several places answer to (§27.1, §27.2).
///
/// §27.1 resolves against what is visible first, so nothing is asked of a provider while a
/// declared child or a known neighbour answers. When none does, the *selector* says which
/// providers could hold the answer, exactly as a predicate does for `find place` (§34, §45.3):
/// `enter 1842` is a question about processes and about everything else that answers to `1842`,
/// and it is asked once.
pub async fn resolved_place(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    here: &ono_spatial_core::SpatialId,
    selector: &str,
    now: Timestamp,
) -> Result<ono_spatial_core::SpatialId, ErrorValue> {
    // §27.1 step 4 is the *current host's* index: standing on a linked host, `enter process/1`
    // is a question about that host, and answering it with the local pid 1 the index still holds
    // is the accidental local/remote merge §43.7 forbids.
    let context = SelectorContext::at(here.clone())
        .on_host(ono_spatial_query::resolve::locality(Some(session.current_scope())).to_owned());
    let mut resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
    if matches!(resolution, ono_spatial_query::Resolution::NotFound)
        && let Some(space) = ono_spatial_query::resolve::space_of(here)
    {
        view::observe_space(providers, session, space, false, now).await?;
        resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
    }
    if matches!(resolution, ono_spatial_query::Resolution::NotFound)
        && let Some(path) = absolute_path_in(selector)
    {
        // §33.3: the path tree is query-driven, so a path is a place only once somebody names
        // one. `jump storage:/data` and `enter /etc/nginx` name one (§6.5, §15.1).
        view::observe_path(providers, session, &path, now).await;
        resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
    }
    if matches!(resolution, ono_spatial_query::Resolution::NotFound) {
        let plan = ono_spatial_query::targets_for(
            type_hint(selector),
            &std::collections::BTreeSet::new(),
            &|target| !providers.for_target(target).is_empty(),
            &|_, _| true,
        );
        // v0.4.1 §36.1: "A selector miss MUST not be substantially more expensive than a hit
        // solely because the system scans an unnecessarily complete global candidate set." The
        // set is asked cheapest first and the sweep stops as soon as the selector resolves, so it
        // is complete only where completeness is what the answer needed. §34.2's classes are the
        // order (ADR-0494, ADR-0497).
        for class in ono_spatial_core::AcquisitionCost::ALL {
            let targets: std::collections::BTreeSet<&'static str> = plan
                .asked()
                .iter()
                .copied()
                .filter(|target| ono_spatial_query::acquisition_of_target(target) == Some(*class))
                .collect();
            if targets.is_empty() {
                continue;
            }
            view::observe_targets(providers, session, &targets, now).await;
            resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
            if !matches!(resolution, ono_spatial_query::Resolution::NotFound) {
                break;
            }
        }
    }
    // §27.2: "Interactive ambiguity opens a picker." §29.3: a script never sees one, so the same
    // resolution turns into `spatial.ambiguous_selector` wherever there is nobody to ask.
    if let ono_spatial_query::Resolution::Ambiguous(candidates) = &resolution
        && crate::spatial::at_terminal()
        && let Some(chosen) = crate::spatial::interactive::pick(selector, candidates)
        && let Some(candidate) = candidates.get(chosen)
    {
        return Ok(candidate.spatial_id().clone());
    }
    let found = resolution.require(selector)?;
    Ok(found.spatial_id().clone())
}

/// The absolute path a selector names, where it names one.
///
/// Two spellings reach the path tree: the path itself, and the scoped form §6.5 writes a `jump`
/// with — `storage:/data`. Only absolute paths, because a command has no working directory of its
/// own: a relative one is resolved by the evaluator before it ever reaches a place (§30.2).
fn absolute_path_in(selector: &str) -> Option<std::path::PathBuf> {
    let candidate = selector
        .split_once(':')
        .map_or(selector, |(_, rest)| rest)
        .trim();
    candidate
        .starts_with('/')
        .then(|| std::path::PathBuf::from(candidate))
}

/// The kind of place a `<type>/<key>` selector names, where it names one (§11.2's `process/1842`).
pub(crate) fn type_hint(selector: &str) -> Option<SpatialType> {
    let (kind, key) = selector.split_once('/')?;
    if key.is_empty() {
        return None;
    }
    SpatialType::ALL
        .iter()
        .copied()
        .find(|known| known.as_str().eq_ignore_ascii_case(kind))
}

/// `home` (spec v0.4 §6.6, §7.1).
#[derive(Debug)]
pub struct Home;

impl CommandImpl for Home {
    fn id(&self) -> &str {
        "ono.place.home"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("home"))
    }

    fn invoke_async<'a>(&'a self, _ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let now = Timestamp::now();
            let mut session = spatial_session().await;
            go_home(&mut session, now);
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

/// Back to the root place of this host (spec v0.4 §6.6, §7.1).
///
/// Shared with the full-screen view, which binds the same semantic action to `h` (§23.3).
pub fn go_home(session: &mut SpatialSessionState, now: Timestamp) {
    let here = session.current_place().clone();
    let root = ono_spatial_core::space::root().spatial_id();
    if here != root {
        session
            .trail_mut()
            .record(NavigationStep::new(now, here, root, Movement::Home));
    }
}

/// The refusal §10.3 requires of an action that needs a live object, where the place is gone.
///
/// The message is §40's own example, which is what an actionable next step looks like here: the
/// place, when it ended, and — where one can be identified — what took its place.
pub(crate) fn gone_here(
    session: &SpatialSessionState,
    here: &ono_spatial_core::SpatialId,
    now: Timestamp,
) -> Option<ErrorValue> {
    let liveness = session.liveness(here, now);
    if liveness.accepts_actions() {
        return None;
    }
    let (what, when) = liveness.tombstone().map_or_else(
        || ("this place".to_owned(), String::new()),
        |tombstone| {
            let age = ono_value::Duration::from_nanoseconds(
                tombstone
                    .age(now)
                    .total(jiff::Unit::Nanosecond)
                    .unwrap_or(0.0)
                    .max(0.0) as i128,
            );
            (
                format!("`{}`", tombstone.display_name()),
                format!(" {age} ago"),
            )
        },
    );
    Some(
        ErrorValue::new(
            ErrorCode::SpatialDestinationGone,
            format!("destination no longer exists: {what} ended{when}"),
        )
        .with_help(
            "a tombstone keeps the place and its trail record; it does not accept actions that \
             need the object to be there (spec v0.4 §10.3, §40)",
        ),
    )
}

pub(crate) fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        other => ono_value::canonical_text(other)
            .ok()
            .filter(|text| !text.is_empty()),
    }
}

/// `follow <relation> [selector]` (spec v0.4 §6.4, §11.1, §11.2, §40).
///
/// §6.4 is one sentence and this command is that sentence: "`follow` MUST traverse a relationship
/// edge, not a canonical hierarchy edge." So a word that names a canonical space is refused even
/// though `enter` reaches it, and a word that names a relation this place does not have is
/// refused even though another kind of place has it. The four refusals of §40 stay distinct:
///
/// - a hierarchy name, or a relation this place has no edge along — `spatial.no_relation`;
/// - a relation whose exit the provider refused — `spatial.permission_denied`;
/// - a relation nothing in this build serves — `spatial.unsupported`;
/// - a word that names no relation of this kind of place — `spatial.not_found`, because the
///   name was understood but not here.
#[derive(Debug)]
pub struct Follow {
    pins: Option<crate::spatial::PinStore>,
}

impl Follow {
    /// The implementation registered against `ono.place.follow`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self { pins }
    }
}

impl CommandImpl for Follow {
    fn id(&self) -> &str {
        "ono.place.follow"
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited("follow"))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            // A words-mode command that reads values resolves its arguments first: `--type
            // ["process"]`, `--limit (1 + 1)` and `--changed (1h)` are expressions until
            // something evaluates them, and an option nobody evaluated reads as an option nobody
            // wrote (ADR-0219, ADR-0556).
            let arguments = ctx.arguments().evaluated(ctx.scope())?;
            let arguments = &arguments;
            let relation = arguments
                .selector("relation")
                .and_then(text_of)
                .ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::SpatialNoRelation,
                        "`follow` needs a relation to traverse",
                    )
                    .with_help("`look` lists the exits of the current place (spec v0.4 §24.2)")
                })?;
            let wanted = arguments.selector("selector").and_then(text_of);
            let now = Timestamp::now();

            let mut session = spatial_session().await;
            with_pins(&mut session, self.pins.as_ref(), now).await?;
            let here = session.current_place().clone();

            // §11.1: hierarchy is not the graph. A canonical space is reached with `enter`, and
            // saying so is more useful than saying the word is unknown.
            if let Some(space) = ono_spatial_core::space::spaces()
                .iter()
                .find(|space| space.label.eq_ignore_ascii_case(&relation) && space.is_served())
            {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialNoRelation,
                    format!("`{relation}` is a canonical child, not a relationship edge"),
                )
                .with_help(format!(
                    "`enter {}` moves into it; `follow` traverses relationships only (spec v0.4 \
                     §6.4, §11.1)",
                    space.label
                )));
            }

            let Some(object_type) = session
                .index()
                .get(&here)
                .map(|entry| entry.object().object_type())
            else {
                // A canonical space is geography: it holds places, and holding is hierarchy
                // (§3.4, §11.1). A declared relation name is therefore a relation this place does
                // not have; a word nobody declares names nothing at all (§40).
                if ono_spatial_core::relation::labels().contains(&relation.as_str()) {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialNoRelation,
                        format!("this place has no `{relation}` relation to traverse"),
                    )
                    .with_help(
                        "a canonical space holds places rather than relationships; `enter` moves \
                         into one (spec v0.4 §11.1)",
                    ));
                }
                return Err(ErrorValue::new(
                    ErrorCode::SpatialNotFound,
                    format!("`{relation}` is not a relation any place declares"),
                )
                .with_help("`look` lists the exits of the current place (spec v0.4 §24.2, §40)"));
            };

            let specs = ono_spatial_core::relation::resolve_label(object_type, &relation);
            if specs.is_empty() {
                // The word may still be a relation of some other kind of place. §40 wants the
                // difference visible: a name nobody declares is `not_found` as well, and both are
                // "the current place cannot answer to this word".
                return Err(unknown_relation(&relation, object_type));
            }
            if specs.len() > 1 {
                // §29.3: a script never picks. §6.4's own diagnostic lists what it could mean.
                return Err(ErrorValue::new(
                    ErrorCode::SpatialAmbiguousSelector,
                    format!(
                        "`{relation}` names {} relations of a {object_type}: {}",
                        specs.len(),
                        specs
                            .iter()
                            .map(|spec| spec.id)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
                .with_help(
                    "name the relation itself — `follow <relation-id>` — or one of its exits \
                     (spec v0.4 §29.3, §41.2)",
                ));
            }

            // Reading the edges is reading the providers (§2.16): a relation nobody asked about
            // yet is not a relation that is not there.
            //
            // v0.4.1 §34.3: "If a relationship is described as 'available on request', there MUST
            // actually be a request path." Naming the relation *is* the request for anything an
            // orientation query merely declined to spend the budget on; `--resolve` is the
            // request for the rest — a relation whose class is `expensive` or `external`, which a
            // named `follow` still will not pay for by itself (§34.2, ADR-0495).
            let interest = crate::spatial::relations::Interest::here()
                .along(Some(relation.clone()))
                .complete(arguments.flag("resolve"));
            crate::spatial::relations::observe(
                ctx.providers(),
                &mut session,
                &here,
                &interest,
                now,
            )
            .await?;

            // §10.3: a tombstone "MUST NOT accept actions that require a live object", and
            // traversing a relationship is one — the edges of a place that is gone are the ones
            // it had, not the ones it has. The check comes after the observation because that
            // observation is what discovers the absence (§33.2).
            if let Some(refusal) = gone_here(&session, &here, now) {
                return Err(refusal);
            }

            // The word decides the direction: `parent` and `children` are the two exits of one
            // relation, and following the wrong one would walk the edge backwards (§12).
            let Some(exit) = specs[0].group_for(object_type, &relation) else {
                return Err(unknown_relation(&relation, object_type));
            };
            let group = session
                .index()
                .relation_summary(&here, usize::MAX, now)
                .into_iter()
                .find(|group| group.label() == exit);
            let Some(group) = group else {
                return Err(unknown_relation(&relation, object_type));
            };
            match group.state() {
                PermissionState::PermissionDenied => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialPermissionDenied,
                        format!(
                            "the `{relation}` of this place could not be read: {}",
                            group.detail().unwrap_or("permission denied")
                        ),
                    ));
                }
                // §35.2 keeps these apart and §34.3 makes the difference actionable: `unknown`
                // is "nobody has paid for it yet", and a refusal that says so without saying how
                // to pay is the state §34.3 forbids.
                PermissionState::Unknown => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialUnsupported,
                        format!(
                            "the `{relation}` of this place has not been read: {}",
                            group.detail().unwrap_or("nothing has asked for it")
                        ),
                    )
                    .with_help(
                        "`follow <relation> --resolve` pays for it (v0.4.1 §34.3). It is                          classified expensive or external, so an orientation `look` leaves it                          discoverable and unloaded (§34.2).",
                    ));
                }
                PermissionState::Unsupported => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialUnsupported,
                        format!(
                            "the `{relation}` of this place is not answered here: {}",
                            group.detail().unwrap_or("no provider serves it")
                        ),
                    ));
                }
                _ => {}
            }

            let index = session.index();
            let candidates: Vec<ono_spatial_core::SpatialId> = match wanted.as_deref() {
                None => group.members().to_vec(),
                // §27.1 resolves the most specific answer, and §6.4's own example is the sharp
                // case: a process holding a listener on :443 also holds the connections accepted
                // on :443, and `follow socket :443` means the listener — the place the selector
                // names outright rather than the place it happens to appear in.
                Some(text) => best_matches(index, group.members(), text),
            };
            let there = match candidates.len() {
                1 => candidates[0].clone(),
                0 if wanted.is_some() => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialNotFound,
                        format!(
                            "no `{relation}` of this place answers to `{}`",
                            wanted.unwrap_or_default()
                        ),
                    )
                    .with_help(format!(
                        "`near {relation}` lists what this exit reaches (spec v0.4 §6.2)"
                    )));
                }
                0 => {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialNoRelation,
                        format!("this place has no `{relation}` to follow"),
                    )
                    .with_help(
                        "`look` lists the exits this place does have (spec v0.4 §24.2, §40)",
                    ));
                }
                _ => {
                    // §6.4: "If multiple edges match, interactive selection is required." A
                    // script has nobody to ask, so §29.3 makes it an error with the candidates.
                    return Err(ambiguous_edge(index, &relation, &candidates));
                }
            };

            let step = NavigationStep::new(now, here, there.clone(), Movement::Follow)
                .along(specs[0].relation_type())
                // §6.4: the relation traversed is recorded — and a relation has two ends and
                // therefore two words, so the word this traversal took travels with the id.
                .spelled(relation.as_str());
            session.trail_mut().record(step);
            // §14.5: following an edge whose far end is on another host crosses the boundary,
            // and the session's host follows the place it is standing on (§19.2, §3.2).
            session.arrive_at(&there, now);
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

/// The refusal for a word that names no relation of this kind of place (§40).
fn unknown_relation(relation: &str, object_type: SpatialType) -> ErrorValue {
    let exits: Vec<&str> = ono_spatial_core::relation::exits_from(object_type)
        .map(|(label, _)| label)
        .collect();
    if ono_spatial_core::relation::labels().contains(&relation) {
        return ErrorValue::new(
            ErrorCode::SpatialNotFound,
            format!(
                "a {object_type} has no `{relation}`; the relation belongs to another kind of place"
            ),
        )
        .with_help(format!("this place has {}", exits.join(", ")));
    }
    ErrorValue::new(
        ErrorCode::SpatialNotFound,
        format!("`{relation}` is not a relation any place declares"),
    )
    .with_help(format!("this place has {}", exits.join(", ")))
}

/// The refusal §29.3 requires when several edges of one relation match (§6.4).
fn ambiguous_edge(
    index: &ono_spatial_index::SpatialIndex,
    relation: &str,
    candidates: &[ono_spatial_core::SpatialId],
) -> ErrorValue {
    // The candidates are a list, and a list is carried as one: the shell decided there are
    // several, and the render boundary lays them out one per line while still escaping whatever
    // a display name brought with it (ADR-0211).
    let rows: Vec<Value> = candidates
        .iter()
        .take(10)
        .map(|id| {
            let name = index.get(id).map_or_else(
                || id.to_string(),
                |entry| entry.object().display_name().to_owned(),
            );
            Value::string(&format!("{name}  {id}"))
        })
        .collect();
    ErrorValue::new(
        ErrorCode::SpatialAmbiguousSelector,
        format!("`{relation}` reaches {} places:", candidates.len()),
    )
    .with_metadata("details", Value::list(rows))
    .with_help(format!(
        "name which one — `follow {relation} <selector>` — or list them with `near {relation}` \
         (spec v0.4 §6.4, §29.3)"
    ))
}

/// The members `text` names, keeping only the closest matches (§27.1, §27.2).
///
/// A selector answers exactly, or as a part of a longer name, and an exact answer is never
/// ambiguous with an approximate one. Among equally close matches the shorter name is the more
/// specific: `:443` names the listener `127.0.0.1:443` before the connection
/// `10.0.0.5:51722 -> 127.0.0.1:443` that merely ends there. Where several are equally close and
/// equally specific, the caller is told so rather than given one of them (§29.3).
fn best_matches(
    index: &ono_spatial_index::SpatialIndex,
    members: &[ono_spatial_core::SpatialId],
    text: &str,
) -> Vec<ono_spatial_core::SpatialId> {
    let mut ranked: Vec<((u8, usize), ono_spatial_core::SpatialId)> = members
        .iter()
        .filter_map(|id| match_rank(index, id, text).map(|rank| (rank, id.clone())))
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0));
    let Some(best) = ranked.first().map(|(rank, _)| *rank) else {
        return Vec::new();
    };
    ranked
        .into_iter()
        .filter(|(rank, _)| *rank == best)
        .map(|(_, id)| id)
        .collect()
}

/// How closely `text` names the place at `id`: exact first, then containment, then the length of
/// the name the selector had to be found in.
fn match_rank(
    index: &ono_spatial_index::SpatialIndex,
    id: &ono_spatial_core::SpatialId,
    text: &str,
) -> Option<(u8, usize)> {
    let needle = text.trim().to_ascii_lowercase();
    let bare = needle.trim_start_matches(':').to_owned();
    if id.to_string() == text.trim() {
        return Some((0, 0));
    }
    let entry = index.get(id)?;
    let shown = entry.object().display_name().to_ascii_lowercase();
    if shown == needle || shown == bare {
        return Some((0, shown.len()));
    }
    if entry.aliases().contains(&needle) || entry.aliases().contains(&bare) {
        return Some((1, shown.len()));
    }
    if shown.contains(&needle) || (!bare.is_empty() && shown.contains(&bare)) {
        return Some((2, shown.len()));
    }
    None
}
