//! Historical provider merge (spec v0.5 §9.1, §21.4): folding a provider-owned answer about the
//! past into replayed state, with per-field provenance and coverage surviving the merge.
//!
//! §21.4 lets a provider answer directly about the past — journald holds its own history, a
//! cluster API keeps its own record — and §9.1 replays the ledger to reach the same instant. Both
//! answers are legitimate, and the merge has one rule: the stronger evidence wins, by the
//! ordering [`EvidenceStrength`] fixes, and never by which side was passed first.
//!
//! §7.2 bounds what a merge may do to strength: "evidence strength MUST NOT be automatically
//! upgraded". A [`MergedField`] therefore carries the winner's own strength. Two sources
//! agreeing does not make a claim authoritative, and this module has no operation that would say
//! it does.
//!
//! What survives the merge is per field, because §8.5 requires coverage to be computed "per
//! field/relation rather than simply selecting the strongest global label": each answer keeps its
//! own source, its own coverage and its own evidence, and every source that answered is named in
//! [`MergedField::contributors`] so `inspect` can show the ones that lost.

use std::sync::Arc;

use jiff::Timestamp;
use ono_temporal_core::{EvidenceId, EvidenceSource, EvidenceStrength, TemporalCompleteness};
use ono_value::Value;

/// One source's answer about one field at the queried instant (§9.1, §21.4).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldAnswer {
    /// The field, under the name its schema gives it.
    pub field: Arc<str>,
    /// What the source says it held. `None` where the source reached the field and saw nothing,
    /// which reads back as unknown rather than as a zero (v0.2 §10.5).
    pub value: Option<Value>,
    /// How strongly the source supports the claim (§7.2).
    pub strength: EvidenceStrength,
    /// The §7.1 source that answered.
    pub source: EvidenceSource,
    /// What the source could observe over the interval the answer is about (§3.5).
    pub coverage: TemporalCompleteness,
    /// The evidence records behind the answer (§7.3).
    pub evidence: Vec<EvidenceId>,
    /// When the source observed it, which is how a tie in strength is broken.
    pub observed_at: Option<Timestamp>,
}

/// The field one merge produced, and what it rests on.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedField {
    /// The field.
    pub field: Arc<str>,
    /// The winning value.
    pub value: Option<Value>,
    /// The winner's own strength. Nothing here raises it (§7.2).
    pub strength: EvidenceStrength,
    /// The source the value came from, so provenance survives per field.
    pub source: EvidenceSource,
    /// The winner's coverage, so §8.5's per-field computation survives the merge.
    pub coverage: TemporalCompleteness,
    /// The evidence behind the winning answer (§7.3).
    pub evidence: Vec<EvidenceId>,
    /// Every source that answered for this field, in the order the merge saw them.
    pub contributors: Vec<EvidenceSource>,
    /// When the winning source observed it, where it said.
    pub observed_at: Option<Timestamp>,
    /// Whether two sources answered with different values.
    ///
    /// A disagreement is a fact about the evidence rather than an error: §7.2 lets the stronger
    /// source win and §3.4 keeps the weaker record addressable, so a reader can go and look.
    pub conflicting: bool,
}

/// Merges a replayed answer with a provider-owned historical answer, field by field (§9.1).
///
/// The stronger [`EvidenceStrength`] wins. On an exact tie the answer observed closer to the
/// question wins, and on a further tie the source name decides — so the result depends on the
/// evidence and never on which slice a caller passed first.
#[must_use]
pub fn merge_fields(replayed: &[FieldAnswer], provider: &[FieldAnswer]) -> Vec<MergedField> {
    let mut merged: Vec<MergedField> = Vec::new();
    for answer in replayed.iter().chain(provider.iter()) {
        match merged.iter_mut().find(|field| field.field == answer.field) {
            Some(held) => absorb(held, answer),
            None => merged.push(MergedField {
                field: Arc::clone(&answer.field),
                value: answer.value.clone(),
                strength: answer.strength,
                source: answer.source.clone(),
                coverage: answer.coverage,
                evidence: answer.evidence.clone(),
                contributors: vec![answer.source.clone()],
                observed_at: answer.observed_at,
                conflicting: false,
            }),
        }
    }
    merged.sort_by(|a, b| a.field.cmp(&b.field));
    merged
}

/// Folds one further answer into a field that already has one.
fn absorb(held: &mut MergedField, answer: &FieldAnswer) {
    if !held.contributors.contains(&answer.source) {
        held.contributors.push(answer.source.clone());
    }
    if held.value != answer.value {
        held.conflicting = true;
    }
    if !wins(held, answer) {
        return;
    }
    held.value = answer.value.clone();
    held.strength = answer.strength;
    held.source = answer.source.clone();
    held.coverage = answer.coverage;
    held.evidence = answer.evidence.clone();
    held.observed_at = answer.observed_at;
}

/// Whether `answer` beats what is already held.
///
/// Strength first, by §7.2's own ordering. Then the observation closer to the question, because
/// two equally strong readings differ only in how recently they were taken. Then the source
/// name, so the answer is the same on every run and no side wins by being passed first.
fn wins(held: &MergedField, answer: &FieldAnswer) -> bool {
    match answer.strength.cmp(&held.strength) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match (answer.observed_at, held.observed_at) {
            (Some(candidate), Some(incumbent)) if candidate != incumbent => candidate > incumbent,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            _ => answer.source.as_str() < held.source.as_str(),
        },
    }
}
