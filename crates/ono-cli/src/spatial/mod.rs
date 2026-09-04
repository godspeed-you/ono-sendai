//! The spatial commands the shell dispatches (spec v0.4 §6, §45.6, §46).
//!
//! §45.6: "`ono-cli` should parse/dispatch spatial commands and own session current-place state,
//! but SHOULD NOT implement graph selection, identity reconciliation or map layout directly."
//! That is the split here. The shell reads the arguments, knows which host and which boot the
//! session belongs to, asks the providers for the objects a query needs, and hands everything
//! else on:
//!
//! - which record is which place, and whether two records are one — `ono-spatial-index`'s
//!   provider bridge (§45.2);
//! - which places answer a query, in which order, and what a query may cost —
//!   `ono-spatial-query` (§45.3).
//!
//! What it does own is the state neither of those crates can have: the host and boot identity
//! every observation belongs to (§10.2), and the pins that outlive the session (§46.1).

pub mod commands;
pub mod complete;
pub mod contributions;
pub mod find;
pub mod interactive;
pub mod links;
pub mod live;
pub mod map;
pub mod movement;
pub mod pins;
pub mod prompt;
pub mod relations;
pub mod session;
pub mod storage;
pub mod view;

pub use commands::{Enter, Follow, Home, Look, Near, enter_observed, observe_adapted};
pub use find::{FindPlace, local_scope, spatial_type};
pub use links::LinkFacts;
pub use map::{Map, MapLinks};
pub use movement::{Back, Jump, Trail, Up};
pub use pins::{PinPlace, PinStore, UnpinPlace, pin_path};
pub use prompt::{at_terminal, mark_interactive, place_segment};
pub use session::{RemoteHost, SpatialSessionState, spatial_session};

/// Reads the `spatial.*` settings the spatial layer honours, and hands them to the session state
/// (spec v0.4 §26.3, §34.2, §47).
///
/// §26.3 requires the landmark thresholds to be "inspectable and configurable"; a threshold the
/// engine did not read would only be the first of those. The same holds for §34.2's node budget,
/// which §47 spells `spatial.map.node_budget`.
pub fn configure_from(settings: &crate::settings::Settings) {
    let integer = |key: &str, fallback: i128| -> i128 {
        match settings.effective(key).map(|resolved| &resolved.value) {
            Some(ono_value::Value::Int(number)) => *number,
            _ => fallback,
        }
    };
    let flag = |key: &str, fallback: bool| -> bool {
        match settings.effective(key).map(|resolved| &resolved.value) {
            Some(ono_value::Value::Bool(state)) => *state,
            _ => fallback,
        }
    };
    let defaults = ono_spatial_query::LandmarkThresholds::default();
    let window = match settings
        .effective("spatial.look.change_window")
        .map(|resolved| &resolved.value)
    {
        Some(ono_value::Value::String(text)) => ono_value::Duration::parse(text).ok(),
        Some(ono_value::Value::Duration(window)) => Some(*window),
        _ => None,
    };
    let seconds = window.map_or(300, |window| {
        i64::try_from(window.nanoseconds() / 1_000_000_000).unwrap_or(300)
    });
    let thresholds = ono_spatial_query::LandmarkThresholds {
        enabled: flag("spatial.landmarks.enabled", defaults.enabled),
        #[allow(
            clippy::cast_precision_loss,
            reason = "a percentage threshold is a small integer in the settings catalogue"
        )]
        high_cpu_percent: integer("spatial.landmarks.high_cpu", 80) as f64,
        #[allow(
            clippy::cast_precision_loss,
            reason = "a percentage threshold is a small integer in the settings catalogue"
        )]
        storage_pressure_percent: integer("spatial.landmarks.storage_pressure", 90) as f64,
        change_window: jiff::Span::new().seconds(seconds),
    };
    let duration = |key: &str| -> Option<ono_value::Duration> {
        match settings.effective(key).map(|resolved| &resolved.value) {
            Some(ono_value::Value::String(text)) => ono_value::Duration::parse(text).ok(),
            Some(ono_value::Value::Duration(configured)) => Some(*configured),
            _ => None,
        }
    };
    let fallback = session::ViewPreferences::default();
    let preferences = session::ViewPreferences {
        change_window: window.unwrap_or(fallback.change_window),
        map_node_budget: usize::try_from(integer("spatial.map.node_budget", 100).max(1))
            .unwrap_or(ono_spatial_query::MAP_NODE_BUDGET),
        tombstone_lifetime: duration("spatial.tombstone.lifetime")
            .unwrap_or(fallback.tombstone_lifetime),
        live_interval: duration("spatial.live.interval").unwrap_or(fallback.live_interval),
        orientation_objects: usize::try_from(integer("limits.orientation_objects", 128).max(1))
            .unwrap_or(fallback.orientation_objects),
        orientation_ceiling: usize::try_from(integer("limits.orientation_ceiling", 16_384).max(1))
            .unwrap_or(fallback.orientation_ceiling),
    };
    session::configure(preferences, thresholds);
    session::configure_values(
        settings
            .effective_values()
            .filter(|(key, _)| key.starts_with("spatial."))
            .map(|(key, value)| (key.to_owned(), value.clone()))
            .collect(),
    );
}

