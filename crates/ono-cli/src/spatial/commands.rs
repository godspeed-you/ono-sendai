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
            let arguments = ctx.arguments();
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
            let (neighborhood, permission, cached) =
                view::neighborhood_here(ctx, &mut session, &request, now).await?;
            let view = place_view(
                &session,
                &neighborhood,
                permission,
                all,
                changes,
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
    changes: Option<ono_value::Duration>,
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
    .set("hostname", Value::string(session.scope().host_scope().id()))?
    .set("place", Value::Record(Arc::new(place)))?
    .set(
        "freshness",
        Value::string(source_freshness(neighborhood, index, &here, cached, now)),
    )?
    .set("groups", Value::list(groups))?
    .set("exits", Value::Map(Arc::new(exits)))?
    .set("domains", domains)?
    .set("landmarks", Value::list(landmarks))?
    .set("neighborhood", Value::Record(Arc::new(neighborhood_record)))?
    .set("system", system)?
    .set("changed", change_summary(changes)?)?
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
    cached: bool,
    now: Timestamp,
) -> &'static str {
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
/// exists." This build has neither — the event merge and the snapshot diff of §25 are a later
/// phase — so a caller who asks gets the §35.2 state that says so, with no entries. That is a
/// different answer from "nothing changed", and §2.17 requires the difference to be visible.
fn change_summary(window: Option<ono_value::Duration>) -> Result<Value, ErrorValue> {
    let Some(window) = window else {
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
    let record = RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.change-summary", 1)),
    )
    .set("window", Value::Duration(window))?
    .set("state", Value::string("unsupported"))?
    .set("source", Value::Null)?
    .set("entries", Value::list(Vec::new()))?
    .build();
    Ok(Value::Record(Arc::new(record)))
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
        session
            .trail_mut()
            .record(NavigationStep::new(now, here, there, Movement::Enter));
    }
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
            let arguments = ctx.arguments();
            let mut request = NeighborhoodRequest::new().all(arguments.flag("all"));
            if let Some(relation) = arguments.selector("relation").and_then(text_of) {
                request = request.along(relation);
            }
            if let Some(value) = arguments.option("type") {
                request = request.of_type(crate::spatial::spatial_type(value)?);
            }
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
                view::neighborhood_here(ctx, &mut session, &request, now).await?;

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
            let arrived = match piped {
                Some(stream) => first_record(stream.collect().await.into_values()),
                None => selected.as_ref().and_then(|value| {
                    first_record(match value {
                        Value::List(items) => items.to_vec(),
                        other => vec![other.clone()],
                    })
                }),
            };
            let there = if let Some(record) = arrived {
                enter_projected(&mut session, &record, now)?
            } else {
                let selector = selected.as_ref().and_then(text_of).ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::SpatialNotFound,
                        "`enter` needs a place to move into",
                    )
                    .with_help("`look` lists the exits of the current place (spec v0.4 §24.2)")
                })?;
                resolved_place(ctx, &mut session, &here, &selector, now).await?
            };

            if here != there {
                session
                    .trail_mut()
                    .record(NavigationStep::new(now, here, there, Movement::Enter));
            }
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
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
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    here: &ono_spatial_core::SpatialId,
    selector: &str,
    now: Timestamp,
) -> Result<ono_spatial_core::SpatialId, ErrorValue> {
    let context = SelectorContext::at(here.clone());
    let mut resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
    if matches!(resolution, ono_spatial_query::Resolution::NotFound)
        && let Some(space) = ono_spatial_query::resolve::space_of(here)
    {
        view::observe_space(ctx, session, space, false, now).await?;
        resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
    }
    if matches!(resolution, ono_spatial_query::Resolution::NotFound)
        && let Some(path) = absolute_path_in(selector)
    {
        // §33.3: the path tree is query-driven, so a path is a place only once somebody names
        // one. `jump storage:/data` and `enter /etc/nginx` name one (§6.5, §15.1).
        view::observe_path(ctx, session, &path, now).await;
        resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
    }
    if matches!(resolution, ono_spatial_query::Resolution::NotFound) {
        let plan = ono_spatial_query::targets_for(
            type_hint(selector),
            &std::collections::BTreeSet::new(),
            &|target| !ctx.providers().for_target(target).is_empty(),
            &|_, _| true,
        );
        let targets: std::collections::BTreeSet<&'static str> =
            plan.asked().iter().copied().collect();
        view::observe_targets(ctx, session, &targets, now).await;
        resolution = ono_spatial_query::resolve(session.index(), selector, &context, now);
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
            let arguments = ctx.arguments();
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
            let interest =
                crate::spatial::relations::Interest::here().along(Some(relation.clone()));
            crate::spatial::relations::observe(ctx, &mut session, &here, &interest, now).await?;

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
                PermissionState::Unsupported | PermissionState::Unknown => {
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

            let step = NavigationStep::new(now, here, there, Movement::Follow)
                .along(specs[0].relation_type())
                // §6.4: the relation traversed is recorded — and a relation has two ends and
                // therefore two words, so the word this traversal took travels with the id.
                .spelled(relation.as_str());
            session.trail_mut().record(step);
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
    let rows: Vec<String> = candidates
        .iter()
        .take(10)
        .map(|id| {
            let name = index.get(id).map_or_else(
                || id.to_string(),
                |entry| entry.object().display_name().to_owned(),
            );
            format!("  {name}  {id}")
        })
        .collect();
    ErrorValue::new(
        ErrorCode::SpatialAmbiguousSelector,
        format!(
            "`{relation}` reaches {} places:\n{}",
            candidates.len(),
            rows.join("\n")
        ),
    )
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
