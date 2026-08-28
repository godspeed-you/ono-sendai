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
const COMPOSER: &str = "ono.spatial";

/// Reads the session's pins from the store, once per command that needs them (§46.1).
async fn with_pins(
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
            let (neighborhood, permission) =
                view::neighborhood_here(ctx, &mut session, &request, now).await?;
            let view = place_view(&session, &neighborhood, permission, all, changes, now)?;

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
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let here = session.current_place().clone();
    let index = session.index();
    let scope = session.scope();
    let pinned = session.pins().pins().any(|pin| pin.spatial_id() == &here);
    let place = view::place_record(index, &here, scope, permission, pinned, now)?;

    let mut groups = Vec::with_capacity(neighborhood.groups().len());
    for group in neighborhood.groups() {
        groups.push(Value::Record(Arc::new(view::group_record(
            index, &here, group, scope, all, now,
        )?)));
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
    .set("groups", Value::list(groups))?
    .set("domains", domains)?
    .set("landmarks", Value::list(landmarks))?
    .set("neighborhood", Value::Record(Arc::new(neighborhood_record)))?
    .set("system", system)?
    .set("changed", change_summary(changes)?)?
    .set("generated_at", Value::Timestamp(now))?
    .build())
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
    let (index, bridge) = session.absorb_with();
    let Ok(object) = bridge.project(record) else {
        // A record §7 gives no place — an image, a link, a plugin — is not a place, and entering
        // it is a context push and nothing more (ADR-0133). Refusing here would break `enter`.
        return;
    };
    let there = object.spatial_id().clone();
    bridge.absorb(index, std::slice::from_ref(record), now);
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
            let (neighborhood, _) =
                view::neighborhood_here(ctx, &mut session, &request, now).await?;

            let index = session.index();
            let scope = session.scope();
            let mut rows: Vec<Value> = Vec::new();
            for group in neighborhood.groups() {
                for member in group.members() {
                    let pinned = session.pins().pins().any(|pin| pin.spatial_id() == member);
                    rows.push(Value::Record(Arc::new(view::neighbor_record(
                        index,
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
            let selector = ctx
                .arguments()
                .selector("selector")
                .and_then(text_of)
                .ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::SpatialNotFound,
                        "`enter` needs a place to move into",
                    )
                    .with_help("`look` lists the exits of the current place (spec v0.4 §24.2)")
                })?;
            let now = Timestamp::now();

            let mut session = spatial_session().await;
            with_pins(&mut session, self.pins.as_ref(), now).await?;
            let here = session.current_place().clone();
            let context = SelectorContext::at(here.clone());

            // §27.1 resolves against what is visible first. The canonical children of a space are
            // declared, so they are visible without asking anyone; the objects inside it are not,
            // and are observed only when the declared answer misses (§34).
            let mut resolution =
                ono_spatial_query::resolve(session.index(), &selector, &context, now);
            if matches!(resolution, ono_spatial_query::Resolution::NotFound)
                && let Some(space) = ono_spatial_query::resolve::space_of(&here)
            {
                view::observe_space(ctx, &mut session, space, false, now).await?;
                resolution = ono_spatial_query::resolve(session.index(), &selector, &context, now);
            }
            let found = resolution.require(&selector)?;
            let there = found.spatial_id().clone();

            session
                .trail_mut()
                .record(NavigationStep::new(now, here, there, Movement::Enter));
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
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
            let here = session.current_place().clone();
            let root = ono_spatial_core::space::root().spatial_id();
            if here != root {
                session
                    .trail_mut()
                    .record(NavigationStep::new(now, here, root, Movement::Home));
            }
            Ok(Outcome::Values(ValueStream::from_values(Vec::new())))
        })
    }
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        other => ono_value::canonical_text(other)
            .ok()
            .filter(|text| !text.is_empty()),
    }
}
