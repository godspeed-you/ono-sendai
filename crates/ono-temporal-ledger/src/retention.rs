//! Retention (v0.5 §10.4, §31.8, §53).
//!
//! §10.4 and §53 together fix the policy: 24 hours or 512 MiB, "whichever bound removes data
//! first". §31.8 fixes what removal means — "deleting expired events MUST also handle orphaned
//! evidence/checkpoints/causal links without leaving invalid references" — and that removal "MUST
//! run in bounded background work".
//!
//! Bounded is why [`RetentionPolicy::batch`] exists. A sweep removes at most one batch of events
//! per call and reports whether more remains, so a caller drives it from a timer instead of
//! holding the prompt while a 512 MiB ledger is compacted.

use ono_value::{ByteSize, Duration};

/// How many events one sweep removes before yielding (§31.8: bounded background work).
pub const DEFAULT_RETENTION_BATCH: usize = 2_048;

/// `temporal.retention.max_age`'s default: 24 hours (§10.4).
pub const DEFAULT_MAX_AGE: Duration = Duration::from_nanoseconds(24 * 3_600 * 1_000_000_000);
/// `temporal.retention.max_size`'s default: 512 MiB (§10.4).
pub const DEFAULT_MAX_SIZE: ByteSize = ByteSize::from_bytes(512 * 1024 * 1024);

/// What the store is allowed to keep (§10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    /// The oldest an event may be. `None` keeps history until the size bound bites.
    pub max_age: Option<Duration>,
    /// The largest the store may become. `None` keeps history until the age bound bites.
    pub max_size: Option<ByteSize>,
    /// How many events one call of the sweep removes before yielding (§31.8).
    pub batch: usize,
}

impl Default for RetentionPolicy {
    /// §10.4's defaults: 24 hours or 512 MiB, whichever removes data first.
    fn default() -> Self {
        Self {
            max_age: Some(DEFAULT_MAX_AGE),
            max_size: Some(DEFAULT_MAX_SIZE),
            batch: DEFAULT_RETENTION_BATCH,
        }
    }
}

impl RetentionPolicy {
    /// A policy that removes nothing, for a caller that manages its own lifetime.
    #[must_use]
    pub const fn unlimited() -> Self {
        Self {
            max_age: None,
            max_size: None,
            batch: DEFAULT_RETENTION_BATCH,
        }
    }

    /// The same policy with a different age bound.
    #[must_use]
    pub const fn with_max_age(mut self, max_age: Option<Duration>) -> Self {
        self.max_age = max_age;
        self
    }

    /// The same policy with a different size bound.
    #[must_use]
    pub const fn with_max_size(mut self, max_size: Option<ByteSize>) -> Self {
        self.max_size = max_size;
        self
    }

    /// The same policy sweeping at most `batch` events per call.
    ///
    /// A batch of zero is raised to one: a sweep that can remove nothing would never finish.
    #[must_use]
    pub const fn with_batch(mut self, batch: usize) -> Self {
        self.batch = if batch == 0 { 1 } else { batch };
        self
    }

    /// Whether either bound is in force.
    #[must_use]
    pub const fn is_bounded(&self) -> bool {
        self.max_age.is_some() || self.max_size.is_some()
    }
}

/// What one bounded sweep removed (§31.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Swept {
    /// How many events went.
    pub events: u64,
    /// How much evidence went with them, having nothing left that cites it.
    pub evidence: u64,
    /// How many causal links went, an end of each having gone.
    pub links: u64,
    /// How many checkpoints went, being older than the retained boundary.
    pub checkpoints: u64,
    /// How many action records went.
    pub actions: u64,
    /// How many coverage intervals went.
    pub coverage: u64,
    /// Whether the bounds now hold. `false` means the caller should sweep again (§31.8).
    pub complete: bool,
}

impl Swept {
    /// Whether the sweep removed anything at all.
    #[must_use]
    pub const fn removed_anything(&self) -> bool {
        self.events > 0
            || self.evidence > 0
            || self.links > 0
            || self.checkpoints > 0
            || self.actions > 0
            || self.coverage > 0
    }
}
