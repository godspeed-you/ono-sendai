//! Completion as spatial discovery (spec v0.4 §9.1, §9.4).
//!
//! §9.4: "completion MUST prioritize services visible in the current neighborhood and then offer
//! broader matches", and after `follow` it "MUST show actual available relation types". Its
//! closing sentence is the design: "Completion is therefore not merely token completion; it is a
//! lightweight local map."
//!
//! So what is offered here is what the session can actually see from where it stands — the
//! canonical geography of the current place, which is declared and costs nothing, and the places
//! and relations this session has already observed. Nothing is asked of a provider while the user
//! is holding Tab: §34 budgets 50 ms for the first results from local metadata, and an offer that
//! blocked on `/proc` would be neither local nor metadata. A neighbourhood nobody has looked at
//! yet is therefore offered as its declared geography and no more, which is honest — it is
//! exactly what the shell knows.

use ono_spatial_core::{PermissionState, space};

/// One thing completion offers: the word it inserts, and the line it shows while offering it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The text that replaces the word under the cursor.
    pub insert: String,
    /// The line the listing shows — the word, and the compact count or state §9.4 allows.
    pub line: String,
}

impl Offer {
    fn new(insert: impl Into<String>, detail: &str) -> Self {
        let insert = insert.into();
        let line = if detail.is_empty() {
            format!("  {insert}")
        } else {
            format!("  {insert:<24} {detail}")
        };
        Self { insert, line }
    }
}

/// How many observed neighbours a single Tab offers before the list stops being a map.
const HORIZON: usize = 24;

/// The places `enter` can reach from where the session is standing (§9.4, §24.2).
#[must_use]
pub fn places_here() -> Vec<Offer> {
    if !crate::spatial::session::configured_flag_or("spatial.enabled", true) {
        // §47: with the layer off there is no neighbourhood to complete from, and offering one
        // would promise a `enter compute` that refuses.
        return Vec::new();
    }
    let Ok(state) = crate::spatial::session::session_state().try_lock() else {
        return Vec::new();
    };
    let here = state.current_place().clone();
    let index = state.index();

    if let Some(space) = ono_spatial_query::resolve::space_of(&here) {
        // The declared geography first: §24.2 makes a canonical child a navigation target
        // whether or not anybody has looked inside it yet.
        let mut offers: Vec<Offer> = space::children(space.id)
            .filter(|child| child.is_served() && child.enterable)
            .map(|child| Offer::new(child.label.to_ascii_lowercase(), ""))
            .collect();
        // Then what this session has actually seen filed under this place, in a fixed order and
        // bounded — the same two rules the map obeys, for the same reason (§34.2).
        let mut seen: Vec<Offer> = index
            .entries()
            .filter(|entry| {
                entry
                    .canonical_parent()
                    .is_some_and(|edge| edge.parent() == &here)
            })
            .map(|entry| Offer::new(entry.object().display_name(), ""))
            .collect();
        seen.sort_by(|left, right| left.insert.cmp(&right.insert));
        seen.truncate(HORIZON);
        offers.append(&mut seen);
        offers.dedup_by(|left, right| left.insert == right.insert);
        return offers;
    }

    // At an observed object the neighbourhood is the graph: the places its edges reach.
    let Some(entry) = index.get(&here) else {
        return Vec::new();
    };
    let mut offers: Vec<Offer> = entry
        .edges()
        .iter()
        .filter_map(|edge| index.get(edge.target()))
        .map(|neighbour| {
            Offer::new(
                neighbour.object().display_name(),
                neighbour.object().object_type().as_str(),
            )
        })
        .collect();
    offers.sort_by(|left, right| left.insert.cmp(&right.insert));
    offers.dedup_by(|left, right| left.insert == right.insert);
    offers.truncate(HORIZON);
    offers
}

/// The relations the current place actually has, with the count or state §9.4 allows (§3.5, §12).
#[must_use]
pub fn relations_here() -> Vec<Offer> {
    if !crate::spatial::session::configured_flag_or("spatial.enabled", true) {
        return Vec::new();
    }
    let Ok(state) = crate::spatial::session::session_state().try_lock() else {
        return Vec::new();
    };
    let here = state.current_place().clone();
    let now = jiff::Timestamp::now();
    state
        .index()
        .relation_summary(&here, usize::MAX, now)
        .into_iter()
        .map(|group| {
            // §35.2: a group that could not be read says why instead of showing a number, here
            // exactly as in `look` — a count of zero would claim the relation is empty.
            let detail = match group.state() {
                PermissionState::Available => group.members().len().to_string(),
                PermissionState::Empty => "0".to_owned(),
                other => other.as_str().replace('_', " "),
            };
            Offer::new(group.label(), &detail)
        })
        .collect()
}
