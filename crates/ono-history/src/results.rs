//! The retained result history of spec v0.4.1 §24 and spec §20.2.
//!
//! The session keeps recent structured results so `@-1` and `@N` can reach them. v0.4.1 §24.1
//! keeps the existing count limits and adds the byte ceilings §2.4 requires beside every count;
//! §24.2 fixes what happens when one is reached, and it is the *other* branch of §21.3:
//!
//! > Result history is a cache, not a correctness requirement. It therefore uses eviction rather
//! > than failing the user's command.
//!
//! So nothing in this module returns an error, and nothing here can fail a command. §31.3 puts
//! the policy here rather than at the evaluator's call sites, so there is one place that decides
//! what is kept and one place that knows what was dropped.

use ono_value::{Budget, Value, estimated_size};

/// What the retained history may hold (v0.4.1 §24.1, Appendix A).
///
/// Four bounds because §2.4 requires a byte bound beside every count where the elements can be
/// any size, and §65.6 names the alternative — `N` values of arbitrary size — as a defect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionLimits {
    /// How many results `@-1` … `@-N` can reach.
    pub results: usize,
    /// How many values of one result are retained.
    pub items_per_result: usize,
    /// How many bytes of one result are retained.
    pub bytes_per_result: u64,
    /// How many bytes the whole history may hold before oldest-first eviction.
    pub bytes_total: u64,
}

impl Default for RetentionLimits {
    /// Appendix A's defaults.
    ///
    /// A `Default` here is not the hazard §21.1 names: every field is a finite figure the
    /// specification fixes, and the type cannot express an unlimited one.
    fn default() -> Self {
        Self {
            results: 16,
            items_per_result: 10_000,
            bytes_per_result: 16 * 1024 * 1024,
            bytes_total: 64 * 1024 * 1024,
        }
    }
}

/// What retaining one result did (v0.4.1 §24.2, §24.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retained {
    kept: usize,
    total: usize,
}

impl Retained {
    /// How many values were retained.
    #[must_use]
    pub const fn kept(self) -> usize {
        self.kept
    }

    /// How many values the result held.
    #[must_use]
    pub const fn total(self) -> usize {
        self.total
    }

    /// How many values were not retained.
    #[must_use]
    pub const fn dropped(self) -> usize {
        self.total.saturating_sub(self.kept)
    }

    /// §24.2 rule 3's marker: whether the stored result is part of the original.
    #[must_use]
    pub const fn truncated_for_history(self) -> bool {
        self.kept < self.total
    }

    /// The sentence §24.3 requires, or `None` when the whole result was kept.
    ///
    /// §54.1 writes the shape: *"result history kept 10,000 of 84,212 values because the 16 MiB
    /// history budget was reached"*. The command's own output is not what was shortened, and the
    /// wording says "history kept" for that reason.
    #[must_use]
    pub fn notice(self) -> Option<String> {
        self.truncated_for_history().then(|| {
            format!(
                "result history kept {} of {} values because its retention budget was reached; \
                 the command's own output was complete",
                self.kept, self.total
            )
        })
    }
}

/// The session's recent structured results, bounded and honest (v0.4.1 §24, §31.3).
///
/// ```
/// use ono_history::{ResultHistory, RetentionLimits};
/// use ono_value::Value;
///
/// let mut history = ResultHistory::new(RetentionLimits::default());
/// let outcome = history.retain(&[Value::Int(1), Value::Int(2)]);
/// assert!(!outcome.truncated_for_history());
/// assert_eq!(history.previous(1), Some(&[Value::Int(1), Value::Int(2)][..]));
/// ```
#[derive(Debug, Clone)]
pub struct ResultHistory {
    limits: RetentionLimits,
    entries: std::collections::VecDeque<Entry>,
}

#[derive(Debug, Clone)]
struct Entry {
    values: Vec<Value>,
    bytes: u64,
    outcome: Retained,
}

impl ResultHistory {
    /// An empty history bounded by `limits`.
    #[must_use]
    pub fn new(limits: RetentionLimits) -> Self {
        Self {
            limits,
            entries: std::collections::VecDeque::new(),
        }
    }

    /// The limits in force.
    #[must_use]
    pub const fn limits(&self) -> RetentionLimits {
        self.limits
    }

    /// Replaces the limits, evicting whatever no longer fits.
    ///
    /// Configuration is read after the session exists, so the shell starts at Appendix A's
    /// defaults and narrows to the user's; narrowing must take effect rather than apply only to
    /// results retained afterwards.
    pub fn set_limits(&mut self, limits: RetentionLimits) {
        self.limits = limits;
        self.evict_to_fit();
    }

