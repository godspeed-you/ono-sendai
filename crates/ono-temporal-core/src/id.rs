//! The temporal identities of v0.5 §3.3, §3.4, §17.3 and §42: event, evidence, action, causal
//! link and checkpoint.
//!
//! Every one of them is a content digest rather than a counter (ADR-0620), so two reports of one
//! observation carry one identity and §6.8's deduplication is a property of the identity rather
//! than of comparing rendered rows. What went into the digest is documented on the constructor
//! that builds it; the id itself stays opaque, because an id a user could compose by hand is an
//! identity rule that can be worked around.

use std::fmt;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialScope};

use crate::digest::{Digest, token};
use crate::event::{EventSeed, SpatialRef};
use crate::evidence::EvidenceClaim;
use crate::source::EvidenceSource;

/// How many hex digits an event, evidence, link or checkpoint id carries.
const LONG: usize = 24;
/// How many hex digits an action id carries — §17.3's `ono:a91f` is a short, quotable token.
const SHORT: usize = 16;

/// Whether `text` is `1..=digits` lowercase hex characters.
fn is_hex(text: &str, digits: usize) -> bool {
    !text.is_empty()
        && text.len() <= digits
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The stable identity of a temporal event (§3.3), rendered `@e<hex>` (§11.6).
///
/// A reference may be shortened: §11.6's `@e42` is a prefix, and the ledger resolves it, raising
/// `temporal.ambiguous_event` when it names more than one event (§34).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId(Arc<str>);

impl EventId {
    /// The content digest of an event: the facts that make it *this* observation.
    ///
    /// The parts are the source that reported it, the kind, the subject, the source's own time,
    /// the observing component's time, the source sequence, the clock domain, and a digest of
    /// the body — subtype, scope, related subjects, before, after, changed fields and payload.
    ///
    /// `ingested_at` is deliberately not among them: when a ledger received a report is a fact
    /// about the ledger, and including it would give one observation two identities across a
    /// restart, which is exactly what §6.8 asks to avoid.
    #[must_use]
    pub fn of(seed: &EventSeed) -> Self {
        let mut body = Digest::new("ono.temporal-event.body/1");
        body.optional("subtype", seed.subtype.as_deref());
        body.part("scope", &seed.scope.to_string());
        for (index, related) in seed.related.iter().enumerate() {
            body.part(&format!("related.{index}"), &subject_token(Some(related)));
        }
        body.optional("before", seed.before.as_ref().map(token).as_deref());
        body.optional("after", seed.after.as_ref().map(token).as_deref());
        for change in &seed.changed_fields {
            body.part(
                &format!("changed.{}", change.field),
                &format!(
                    "{}\u{1}{}\u{1}{}",
                    change.before.as_ref().map(token).unwrap_or_default(),
                    change.after.as_ref().map(token).unwrap_or_default(),
                    change.certainty.as_str()
                ),
            );
        }
        body.optional("payload", seed.payload.as_ref().map(token).as_deref());

        let mut digest = Digest::new("ono.temporal-event/1");
        digest.part("source", seed.provenance.provider());
        digest.part("kind", seed.kind.as_str());
        digest.part("subject", &subject_token(seed.subject.as_ref()));
        digest.optional(
            "source_time",
            seed.times
                .source_time
                .map(|time| time.as_nanosecond().to_string())
                .as_deref(),
        );
        digest.part(
            "observed_at",
            &seed.times.observed_at.as_nanosecond().to_string(),
        );
        digest.optional(
            "source_sequence",
            seed.times.source_sequence.map(|s| s.to_string()).as_deref(),
        );
        digest.part("host", &seed.times.domain.host);
        digest.optional("boot_id", seed.times.domain.boot_id.as_deref());
        digest.part("body", &body.finish(32));
        Self(format!("e{}", digest.finish(LONG / 2)).into())
    }

    /// Reads an event reference back — `@e…` as a renderer writes it, or the bare `e…`.
    ///
    /// Returns `None` for anything else, so a hand-written string cannot become an event.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bare = text.strip_prefix('@').unwrap_or(text);
        let digits = bare.strip_prefix('e')?;
        is_hex(digits, LONG).then(|| Self(bare.into()))
    }

    /// The identity as text, without the `@` a renderer adds.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this reference is a complete identity rather than a shortened one (§11.6).
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.0.len() == LONG + 1
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// The stable identity of an evidence record (§3.4), rendered `@v<hex>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EvidenceId(Arc<str>);

impl EvidenceId {
    /// The content digest of an observation: who observed it, when, about what, and what it claims.
    #[must_use]
    pub fn of(
        source: &EvidenceSource,
        observed_at: Timestamp,
        scope: &SpatialScope,
        subject: Option<&SpatialId>,
        claim: &EvidenceClaim,
    ) -> Self {
        let mut digest = Digest::new("ono.temporal-evidence/1");
        digest.part("source", source.as_str());
        digest.part("observed_at", &observed_at.as_nanosecond().to_string());
        digest.part("scope", &scope.to_string());
        digest.optional("subject", subject.map(SpatialId::as_str));
        digest.part("claim", &claim.digest_token());
        Self(format!("v{}", digest.finish(LONG / 2)).into())
    }

    /// Reads an evidence reference back — `@v…` or the bare `v…`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bare = text.strip_prefix('@').unwrap_or(text);
        let digits = bare.strip_prefix('v')?;
        (digits.len() == LONG && is_hex(digits, LONG)).then(|| Self(bare.into()))
    }

    /// The identity as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EvidenceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// The identity of a mutation requested through Ono (§17.3), rendered `ono:a<hex>`.
///
/// §17.3 requires it to exist *before* execution and to be propagated through provider calls, so
/// it is derived from the request rather than from anything the result carries.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActionId(Arc<str>);

impl ActionId {
    /// The identity of the request: the session, the instant, the operation and the target.
    #[must_use]
    pub fn of(
        session_id: &str,
        requested_at: Timestamp,
        operation: &str,
        target: Option<&SpatialId>,
    ) -> Self {
        let mut digest = Digest::new("ono.action-event/1");
        digest.part("session", session_id);
        digest.part("requested_at", &requested_at.as_nanosecond().to_string());
        digest.part("operation", operation);
        digest.optional("target", target.map(SpatialId::as_str));
        Self(format!("a{}", digest.finish(SHORT / 2)).into())
    }

    /// Reads an action reference back — `ono:a…` as §17.3 renders it, or the bare `a…`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bare = text.strip_prefix("ono:").unwrap_or(text);
        let digits = bare.strip_prefix('a')?;
        (digits.len() == SHORT && is_hex(digits, SHORT)).then(|| Self(bare.into()))
    }

    /// The identity as text, without the `ono:` prefix a renderer adds.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ActionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ono:{}", self.0)
    }
}