/// Whether the spatial layer is switched off for this session (spec v0.4 §47).
///
/// §47: "Disabling `spatial.enabled` MUST leave the typed shell and ordinary commands
/// functional." Off means the spatial *verbs* refuse — with `spatial.unsupported`, a named
/// refusal a script can branch on (§40) rather than a command that vanished — and the spatial
/// side effects of the ordinary ones stop happening: `enter <target>` still narrows the context
/// frame of v0.2 §14.3, and no longer moves a place nobody can look at.
///
/// The setting is read from the session every time rather than from the snapshot the view
/// preferences are taken from, because it is the one `spatial.*` key whose whole purpose is to
/// be flipped: a `set config spatial.enabled = false` must take effect on the next statement.
#[must_use]
pub fn disabled(session: &crate::session::Session) -> bool {
    session.settings().flag("spatial.enabled") == Some(false)
}

/// The refusal a spatial verb answers with while the layer is off (§40, §47).
#[must_use]
pub fn switched_off(command: &str) -> ono_value::ErrorValue {
    ono_value::ErrorValue::new(
        ono_core::ErrorCode::SpatialUnsupported,
        format!("`{command}` is a spatial command and `spatial.enabled` is false"),
    )
    .with_help("set `spatial.enabled = true` to navigate again (spec v0.4 §47)")
}

/// `help here` — what the place the session is standing in offers (spec v0.4 §38.2).
///
/// §38.1's overview says what the spatial verbs are; this says what they reach *from here*, which
/// is the half a general page cannot know. The exits are the ones this place actually has — read
/// from the live neighbourhood, not from the relation vocabulary — so a process names `children`
/// and `sockets` and COMPUTE names `processes` and `services`.
///
/// # Errors
///
/// Whatever the providers refused with while the neighbourhood was being observed.
pub fn here_help(
    session: &mut crate::session::Session,
) -> crate::eval::Eval<ono_command::TopicHelp> {
    let now = jiff::Timestamp::now();
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        crate::eval::Flow::Failed(ono_value::ErrorValue::new(
            ono_core::ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        ))
    })?;
    let observed = runtime.block_on(async {
        let mut state = spatial_session().await;
        let request = ono_spatial_query::NeighborhoodRequest::new().all(true);
        let neighborhood = view::neighborhood_here(providers, &mut state, &request, now).await?;
        let path = ono_spatial_query::resolve::concise_path(state.index(), state.current_place());
        let kind = state
            .index()
            .get(state.current_place())
            .map(|entry| entry.object().object_type().as_str().to_owned());
        let depth = state.trail().depth();
        Ok::<_, ono_value::ErrorValue>((path, kind, depth, neighborhood.0))
    });
    let (path, kind, depth, neighborhood) = observed.map_err(crate::eval::Flow::Failed)?;

    let mut entries: Vec<(String, String)> = Vec::new();
    for group in neighborhood.groups() {
        let label = group.label().to_owned();
        // §11.1: hierarchy is not the graph. A group reached by a relation is what `follow` and
        // `near` traverse; a canonical child is what `enter` moves into, and telling a reader to
        // follow one would be telling them to do something the shell refuses.
        let how = if group.relation().is_some() {
            format!("`near {label}`, `follow {label}`")
        } else {
            format!("`enter {label}`")
        };
        let line = match group.state() {
            ono_spatial_core::PermissionState::Available => match group.total() {
                // §2.17: an unknown count is not zero, and neither is worth calling a number.
                Some(held) => format!("{held} — {how}"),
                None => how,
            },
            ono_spatial_core::PermissionState::Empty => "no neighbour here".to_owned(),
            state => match group.detail() {
                Some(detail) => format!("{} — {detail}", state.as_str()),
                None => state.as_str().to_owned(),
            },
        };
        entries.push((label, line));
    }
    let mut see_also = vec![
        "look                   this place and its exits, in full".to_owned(),
        "map                    the topology around it".to_owned(),
    ];
    if kind.is_some() {
        see_also.push("up                     the canonical parent of this place".to_owned());
    }
    if depth > 0 {
        see_also.push("back                   the place you came from".to_owned());
    }
    see_also.push("pin <name>             keep this place as a landmark of your own".to_owned());
    see_also.push("help spatial           every spatial verb".to_owned());

    if entries.is_empty() {
        entries.push((
            "(none)".to_owned(),
            "this place offers no relationships — `enter` moves into what it holds".to_owned(),
        ));
    }
    Ok(ono_command::TopicHelp {
        name: "here".to_owned(),
        summary: match kind {
            Some(kind) => format!("{path} — a {kind} place, and what it offers (spec v0.4 §38.2)"),
            None => format!("{path} — a canonical space, and what it offers (spec v0.4 §38.2)"),
        },
        entries,
        see_also,
    })
}
