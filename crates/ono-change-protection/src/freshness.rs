//! Whether an asset created earlier still describes the state about to change (spec §18.3).
//!
//! §18.1 wants recovery assets created immediately before mutation, and §18.2 lets an operator
//! run `protect @plan` hours ahead of the maintenance window. §18.3 is what reconciles them:
//!
//! > If current state drifted materially after protection, Ono MUST NOT pretend the old asset is
//! > a just-before-change recovery point.
//!
//! Three answers follow, and [`assess`] returns exactly one of them: the asset is fresh, the
//! asset may be used with explicit acceptance that it is stale, or the asset must be replaced.
//! An asset with no recorded `captured_state_id` is never the first of those — an asset of
//! unknown vintage is not evidence about the present, and §2.4 forbids reading it as one.

use std::sync::Arc;

use ono_change_core::RecoveryAsset;
use ono_change_core::error;
use ono_value::ErrorValue;

/// What §18.3 permits doing with an asset that already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    /// The asset captured the state that is about to change: a just-before-change recovery point.
    Fresh,
    /// The asset predates the current state and may be used only with explicit acceptance.
    StaleAcceptable,
    /// The asset cannot serve as protection for this apply and a new one is needed.
    MustReplace,
}

impl Freshness {
    /// The word a rendering shows.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Freshness::Fresh => "fresh",
            Freshness::StaleAcceptable => "stale-acceptable",
            Freshness::MustReplace => "must-replace",
        }
    }

    /// Whether apply may proceed on this asset without the operator saying anything more.
    #[must_use]
    pub const fn is_usable_without_acceptance(self) -> bool {
        matches!(self, Freshness::Fresh)
    }
}

/// The answer, with the sentence that justifies it (§18.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreshnessVerdict {
    freshness: Freshness,
    detail: Arc<str>,
}

impl FreshnessVerdict {
    /// Which of §18.3's three answers this is.
    #[must_use]
    pub const fn freshness(&self) -> Freshness {
        self.freshness
    }

    /// Why.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Whether the asset is a just-before-change recovery point (§18.1).
    #[must_use]
    pub const fn is_fresh(&self) -> bool {
        matches!(self.freshness, Freshness::Fresh)
    }

    /// The refusal to raise when policy requires fresh protection (§18.3).
    ///
    /// Returns `None` for a fresh asset. Everything else is `recovery.asset_stale`, whose help
    /// text is §18.3's own sentence: Ono does not pretend an asset of an earlier state is a
    /// just-before-change recovery point.
    #[must_use]
    pub fn refusal(&self, asset: &RecoveryAsset) -> Option<ErrorValue> {
        match self.freshness {
            Freshness::Fresh => None,
            _ => Some(error::asset_stale(asset.id(), &self.detail)),
        }
    }
}

/// Decides what §18.3 permits for `asset` against the current state fingerprint.
///
/// `current` is the fingerprint of the state the plan is about to change, as the plan's
/// preconditions recorded it (§7.2). `require_fresh` is the policy that turns "you may accept
/// this" into "this must be replaced" — §18.3's third option, aborting when policy requires fresh
/// protection.
#[must_use]
pub fn assess(
    asset: &RecoveryAsset,
    current: Option<&str>,
    require_fresh: bool,
) -> FreshnessVerdict {
    let stale = |detail: String| FreshnessVerdict {
        freshness: if require_fresh {
            Freshness::MustReplace
        } else {
            Freshness::StaleAcceptable
        },
        detail: Arc::from(detail),
    };

    if !asset.is_usable() {
        return FreshnessVerdict {
            freshness: Freshness::MustReplace,
            detail: Arc::from(format!(
                "the asset is {} and only a validated, ready asset is a recovery point (§11.4)",
                asset.state()
            )),
        };
    }
    let Some(captured) = asset.captured_state() else {
        return stale(
            "the asset records no captured state fingerprint, so nothing establishes that it \
             reflects the state about to change (§18.3)"
                .to_owned(),
        );
    };
    let Some(current) = current else {
        return stale(format!(
            "the current state of the target could not be fingerprinted, so the asset's captured \
             state {captured} cannot be shown to match it (§18.3, §56.3)"
        ));
    };
    if asset.is_fresh_for(current) {
        return FreshnessVerdict {
            freshness: Freshness::Fresh,
            detail: Arc::from(format!(
                "the asset captured {captured}, which is the state about to change (§18.1)"
            )),
        };
    }
    stale(format!(
        "the asset captured {captured} and the target is now {current}: the state drifted after \
         protection was created (§18.3)"
    ))
}