/// The identity of a causal link (§15), rendered `@l<hex>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CausalLinkId(Arc<str>);

impl CausalLinkId {
    /// The content digest of a link: the rule that emitted it, its class and both ends.
    ///
    /// One rule emitting one relation between one pair of events produces one link, however many
    /// times it runs, so re-deriving causality over a window does not multiply edges.
    #[must_use]
    pub fn of(
        rule: &crate::causal::CausalRuleId,
        relation: crate::causal::CausalRelation,
        cause: &EventId,
        effect: &EventId,
    ) -> Self {
        let mut digest = Digest::new("ono.causal-link/1");
        digest.part("rule", rule.as_str());
        digest.part("relation", relation.as_str());
        digest.part("cause", cause.as_str());
        digest.part("effect", effect.as_str());
        Self(format!("l{}", digest.finish(LONG / 2)).into())
    }

    /// Reads a link reference back — `@l…` or the bare `l…`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bare = text.strip_prefix('@').unwrap_or(text);
        let digits = bare.strip_prefix('l')?;
        (digits.len() == LONG && is_hex(digits, LONG)).then(|| Self(bare.into()))
    }

    /// The identity as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CausalLinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// The identity of a checkpoint (§3.6, §42), rendered `@k<hex>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CheckpointId(Arc<str>);

impl CheckpointId {
    /// The content digest of a checkpoint: the scope it projects and the instant it captured.
    #[must_use]
    pub fn of(scope: &SpatialScope, captured_at: Timestamp) -> Self {
        let mut digest = Digest::new("ono.checkpoint/1");
        digest.part("scope", &scope.to_string());
        digest.part("captured_at", &captured_at.as_nanosecond().to_string());
        Self(format!("k{}", digest.finish(LONG / 2)).into())
    }

    /// Reads a checkpoint reference back — `@k…` or the bare `k…`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let bare = text.strip_prefix('@').unwrap_or(text);
        let digits = bare.strip_prefix('k')?;
        (digits.len() == LONG && is_hex(digits, LONG)).then(|| Self(bare.into()))
    }

    /// The identity as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "@{}", self.0)
    }
}

/// What a subject contributes to an identity.
///
/// §5.5 keeps a resolved subject and a subject a source only *named* apart, and so does this: an
/// event Ono could not reconcile is a different observation from the one it might have been.
fn subject_token(subject: Option<&SpatialRef>) -> String {
    match subject {
        None => "\u{0}".to_owned(),
        Some(SpatialRef::Resolved { id, .. }) => format!("resolved\u{1}{}", id.as_str()),
        Some(SpatialRef::Unresolved { source, described }) => {
            format!("unresolved\u{1}{}\u{1}{described}", source.as_str())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_a_reference_when_it_carries_a_digit_no_digest_produces() {
        assert!(EventId::parse("@eABCDEF").is_none(), "the hex is lowercase");
        assert!(EventId::parse("@e-1").is_none());
        assert!(
            ActionId::parse("ono:a0123").is_none(),
            "an action id is exact"
        );
        assert!(
            EvidenceId::parse("@v01").is_none(),
            "an evidence id is exact"
        );
    }

    #[test]
    fn should_separate_an_absent_part_from_an_empty_one_when_a_digest_is_built() {
        let mut absent = Digest::new("test");
        absent.optional("x", None);
        let mut empty = Digest::new("test");
        empty.optional("x", Some(""));
        assert_ne!(absent.finish(12), empty.finish(12));
    }
}
