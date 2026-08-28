//! The navigation trail (spec v0.4 §20.1, §20.3, §2.4).
//!
//! "The spatial trail is separate from command history" (§20.1) and "Every movement is
//! reversible: `back` MUST return through the actual navigation trail where the previous
//! location still exists" (§2.4). The trail is therefore an append-only record of movements —
//! including the `back`s — beside a stack of the places a user is standing on top of. Going back
//! never erases the record it went back through (§20.3).

use std::sync::Arc;

use jiff::Timestamp;

use crate::{RelationType, ScopeBoundary, SpatialId};

/// How a place was reached (§20.1's `movement`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Movement {
    /// Down the hierarchy, or into a selected object (§6.3).
    Enter,
    /// Along a relationship edge (§6.4).
    Follow,
    /// Directly to a resolved place, possibly across a scope (§6.5).
    Jump,
    /// Back through the trail (§6.6).
    Back,
    /// Up the canonical hierarchy (§6.6).
    Up,
    /// To the root (§6.6).
    Home,
}

impl Movement {
    /// Every movement, as §20.1 lists them.
    pub const ALL: &'static [Movement] = &[
        Movement::Enter,
        Movement::Follow,
        Movement::Jump,
        Movement::Back,
        Movement::Up,
        Movement::Home,
    ];

    /// The name the trail and `docs/spec/spatial/spatial.yaml` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Movement::Enter => "enter",
            Movement::Follow => "follow",
            Movement::Jump => "jump",
            Movement::Back => "back",
            Movement::Up => "up",
            Movement::Home => "home",
        }
    }

    /// The movement with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|movement| movement.as_str() == name)
    }

    /// Whether the movement pushes a place a later `back` can return to.
    ///
    /// `back` does not: going back and then back again reaches the place before, not the one
    /// just left, which is what makes `back` an undo rather than a toggle (§2.4).
    ///
    /// `home` does not either, for the same reason. §6.6 groups the two as returns rather than
    /// as departures: `home` ends an excursion by going to the root the excursion started from,
    /// so the place a later `back` returns to is the one *before* that excursion. Pushing the
    /// place `home` left would make `back` bounce between the root and it — exactly the toggle
    /// `back` is defined not to be (ADR-0170).
    #[must_use]
    pub fn extends_history(self) -> bool {
        !matches!(self, Movement::Back | Movement::Home)
    }
}

/// One movement, exactly as §20.1 schemas it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationStep {
    timestamp: Timestamp,
    from: SpatialId,
    to: SpatialId,
    movement: Movement,
    relation: Option<RelationType>,
    word: Option<Arc<str>>,
    scope_crossing: Option<ScopeBoundary>,
}

impl NavigationStep {
    /// A step from `from` to `to` by `movement`, at `timestamp`.
    #[must_use]
    pub fn new(timestamp: Timestamp, from: SpatialId, to: SpatialId, movement: Movement) -> Self {
        Self {
            timestamp,
            from,
            to,
            movement,
            relation: None,
            word: None,
            scope_crossing: None,
        }
    }

    /// Records which relation the movement followed (§6.4).
    #[must_use]
    pub fn along(mut self, relation: RelationType) -> Self {
        self.relation = Some(relation);
        self
    }

    /// Records the word the traversal was spelled with — `socket`, `parent`, `owner`.
    ///
    /// A relation has one declared id and two ends, and the two ends are two different words
    /// (§41.2's `canonical_label` and `inverse_label`). The id alone therefore does not say which
    /// way the movement went, so the word `follow` took is kept beside it: that is what §6.7's
    /// trail shows and what tells a `socket` hop from the `owner` hop back.
    #[must_use]
    pub fn spelled(mut self, word: impl Into<Arc<str>>) -> Self {
        self.word = Some(word.into());
        self
    }

    /// Records the scope boundary the movement crossed, which §2.18 requires to be visible.
    #[must_use]
    pub fn crossing(mut self, boundary: ScopeBoundary) -> Self {
        self.scope_crossing = Some(boundary);
        self
    }

    /// When the movement happened.
    #[must_use]
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Where it started.
    #[must_use]
    pub fn from(&self) -> &SpatialId {
        &self.from
    }

    /// Where it arrived.
    #[must_use]
    pub fn to(&self) -> &SpatialId {
        &self.to
    }

    /// Which movement it was.
    #[must_use]
    pub fn movement(&self) -> Movement {
        self.movement
    }

    /// The relation followed, for a `follow`.
    #[must_use]
    pub fn relation(&self) -> Option<&RelationType> {
        self.relation.as_ref()
    }

    /// The word the traversal was spelled with, where one was recorded.
    #[must_use]
    pub fn word(&self) -> Option<&str> {
        self.word.as_deref()
    }

