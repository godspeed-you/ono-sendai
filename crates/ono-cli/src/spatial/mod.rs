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
pub mod find;
pub mod map;
pub mod movement;
pub mod pins;
pub mod relations;
pub mod session;
pub mod storage;
pub mod view;

pub use commands::{Enter, Follow, Home, Look, Near, enter_observed};
pub use find::{FindPlace, local_scope, spatial_type};
pub use map::Map;
pub use movement::{Back, Jump, Trail, Up};
pub use pins::{PinPlace, PinStore, UnpinPlace, pin_path};
pub use session::{SpatialSessionState, spatial_session};

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
    let preferences = session::ViewPreferences {
        change_window: window
            .unwrap_or_else(|| ono_value::Duration::from_nanoseconds(5 * 60 * 1_000_000_000)),
        map_node_budget: usize::try_from(integer("spatial.map.node_budget", 100).max(1))
            .unwrap_or(ono_spatial_query::MAP_NODE_BUDGET),
    };
    session::configure(preferences, thresholds);
}
