//! §37.5's `get recovery` table.
//!
//! §37.5 ends with a requirement that decides the shape of the COST column: *"Cost numbers MUST
//! be labeled estimated where filesystem accounting is not exact."* A copy-on-write snapshot's
//! footprint is a function of what the live filesystem does next, and §38.2 forbids showing it as
//! free. So the column carries the word `estimated` whenever
//! [`ono_change_core::RecoveryCost::is_estimated`] says the figure is one, and never otherwise.
//!
//! v0.2 §35.3 decides the other half: an unknown size is `unknown`, never `0`. A zero in a size
//! column is a measurement, and printing one for a figure nobody took is fabrication.
//!
//! The table is laid out by `ono_render`, which is the only place in the tree that decides column
//! widths (v0.4 §10.7: a table is a rendering strategy, never a value).

use jiff::Timestamp;
use ono_change_core::{RecoveryAsset, RecoveryAssetId, RecoveryCost};
use ono_render::{Cell, Column, Layout, Table};

use crate::{compact_duration, safe};

/// The word §37.5 requires beside a figure filesystem accounting cannot make exact.
pub const ESTIMATED: &str = "estimated";

/// The word v0.2 §35.3 requires instead of a fabricated zero.
pub const UNKNOWN: &str = "unknown";

/// §37.5's table of recovery assets, as of `now`.
///
/// `now` is a parameter because AGE and EXPIRES are both differences from it. §50 makes rendering
/// deterministic, and a view that read the clock would produce different bytes for the same
/// assets on every call, which no test and no audit trail can work with.
#[must_use]
pub fn recovery_assets(assets: &[RecoveryAsset], now: Timestamp, width: usize) -> Vec<String> {
    Layout::new(width.max(crate::MIN_WIDTH)).render(&recovery_table(assets, now))
}

/// The same table as a value, for a caller that renders it into its own layout.
#[must_use]
pub fn recovery_table(assets: &[RecoveryAsset], now: Timestamp) -> Table {
    let mut table = Table::new(vec![
        Column::new("ID"),
        Column::new("TYPE"),
        Column::new("PLAN"),
        Column::new("AGE"),
        Column::new("COST"),
        Column::new("EXPIRES"),
        Column::new("STATUS"),
    ]);
    for asset in assets {
        table.push_row(vec![
            Cell::new(format!("{}{}", RecoveryAssetId::PREFIX, asset.id().short())),
            Cell::new(asset.asset_type().as_str()),
            Cell::new(
                asset
                    .source_plan()
                    .map_or_else(|| "none".to_owned(), |plan| plan.short().to_owned()),
            ),
            Cell::new(age(asset, now)),
            Cell::new(cost(asset.cost())),
            Cell::new(expires(asset, now)),
            Cell::new(safe(asset.state().as_str())),
        ]);
    }
    table
}

/// How long the asset has existed, or `unknown` for one whose creation is in the caller's future.
///
/// A negative age is not zero. It means the caller's `now` and the asset's creation instant
/// disagree, and v0.2 §35.3 would rather say so than round the disagreement away.
fn age(asset: &RecoveryAsset, now: Timestamp) -> String {
    match now.as_second().checked_sub(asset.created_at().as_second()) {
        Some(seconds) if seconds >= 0 => compact_duration(seconds.unsigned_abs()),
        _ => UNKNOWN.to_owned(),
    }
}

/// How long the asset has left, `expired` once it has none, and `unknown` where §37.1 has not
/// fixed one — an asset under a hold has no expiry rather than an infinite one.
fn expires(asset: &RecoveryAsset, now: Timestamp) -> String {
    if asset.retention().is_held() {
        return "held".to_owned();
    }
    let Some(at) = asset.expires_at() else {
        return UNKNOWN.to_owned();
    };
    match at.as_second().checked_sub(now.as_second()) {
        Some(seconds) if seconds > 0 => compact_duration(seconds.unsigned_abs()),
        Some(_) => "expired".to_owned(),
        None => UNKNOWN.to_owned(),
    }
}

/// The COST cell: a size with §37.5's label, or v0.2 §35.3's `unknown`.
///
/// The retained figure is preferred over the initial one because it is the question the column
/// answers — how much storage this asset is costing now, not how much it cost when it was taken.
fn cost(cost: &RecoveryCost) -> String {
    let Some(size) = cost.retained_bytes().or_else(|| cost.initial_bytes()) else {
        return UNKNOWN.to_owned();
    };
    if cost.is_estimated() {
        format!("{size} {ESTIMATED}")
    } else {
        size.to_string()
    }
}