    /// The scope boundary crossed, where one was.
    #[must_use]
    pub fn scope_crossing(&self) -> Option<&ScopeBoundary> {
        self.scope_crossing.as_ref()
    }
}

/// What a `back` did (§20.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackOutcome {
    /// The previous place still exists and the trail returned to it.
    Returned {
        /// The place returned to.
        to: SpatialId,
        /// The step the trail recorded for the return.
        step: NavigationStep,
    },
    /// Places on the way back no longer exist, and the trail skipped past them to the nearest one
    /// that does. §20.3 requires the user to be told, which is why the skipped places are here.
    Skipped {
        /// The place returned to.
        to: SpatialId,
        /// The places that no longer exist, most recent first.
        skipped: Vec<SpatialId>,
        /// The step the trail recorded for the return.
        step: NavigationStep,
    },
    /// Nothing on the trail still exists. §20.3's step 2 has nowhere to arrive, so the caller
    /// reports `spatial.destination_gone` (§40) and stays where it is.
    AllGone {
        /// The places that no longer exist, most recent first.
        skipped: Vec<SpatialId>,
    },
    /// The trail holds no earlier place: `spatial.history_empty` (§40).
    Empty,
}

/// The trail of one session (§20.1, §46).
///
/// It is session-local by default: §46.1 disables trail persistence "for privacy and
/// stale-identity reasons", and `spatial.trail.persist` is the setting that changes that.
#[derive(Debug, Clone)]
pub struct NavigationTrail {
    steps: Vec<NavigationStep>,
    history: Vec<SpatialId>,
    current: SpatialId,
}

impl NavigationTrail {
    /// A trail that starts at `start` — the local `SYSTEM` root, by default (§46.1).
    #[must_use]
    pub fn new(start: SpatialId) -> Self {
        Self {
            steps: Vec::new(),
            history: Vec::new(),
            current: start,
        }
    }

    /// Where the session is standing.
    #[must_use]
    pub fn current(&self) -> &SpatialId {
        &self.current
    }

    /// Every movement, oldest first — including the `back`s, which §20.3 requires to be retained.
    #[must_use]
    pub fn steps(&self) -> &[NavigationStep] {
        &self.steps
    }

    /// How deep the trail is: how many places a `back` could still return through.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.history.len()
    }

    /// Records a movement and moves the session to its destination.
    ///
    /// The step's `from` is rewritten to where the session actually is, so a caller cannot record
    /// a movement that did not happen.
    pub fn record(&mut self, step: NavigationStep) {
        let step = NavigationStep {
            from: self.current.clone(),
            ..step
        };
        if step.movement.extends_history() {
            self.history.push(self.current.clone());
        }
        self.current = step.to.clone();
        self.steps.push(step);
    }

    /// The most recent movement, if any.
    #[must_use]
    pub fn last_step(&self) -> Option<&NavigationStep> {
        self.steps.last()
    }

    /// Moves back through the trail, skipping places that no longer exist (§20.3).
    ///
    /// `exists` answers whether a place is still there. §20.3's order is followed exactly: return
    /// where the previous location still exists; otherwise skip to the nearest valid previous
    /// place — and the skipped places come back in the outcome so the caller can inform the user
    /// before it happens, rather than moving somewhere unannounced. The trail record is retained
    /// either way.
    pub fn back(&mut self, at: Timestamp, exists: impl Fn(&SpatialId) -> bool) -> BackOutcome {
        if self.history.is_empty() {
            return BackOutcome::Empty;
        }
        let mut skipped = Vec::new();
        while let Some(previous) = self.history.pop() {
            if exists(&previous) {
                let step =
                    NavigationStep::new(at, self.current.clone(), previous.clone(), Movement::Back);
                self.current = previous.clone();
                self.steps.push(step.clone());
                return if skipped.is_empty() {
                    BackOutcome::Returned { to: previous, step }
                } else {
                    BackOutcome::Skipped {
                        to: previous,
                        skipped,
                        step,
                    }
                };
            }
            skipped.push(previous);
        }
        BackOutcome::AllGone { skipped }
    }

    /// The places a `back` would return through, most recent first.
    ///
    /// This is what `trail` renders and what a breadcrumb is built from (§20.2).
    pub fn history(&self) -> impl DoubleEndedIterator<Item = &SpatialId> {
        self.history.iter().rev()
    }

    /// The scope boundaries the session has crossed, oldest first (§2.18).
    pub fn crossings(&self) -> impl Iterator<Item = &ScopeBoundary> {
        self.steps.iter().filter_map(NavigationStep::scope_crossing)
    }
}
