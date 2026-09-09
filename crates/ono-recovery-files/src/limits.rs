//! The bounds a file archive stays inside (spec v0.6 §15.2).
//!
//! §15.2 says the provider *"SHOULD NOT silently archive arbitrarily large data trees, databases,
//! sockets, devices or pseudo-filesystems"*, and the word doing the work is *silently*. A limit
//! that truncates a tree produces an asset that looks like protection and is not, which is §62.1's
//! snapshot theatre. So every bound here refuses, the refusal names the bound and the measurement
//! that crossed it, and nothing is ever written for a scope that did not fit.

use ono_change_core::error::asset_create_failed;
use ono_value::{ErrorValue, Value};

use crate::PROVIDER_ID;

/// The default ceiling on a whole archive: 64 MiB.
///
/// A configuration tree is kilobytes. A limit an order of magnitude above `/etc` leaves room for
/// the certificates and templates that live beside it, and stops well short of the data trees and
/// databases §15.2 excludes.
pub const DEFAULT_MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// The default ceiling on how many objects one archive holds.
pub const DEFAULT_MAX_OBJECT_COUNT: usize = 4096;

/// The default ceiling on a single file: 16 MiB.
pub const DEFAULT_MAX_OBJECT_BYTES: u64 = 16 * 1024 * 1024;

/// What a file archive may grow to before the provider refuses it (§15.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileProtectionLimits {
    max_total_bytes: u64,
    max_object_count: usize,
    max_object_bytes: u64,
}

impl Default for FileProtectionLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: DEFAULT_MAX_TOTAL_BYTES,
            max_object_count: DEFAULT_MAX_OBJECT_COUNT,
            max_object_bytes: DEFAULT_MAX_OBJECT_BYTES,
        }
    }
}

impl FileProtectionLimits {
    /// Limits of `total` bytes over at most `objects` objects, none larger than `object_bytes`.
    #[must_use]
    pub const fn new(total: u64, objects: usize, object_bytes: u64) -> Self {
        Self {
            max_total_bytes: total,
            max_object_count: objects,
            max_object_bytes: object_bytes,
        }
    }

    /// Sets the ceiling on the whole archive.
    #[must_use]
    pub const fn with_total_bytes(mut self, bytes: u64) -> Self {
        self.max_total_bytes = bytes;
        self
    }

    /// Sets the ceiling on how many objects one archive holds.
    #[must_use]
    pub const fn with_object_count(mut self, objects: usize) -> Self {
        self.max_object_count = objects;
        self
    }

    /// Sets the ceiling on a single file.
    #[must_use]
    pub const fn with_object_bytes(mut self, bytes: u64) -> Self {
        self.max_object_bytes = bytes;
        self
    }

    /// The ceiling on the whole archive.
    #[must_use]
    pub const fn max_total_bytes(&self) -> u64 {
        self.max_total_bytes
    }

    /// The ceiling on how many objects one archive holds.
    #[must_use]
    pub const fn max_object_count(&self) -> usize {
        self.max_object_count
    }

    /// The ceiling on a single file.
    #[must_use]
    pub const fn max_object_bytes(&self) -> u64 {
        self.max_object_bytes
    }

    /// Whether one more object of `bytes` keeps `total_bytes` and `count` inside the limits.
    ///
    /// The bound that is crossed first is the bound that is reported, so a tree that is both too
    /// large and too numerous names the reason the walk actually stopped rather than a summary.
    pub(crate) const fn admits(
        &self,
        count: usize,
        total_bytes: u64,
        bytes: u64,
    ) -> Result<(), LimitKind> {
        if bytes > self.max_object_bytes {
            return Err(LimitKind::ObjectBytes);
        }
        if count > self.max_object_count {
            return Err(LimitKind::ObjectCount);
        }
        if total_bytes > self.max_total_bytes {
            return Err(LimitKind::TotalBytes);
        }
        Ok(())
    }

    /// The value of the bound `kind` names, for the refusal that reports it.
    pub(crate) const fn bound(&self, kind: LimitKind) -> u64 {
        match kind {
            LimitKind::TotalBytes => self.max_total_bytes,
            LimitKind::ObjectCount => self.max_object_count as u64,
            LimitKind::ObjectBytes => self.max_object_bytes,
        }
    }
}

/// Which configured bound a scope crossed (§15.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitKind {
    /// The archive as a whole would be larger than the configured total.
    TotalBytes,
    /// The archive would hold more objects than the configured count.
    ObjectCount,
    /// One file is larger than the configured single-object ceiling.
    ObjectBytes,
}

impl LimitKind {
    /// The bound's name, as the refusal spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            LimitKind::TotalBytes => "max_total_bytes",
            LimitKind::ObjectCount => "max_object_count",
            LimitKind::ObjectBytes => "max_object_bytes",
        }
    }

    /// The sentence a person reads when this bound refuses a scope.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            LimitKind::TotalBytes => "the tree is larger than the configured archive limit",
            LimitKind::ObjectCount => "the tree holds more objects than the configured limit",
            LimitKind::ObjectBytes => "one file is larger than the configured per-file limit",
        }
    }
}

/// The refusal §15.2 requires: the bound, the measurement, and no archive (§62.1).
pub(crate) fn limit_exceeded(
    kind: LimitKind,
    scope: &str,
    object: &str,
    limit: u64,
    measured: u64,
) -> ErrorValue {
    asset_create_failed(
        PROVIDER_ID,
        scope,
        &format!(
            "v0.6 §15.2: {}. `{object}` takes it to {measured} against a {} of {limit}. Nothing \
             was archived, because a truncated archive is protection that is not there",
            kind.detail(),
            kind.as_str()
        ),
    )
    .with_metadata("limit_name", Value::string(kind.as_str()))
    .with_metadata("limit", Value::Int(i128::from(limit)))
    .with_metadata("measured", Value::Int(i128::from(measured)))
    .with_metadata("object", Value::string(object))
}
