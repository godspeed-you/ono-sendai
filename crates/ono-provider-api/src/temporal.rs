//! What a provider can answer about time, and the window a historical query is asked over.
//!
//! Spec v0.5 §21.1 gives every provider a fourth thing to say beyond "state now", "changes over
//! time" and the runtime-managed watch: what it can answer *about the past*, and how honestly.
//! The vocabulary is closed and every field defaults to false, so a provider that says nothing
//! claims nothing — which is the difference between a source that cannot prove a thing was absent
//! and one that quietly implies it could (§7.4, §21.5).

use ono_value::{Duration, ErrorValue};

/// What a provider can answer about time (spec v0.5 §21.1).
///
/// Each field is a separate claim, and each is checked separately, because they fail separately:
/// journald answers historical queries and emits no live events, netlink emits live events and
/// answers nothing about the past, and procfs does neither (§22.1, §22.3, §22.4).
///
/// ```
/// use ono_provider_api::TemporalCapabilities;
///
/// // A provider that says nothing claims nothing.
/// let silent = TemporalCapabilities::default();
/// assert!(!silent.historical_query);
/// assert!(!silent.exhaustive_events);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TemporalCapabilities {
    /// The provider can report the state of its objects as they are now.
    ///
    /// Every v0.2 provider can, and v0.5 uses it for checkpoints and for comparison (§21.2).
    pub current_snapshot: bool,
    /// The provider emits changes as its source produces them, rather than being polled (§21.3).
    pub live_events: bool,
    /// The provider can answer directly about past state or past events (§21.4).
    pub historical_query: bool,
    /// Sequence continuity is strong enough to support an absence or change claim (§21.5).
    ///
    /// This is the strongest claim in the set. §21.5: "Providers MUST NOT advertise it merely
    /// because events usually arrive." A polled source may never claim it.
    pub exhaustive_events: bool,
    /// The provider carries transaction, job or action identifiers that support a direct causal
    /// link rather than a temporal coincidence (§21.6, §15.2).
    pub causal_tokens: bool,
    /// The provider's current snapshot can be serialised into the temporal store with canonical
    /// identity and provenance (§21.7).
    pub checkpointable: bool,
    /// How far back the source itself keeps history, where it states a bound.
    ///
    /// `None` means the provider does not know, which is not the same as "forever" — a
    /// `historical_query` provider with no stated retention answers what it has and says nothing
    /// about what it no longer has.
    pub retained_history: Option<Duration>,
}

impl TemporalCapabilities {
    /// A provider that claims nothing about time.
    ///
    /// The default for every provider, and the honest answer for one that has not thought about
    /// it: nothing downstream may then treat its silence as coverage.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            current_snapshot: false,
            live_events: false,
            historical_query: false,
            exhaustive_events: false,
            causal_tokens: false,
            checkpointable: false,
            retained_history: None,
        }
    }

    /// A snapshot source: it can say what is true now, and nothing about what was.
    ///
    /// This is procfs, and §22.1 is explicit that it may claim no more: "The reference provider
    /// MUST NOT claim native historical process coverage."
    #[must_use]
    pub const fn snapshot_only() -> Self {
        Self {
            current_snapshot: true,
            ..Self::none()
        }
    }
}

/// The interval a historical query is asked over (spec v0.5 §21.4).
///
/// Both ends are optional, so a window is closed, half-open in either direction, or unbounded.
/// An absent end is "as far as the source goes", never a substituted clock reading: nothing in a
/// provider reads the clock to fill one in (§39.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TimeWindow {
    /// The earliest instant the query is interested in, inclusive. `None` is "from the start of
    /// whatever the source retains".
    pub from: Option<jiff::Timestamp>,
    /// The latest instant the query is interested in, exclusive. `None` is "up to the present".
    pub until: Option<jiff::Timestamp>,
}

impl TimeWindow {
    /// A window with both ends stated.
    #[must_use]
    pub const fn between(from: jiff::Timestamp, until: jiff::Timestamp) -> Self {
        Self {
            from: Some(from),
            until: Some(until),
        }
    }

    /// Everything the source retains, in both directions.
    #[must_use]
    pub const fn unbounded() -> Self {
        Self {
            from: None,
            until: None,
        }
    }

    /// Everything from `from` onwards.
    #[must_use]
    pub const fn since(from: jiff::Timestamp) -> Self {
        Self {
            from: Some(from),
            until: None,
        }
    }

    /// Whether `at` falls inside the window.
    ///
    /// The lower bound is inclusive and the upper bound exclusive, so two adjacent windows
    /// neither overlap nor leave a hole between them.
    #[must_use]
    pub fn contains(&self, at: jiff::Timestamp) -> bool {
        self.from.is_none_or(|from| at >= from) && self.until.is_none_or(|until| at < until)
    }
}

/// The refusal a provider that keeps no history answers a historical query with (§21.4, §34).
///
/// It is one function rather than one sentence per provider so that "this source cannot answer
/// about the past" reads the same everywhere it is said, and so that a provider whose history
/// depends on something that may be missing — journald without a journal — refuses in the same
/// words as one that never had any.
#[must_use]
pub fn unsupported_history(provider_id: &str) -> ErrorValue {
    ErrorValue::new(
        ono_core::ErrorCode::TemporalUnsupportedSource,
        format!("{provider_id} cannot answer about the past"),
    )
    .with_help(
        "a provider that keeps history says so in its temporal capabilities; this one answers \
         about the present only, so the past has to come from where it was recorded",
    )
}
