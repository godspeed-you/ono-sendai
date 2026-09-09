//! §37.5's `get recovery` table.
//!
//! §37.5 ends with a requirement that decides the shape of the COST column: *"Cost numbers MUST
//! be labeled estimated where filesystem accounting is not exact."* A copy-on-write snapshot's
//! footprint is a function of what the live filesystem does next, and §38.2 forbids showing it as
//! free. So the column carries the word [`ESTIMATED`] whenever `ono.recovery-asset/1`'s
//! `size_estimated` says the figure is one, and never otherwise.
//!
//! v0.2 §35.3 decides the other half: an unmeasured size is [`UNKNOWN`], never `0`. A zero in a
//! size column is a measurement, and printing one for a figure nobody took is fabrication.
//!
//! The table is laid out by `ono_render`, which is the only place in the tree that decides column
//! widths (v0.4 §10.7: a table is a rendering strategy, never a value).

use jiff::Timestamp;
use ono_render::{Cell, Column, Layout, Table};
use ono_value::RecordValue;

use crate::{byte_size, compact_duration, flag, text, timestamp};

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
pub fn recovery_assets(assets: &[RecordValue], now: Timestamp, width: usize) -> Vec<String> {
    Layout::new(width.max(crate::MIN_WIDTH)).render(&recovery_table(assets, now))
}

/// The same table as a value, for a caller that renders it into its own layout.
#[must_use]
pub fn recovery_table(assets: &[RecordValue], now: Timestamp) -> Table {
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
            Cell::new(format!("recovery/{}", crate::plan::short(asset, "id"))),
            Cell::new(text(asset, "type").unwrap_or_else(|| UNKNOWN.to_owned())),
            Cell::new(match text(asset, "source_plan") {
                Some(_) => crate::plan::short(asset, "source_plan"),
                None => "none".to_owned(),
            }),
            Cell::new(age(asset, now)),
            Cell::new(cost(asset)),
            Cell::new(expires(asset, now)),
            Cell::new(text(asset, "state").unwrap_or_else(|| UNKNOWN.to_owned())),
        ]);
    }
    table
}

/// How long the asset has existed, or `unknown` for one whose creation is in the caller's future.
///
/// A negative age is not zero. It means the caller's `now` and the asset's `created_at` disagree,
/// and v0.2 §35.3 would rather say so than round the disagreement away.
fn age(asset: &RecordValue, now: Timestamp) -> String {
    let Some(created) = timestamp(asset, "created_at") else {
        return UNKNOWN.to_owned();
    };
    match now.as_second().checked_sub(created.as_second()) {
        Some(seconds) if seconds >= 0 => compact_duration(seconds.unsigned_abs()),
        _ => UNKNOWN.to_owned(),
    }
}

/// How long the asset has left, `expired` once it has none, and `unknown` where §37.1 has not
/// fixed one — an asset under a hold has no expiry rather than an infinite one.
fn expires(asset: &RecordValue, now: Timestamp) -> String {
    if flag(asset, "held") {
        return "held".to_owned();
    }
    let Some(at) = timestamp(asset, "expires_at") else {
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
fn cost(asset: &RecordValue) -> String {
    let Some(size) = byte_size(asset, "retained_size").or_else(|| byte_size(asset, "initial_size"))
    else {
        return UNKNOWN.to_owned();
    };
    if flag(asset, "size_estimated") {
        format!("{size} {ESTIMATED}")
    } else {
        size.to_string()
    }
}
