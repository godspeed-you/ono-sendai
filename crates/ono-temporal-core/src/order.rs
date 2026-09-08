//! Causal ordering across systems (v0.5 §26).
//!
//! §26.1 defines happens-before as "an internal ordering relation supported by stronger evidence
//! than wall time". This module is where that sentence is enforced: [`happens_before`] answers
//! from a source sequence inside one stream or from a monotonic clock inside one boot, and from
//! nothing else. Two events whose only difference is what a wall clock said are
//! [`Ordering::Concurrent`], however far apart those readings are.

use crate::event::TemporalEvent;

/// What one event's relation to another is, as far as evidence supports (§26.1, §26.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Ordering {
    /// The first event happened before the second.
    Before,
    /// The first event happened after the second.
    After,
    /// Neither happens-before the other, so they are potentially concurrent (§26.3).
    Concurrent,
}

impl Ordering {
    /// The name a renderer and `inspect` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Ordering::Before => "before",
            Ordering::After => "after",
            Ordering::Concurrent => "concurrent",
        }
    }

    /// The same relation read from the other side.
    #[must_use]
    pub const fn inverse(self) -> Self {
        match self {
            Ordering::Before => Ordering::After,
            Ordering::After => Ordering::Before,
            Ordering::Concurrent => Ordering::Concurrent,
        }
    }
}

impl std::fmt::Display for Ordering {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What supports an ordering claim (§26.1).
///
/// The list is §26.1's own: "same sequence stream; action -> provider transaction; request ->
/// response; process creation parent event; explicit message/connection transaction identity",
/// plus the monotonic clock of §25.1. Wall time is deliberately absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderEvidence {
    /// Both events carry numbers from one source's own stream (§25.4).
    SourceSequence,
    /// Both events carry monotonic readings from one boot of one host (§25.1).
    Monotonic,
    /// A provider exposed a transaction identifier that spans both (§21.6).
    Transaction,
    /// An Ono action and the provider work it caused (§17.3).
    ActionChain,
    /// A kernel process-creation event naming the parent (§15.2).
    ParentProcess,
    /// A request and the response to it, across a connection identity (§26.4).
    RequestResponse,
}

impl OrderEvidence {
    /// Every kind of evidence, as §26.1 lists them.
    pub const ALL: &'static [OrderEvidence] = &[
        OrderEvidence::SourceSequence,
        OrderEvidence::Monotonic,
        OrderEvidence::Transaction,
        OrderEvidence::ActionChain,
        OrderEvidence::ParentProcess,
        OrderEvidence::RequestResponse,
    ];

    /// The name `inspect` spells when it shows what an ordering rests on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            OrderEvidence::SourceSequence => "source_sequence",
            OrderEvidence::Monotonic => "monotonic",
            OrderEvidence::Transaction => "transaction",
            OrderEvidence::ActionChain => "action_chain",
            OrderEvidence::ParentProcess => "parent_process",
            OrderEvidence::RequestResponse => "request_response",
        }
    }

    /// The evidence with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|evidence| evidence.as_str() == name)
    }
}

/// Whether `a` happens-before `b`, and what says so (§26.1).
///
/// Two events in different clock domains, or in one domain with no sequence and no monotonic
/// reading, are [`Ordering::Concurrent`]: §26.3 requires `inspect` not to claim semantic
/// ordering there, and the only way to keep that promise is to have no answer to give.
///
/// The remaining kinds of [`OrderEvidence`] — a transaction token, an action chain, a parent
/// process, a request and its response — are relations *between* events rather than facts *on*
/// one, so they arrive as [`crate::CausalLink`]s and are read from the ledger rather than
/// derived here.
#[must_use]
pub fn happens_before(a: &TemporalEvent, b: &TemporalEvent) -> (Ordering, Option<OrderEvidence>) {
    if a.event_id == b.event_id {
        return (Ordering::Concurrent, None);
    }
    if a.times.domain.is_comparable_to(&b.times.domain)
        && let (Some(here), Some(there)) = (a.times.monotonic_nanos, b.times.monotonic_nanos)
        && here != there
    {
        return (
            if here < there {
                Ordering::Before
            } else {
                Ordering::After
            },
            Some(OrderEvidence::Monotonic),
        );
    }
    if a.times.domain == b.times.domain
        && a.provenance.provider() == b.provenance.provider()
        && let (Some(here), Some(there)) = (a.times.source_sequence, b.times.source_sequence)
        && here != there
    {
        return (
            if here < there {
                Ordering::Before
            } else {
                Ordering::After
            },
            Some(OrderEvidence::SourceSequence),
        );
    }
    (Ordering::Concurrent, None)
}

/// Sorts events into a stable display order (§26.3).
///
/// "The timeline renderer MAY still choose stable display order, but `inspect` MUST not claim
/// semantic ordering." This is that display order and nothing more: by the instant a human
/// navigates by, then by the identity, so the same set draws the same way whatever order it
/// arrived in and no reader can mistake position for evidence.
pub fn presentation_order(events: &mut [TemporalEvent]) {
    events.sort_by(|a, b| {
        a.times
            .presentation_instant()
            .cmp(&b.times.presentation_instant())
            .then_with(|| a.event_id.as_str().cmp(b.event_id.as_str()))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_invert_when_an_ordering_is_read_from_the_other_side() {
        assert_eq!(Ordering::Before.inverse(), Ordering::After);
        assert_eq!(Ordering::Concurrent.inverse(), Ordering::Concurrent);
        for evidence in OrderEvidence::ALL {
            assert_eq!(OrderEvidence::from_name(evidence.as_str()), Some(*evidence));
        }
    }
}
