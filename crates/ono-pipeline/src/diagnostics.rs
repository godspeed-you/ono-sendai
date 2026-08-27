//! What a pipeline counted while it ran, so a surprising row count has somewhere to be explained.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Counters shared by every stage of one pipeline (ADR-0014).
///
/// ADR-0014 requires that values excluded because a predicate was *unknown*, and values skipped
/// by an aggregate because they were null, be counted rather than silently dropped: "a user who
/// is surprised by a row count has somewhere to look that is not the source code". This is that
/// somewhere; `explain` and the REPL read it.
///
/// Cloning shares the counters.
#[derive(Debug, Clone, Default)]
pub struct Diagnostics {
    inner: Arc<Counters>,
}

#[derive(Debug, Default)]
struct Counters {
    excluded_unknown: AtomicU64,
    skipped_null: AtomicU64,
}

impl Diagnostics {
    /// A fresh set of counters, all zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many values `where` excluded because their predicate evaluated to null.
    ///
    /// Not the same as the values excluded because the predicate was `false`, and not the same
    /// as the values whose predicate failed: ADR-0014 keeps all three apart.
    #[must_use]
    pub fn excluded_unknown(&self) -> u64 {
        self.inner.excluded_unknown.load(Ordering::Relaxed)
    }

    /// How many null values an aggregate skipped, so an average is never quietly computed over a
    /// different population than the user thinks.
    #[must_use]
    pub fn skipped_null(&self) -> u64 {
        self.inner.skipped_null.load(Ordering::Relaxed)
    }

    /// Records one value excluded because its predicate was unknown.
    pub fn record_excluded_unknown(&self) {
        self.inner.excluded_unknown.fetch_add(1, Ordering::Relaxed);
    }

    /// Records one null value skipped by an aggregate.
    pub fn record_skipped_null(&self) {
        self.inner.skipped_null.fetch_add(1, Ordering::Relaxed);
    }
}