    /// How many results are retained.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is retained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The estimated bytes the whole history retains (§21.2).
    #[must_use]
    pub fn retained_bytes(&self) -> u64 {
        self.entries
            .iter()
            .fold(0u64, |total, entry| total.saturating_add(entry.bytes))
    }

    /// The `n`th previous result, `1` for the most recent (spec §6.4's `@-1`).
    #[must_use]
    pub fn previous(&self, n: u32) -> Option<&[Value]> {
        self.entry(n).map(|entry| entry.values.as_slice())
    }

    /// Whether the `n`th previous result is only part of what the command produced (§24.3).
    #[must_use]
    pub fn was_truncated(&self, n: u32) -> bool {
        self.entry(n)
            .is_some_and(|entry| entry.outcome.truncated_for_history())
    }

    /// What retaining the `n`th previous result did, for a caller that inspects it (§24.3).
    #[must_use]
    pub fn retention_of(&self, n: u32) -> Option<Retained> {
        self.entry(n).map(|entry| entry.outcome)
    }

    /// Retains as much of `values` as the limits allow, evicting older results to make room.
    ///
    /// Never fails and never touches `values`: §24.2 rule 1 is that *"the live pipeline result is
    /// never truncated merely to fit history"*, and the borrow is how that is guaranteed rather
    /// than remembered.
    ///
    /// Retention stops at the first of the per-result item cap and the per-result byte cap
    /// (§24.2 rule 2), the entry is marked when it stopped early (rule 3), and older entries are
    /// evicted until the total fits (rule 4). A single value larger than the per-result byte cap
    /// is not retained at all (rule 5) — and the result still flowed through the pipeline, which
    /// is the whole point of the cache being a cache.
    pub fn retain(&mut self, values: &[Value]) -> Retained {
        self.retain_mapped(values, Clone::clone)
    }

    /// [`retain`](Self::retain), storing `map(value)` in place of each value it keeps.
    ///
    /// For the session's redaction policy (spec §20.2, §17.5, ADR-0262): the values that will not
    /// be kept are never passed through it, so a result of eighty thousand values costs the policy
    /// only what history retains. The mapped value is what is charged, because the mapped value is
    /// what is stored.
    pub fn retain_mapped(&mut self, values: &[Value], map: impl Fn(&Value) -> Value) -> Retained {
        let total = values.len();
        // The shared abstraction of §21.1, spending the estimator of §21.2. What differs from
        // every other caller is the response: this one evicts and never raises (§21.3).
        let mut budget = Budget::of(
            "result history",
            u64::try_from(self.limits.items_per_result).unwrap_or(u64::MAX),
            self.limits.bytes_per_result,
        )
        .for_settings(
            "limits.history_items_per_result",
            "limits.history_bytes_per_result",
        );
        let mut kept = Vec::new();
        for value in values {
            let stored = map(value);
            if budget.charge(&stored).is_err() {
                break;
            }
            kept.push(stored);
        }
        let outcome = Retained {
            kept: kept.len(),
            total,
        };

        if kept.is_empty() {
            // Nothing to store, and nothing to evict for. The result is still reported as
            // truncated when it held values, so `@-1` answering nothing is explained rather than
            // mysterious (§24.3).
            if total == 0 {
                return outcome;
            }
            self.push(Entry {
                values: Vec::new(),
                bytes: 0,
                outcome,
            });
            return outcome;
        }

        self.push(Entry {
            bytes: budget.consumed_bytes(),
            values: kept,
            outcome,
        });
        outcome
    }

    fn entry(&self, n: u32) -> Option<&Entry> {
        let index = self.entries.len().checked_sub(n as usize)?;
        self.entries.get(index)
    }

    fn push(&mut self, entry: Entry) {
        self.entries.push_back(entry);
        self.evict_to_fit();
    }

    /// §24.2 rule 4: evict oldest-first until the slot count and the total byte budget are met.
    fn evict_to_fit(&mut self) {
        while self.entries.len() > self.limits.results {
            self.entries.pop_front();
        }
        // The newest entry is never evicted to satisfy the total: a history that answered nothing
        // after every command would be a cache that is never a cache. Its own bytes were already
        // bounded by the per-result ceiling, so keeping it cannot be unbounded.
        while self.retained_bytes() > self.limits.bytes_total && self.entries.len() > 1 {
            self.entries.pop_front();
        }
    }
}

/// The estimated size of one value, re-exported so a caller measuring a result and a caller
/// spending a budget cannot disagree about what a byte is (§21.2).
#[must_use]
pub fn value_size(value: &Value) -> u64 {
    estimated_size(value)
}
