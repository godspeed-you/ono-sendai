//! What is known about one field at one instant, and what is not (v0.5 §9.2, §9.3).
//!
//! §9.2 is the rule this module exists for. Between an observation at 12:00 and one at 12:10,
//! "Ono MUST NOT claim the state at 12:05 unless evidence supports it" — it may report
//! `unknown in interval 12:00..12:10`, naming what was last seen and what was seen next.
//! [`FieldKnowledge`] is that sentence as a type, so a renderer cannot flatten it into a value
//! and a caller cannot read a guess as a reading.
//!
//! §9.3 permits `valid_from` and `valid_until` where the source's own semantics support an
//! interval, and forbids deriving them "from guessed midpoint interpolation". Both ends here
//! therefore come from an observation instant or from the end of a source's own complete
//! coverage, never from arithmetic between two readings.

use std::sync::Arc;

use jiff::Timestamp;
use ono_temporal_core::{ChangeCertainty, EvidenceSource, EvidenceStrength, TemporalCompleteness};
use ono_value::Value;

/// Whether something was there at the reconstructed instant (§9.5, §9.7).
///
/// The third variant is the point. §9.5: "unknown relation state MUST be distinguishable from
/// absent relation state", and §7.4 says the same about objects — an absence is a claim that
/// needs coverage capable of proving it, so "nothing observed it" and "it was not there" are two
/// answers rather than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Presence {
    /// Reconstruction supports that it was there.
    Present,
    /// Coverage capable of proving absence says it was not there.
    Absent,
    /// Nothing observed it and nothing could have proven its absence.
    Unknown,
}

impl Presence {
    /// The name a renderer and `inspect` spell.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Presence::Present => "present",
            Presence::Absent => "absent",
            Presence::Unknown => "unknown",
        }
    }

    /// Whether reconstruction supports its existence at the requested instant.
    #[must_use]
    pub const fn is_present(self) -> bool {
        matches!(self, Presence::Present)
    }

    /// Whether the answer is a claim at all, either way.
    #[must_use]
    pub const fn is_known(self) -> bool {
        !matches!(self, Presence::Unknown)
    }
}

impl std::fmt::Display for Presence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One reading of a field: what a source saw, when, and which source saw it.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// What the field held.
    pub value: Value,
    /// When the source saw it.
    pub at: Timestamp,
    /// The §7.1 source that saw it.
    pub source: EvidenceSource,
}

/// What reconstruction can say about one field at the requested instant (§9.2).
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKnowledge {
    /// Evidence supports this value at the requested instant.
    Known {
        /// The value.
        value: Value,
        /// When the interval it holds over begins, where source semantics support one (§9.3).
        valid_from: Option<Timestamp>,
        /// When that interval ends, where source semantics support one (§9.3).
        valid_until: Option<Timestamp>,
    },
    /// The requested instant sits between two readings and nothing proves what happened in
    /// between — §9.2's `unknown in interval`, with both ends named.
    UnknownInInterval {
        /// What was last seen at or before the requested instant.
        last: Option<Observation>,
        /// What was seen next after it.
        next: Option<Observation>,
    },
    /// No source spoke to this field in the window at all.
    Unobserved,
}

impl FieldKnowledge {
    /// The supported value, or `None` where there is none.
    #[must_use]
    pub const fn value(&self) -> Option<&Value> {
        match self {
            FieldKnowledge::Known { value, .. } => Some(value),
            _ => None,
        }
    }

    /// Whether evidence supports a value at the requested instant.
    #[must_use]
    pub const fn is_known(&self) -> bool {
        matches!(self, FieldKnowledge::Known { .. })
    }

    /// The value as an Ono value: the reading where there is one, unknown otherwise.
    ///
    /// v0.2 §10.5 makes [`Value::Null`] the word for "known to be unknown", which reads back as
    /// [`ono_value::FieldAccess::Unknown`]. A zero or an empty string here would be a fabrication.
    #[must_use]
    pub fn to_value(&self) -> Value {
        self.value().cloned().unwrap_or(Value::Null)
    }
}

/// One reconstructed field, with where it came from and how well it is covered (§8.5, §9.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ReconstructedField {
    name: Arc<str>,
    knowledge: FieldKnowledge,
    completeness: TemporalCompleteness,
    certainty: ChangeCertainty,
    strength: EvidenceStrength,
    sources: Vec<EvidenceSource>,
}

impl ReconstructedField {
    /// A field with this knowledge behind it.
    #[must_use]
    pub fn new(
        name: Arc<str>,
        knowledge: FieldKnowledge,
        completeness: TemporalCompleteness,
        certainty: ChangeCertainty,
        strength: EvidenceStrength,
        sources: Vec<EvidenceSource>,
    ) -> Self {
        Self {
            name,
            knowledge,
            completeness,
            certainty,
            strength,
            sources,
        }
    }

    /// The field's name, under the name its own schema gives it.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// What reconstruction can say about it (§9.2).
    #[must_use]
    pub const fn knowledge(&self) -> &FieldKnowledge {
        &self.knowledge
    }

    /// The supported value, or `None`.
    #[must_use]
    pub const fn value(&self) -> Option<&Value> {
        self.knowledge.value()
    }

    /// The value as an Ono value: unknown stays unknown.
    #[must_use]
    pub fn to_value(&self) -> Value {
        self.knowledge.to_value()
    }

    /// When the supported interval begins (§9.3).
    #[must_use]
    pub const fn valid_from(&self) -> Option<Timestamp> {
        match &self.knowledge {
            FieldKnowledge::Known { valid_from, .. } => *valid_from,
            _ => None,
        }
    }

    /// When the supported interval ends (§9.3).
    #[must_use]
    pub const fn valid_until(&self) -> Option<Timestamp> {
        match &self.knowledge {
            FieldKnowledge::Known { valid_until, .. } => *valid_until,
            _ => None,
        }
    }

    /// What the sources behind this field composed to over the window (§8.5).
    #[must_use]
    pub const fn completeness(&self) -> TemporalCompleteness {
        self.completeness
    }

    /// How the value was arrived at (§6.2).
    #[must_use]
    pub const fn certainty(&self) -> ChangeCertainty {
        self.certainty
    }

    /// How strongly the evidence behind it supports it (§7.2). Nothing raises it.
    #[must_use]
    pub const fn strength(&self) -> EvidenceStrength {
        self.strength
    }

    /// Every source that spoke to this field.
    pub fn sources(&self) -> impl Iterator<Item = &EvidenceSource> {
        self.sources.iter()
    }
}
