//! The prospective-change projections of Ono-Sendai v0.6 (spec §20, §24, §25, §37.5, Appendix E).
//!
//! §50.1 gives this crate one job and forbids it a second: it turns the records of §46 into
//! lines, and it *never computes a fact*. There is no protection level recomputed here, no blast
//! radius counted here, no verdict decided here — every one of those is read off the record that
//! carries it, so a view cannot disagree with the plan it draws.
//!
//! The layering makes that structural rather than editorial. This crate cannot see the change
//! vocabulary at all: it depends on `ono-value` and `ono-render` and nothing above them, exactly
//! as `ono-spatial-render` and `ono-temporal-render` do. A crate that cannot name a `ChangePlan`
//! cannot mutate one, and a crate that reads `protection_level` off a record cannot recompute it.
//! Every entry point therefore takes a [`ono_value::RecordValue`] of a schema §46 names, a width
//! to lay out at, and — where it renders an age — the instant to measure from.
//!
//! Three of the specification's rules shape almost every function below:
//!
//! - **Appendix E.8: no green shield.** A coverage summary is never rendered without its
//!   exclusions. [`protection::protection_block`] is a single function that emits the level and
//!   the exclusions together, and no public function in this crate returns the level alone. §62.6
//!   is the reason: protection that reads as a badge becomes permission to be reckless.
//! - **§2.1 and §62.3: planning is side-effect free.** A plan view that has not been applied ends
//!   with `PLAN NOT EXECUTED`, and a recovery view with `RECOVERY NOT EXECUTED`, because the
//!   operator's whole model of what just happened rests on that line.
//! - **§25.3: a recovery names the scope it verified.** Nothing here can emit a global sentence
//!   claiming a rollback worked; [`verify::recovery_verification`] reports one equivalence domain
//!   at a time and closes with "FULL WORLD EQUIVALENCE NOT CLAIMED".
//!
//! # Determinism
//!
//! §50 and `ono-cli`'s sink make layout a parameter rather than an observation. Every entry point
//! takes the `width` to lay out at and nothing reads the terminal; every entry point that renders
//! an age or a remaining retention takes `now`. The same values therefore produce the same bytes
//! on a pipe, in a file and in a test.

#![forbid(unsafe_code)]

use unicode_width::UnicodeWidthStr;

pub(crate) use field::{
    Fields, Item, byte_size, count, duration_seconds, flag, items, list_len, nested, strings, text,
    timestamp,
};

mod field;

pub mod assets;
pub mod cleanup;
pub mod collapsed;
pub mod impact;
pub mod keys;
pub mod overlay;
pub mod plan;
pub mod progress;
pub mod protection;
pub mod recovery;
pub mod symbols;
pub mod verify;

pub use assets::{recovery_assets, recovery_table};
pub use cleanup::{NOTHING_REMOVED, cleanup_preview};
pub use collapsed::{ActionGroup, action_groups, collapsed_plan};
pub use impact::{blast_radius, boundaries, impact_block};
pub use keys::{BINDINGS, Binding, InspectorAction, binding_for, key_help};
pub use overlay::plan_overlay;
pub use plan::{OUTSTANDING_ACKNOWLEDGEMENTS, PLAN_NOT_EXECUTED, QUESTIONS, Section, plan_view};
pub use progress::{Phase, apply_failure, apply_progress, failure_display, next_steps};
pub use protection::{coverage_matrix, protection_block, recovery_asset_block};
pub use recovery::{NEWER_STATE_AT_RISK, RECOVERY_NOT_EXECUTED, recovery_view};
pub use symbols::{Charset, Symbol, legend};
pub use verify::{NO_FULL_EQUIVALENCE, recovery_verification, verification_view};

/// How many characters of an identity a printed reference carries (§36.4).
///
/// A store resolves a reference on an unambiguous prefix, so a heading prints the prefix and not
/// the whole digest: a reference nobody can type is a reference nobody uses.
pub(crate) const SHORT: usize = 4;

/// The narrowest layout any view is laid out at.
///
/// v0.4 §39.3 keeps forty columns usable and `ono-cli`'s sink never asks for fewer than twenty.
/// Below that a heading and its indent are the whole line, so the width is clamped rather than
/// honoured: a view that renders nothing is worse than a view that overflows a toy terminal.
pub(crate) const MIN_WIDTH: usize = 20;

/// How many terminal cells `text` occupies.
///
/// The plan view aligns a label column against paths and service names, which are data rather
/// than ASCII the renderer chose, so counting `char`s would misalign the moment a CJK filename
/// arrives. `ono-render` measures the same way for the same reason.
pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// The marker a cut line ends with, as `ono-render`'s tables use it (spec v0.2 §13.3).
const TRUNCATION_MARKER: &str = "...";

/// A line cut to `width` cells, with trailing space removed and the cut made visible.
///
/// Truncation is by display width rather than by byte or `char`, so a wide glyph is never split
/// across the boundary and the drawn line never exceeds what the caller promised.
///
/// v0.2 §13.3 requires the truncation to be visible, and a path is the reason: a plan whose
/// target is `/srv/app/config` and one whose target is `/srv/app/config.bak` cut to the same
/// column are the same line, and an operator reading a change before applying it has to be able
/// to tell them apart. The marker is `ono-render`'s own, so the two agree.
pub(crate) fn fit(line: &str, width: usize) -> String {
    let width = width.max(MIN_WIDTH);
    if display_width(line) <= width {
        return line.trim_end().to_owned();
    }
    let budget = width.saturating_sub(display_width(TRUNCATION_MARKER));
    let mut kept = String::with_capacity(line.len());
    let mut used = 0usize;
    for character in line.chars() {
        let cell = display_width(character.encode_utf8(&mut [0u8; 4]));
        if used + cell > budget {
            break;
        }
        kept.push(character);
        used += cell;
    }
    let mut cut = kept.trim_end().to_owned();
    cut.push_str(TRUNCATION_MARKER);
    cut
}

/// `  label      value` — the indented two-column shape §20.2 uses inside a block.
pub(crate) fn labelled(label: &str, value: &str, column: usize) -> String {
    format!("  {}", column_pair(label, value, column))
        .trim_end()
        .to_owned()
}

/// `label      value` at column zero — the shape Appendix E.2 and E.4 use for a top-level row.
pub(crate) fn column_pair(label: &str, value: &str, column: usize) -> String {
    // A label wider than the column still gets a separator: `metadata coverage` is eighteen
    // columns and the value column is sixteen, and without this the two ran together into
    // `metadata coverageAppendix C.7: …`. Appendix E's two-column shape is a reading aid, and a
    // row with no gap in it is not one.
    let padding = " ".repeat(column.saturating_sub(display_width(label)).max(1));
    format!("{label}{padding}{value}").trim_end().to_owned()
}

/// `1 direct target` / `2 direct targets` — a count and the noun it agrees with (§9.5).
pub(crate) fn counted(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("{count} {singular}")
    } else {
        format!("{count} {plural}")
    }
}

/// A duration in the two largest units that carry information — `14m`, `23h46m`, `24h`, `2d6h`.
///
/// §37.5's table prints an age and a remaining retention side by side, and a reader comparing two
/// rows needs the same shape in both. Two units is where precision stops being useful: nobody
/// schedules around the seconds of a twenty-three hour retention, and printing them would make
/// the column jitter for no gain.
///
/// Hours run past a day rather than rolling over at one, because §37.1's window is twenty-four
/// hours and §13.8 prints it as `24h`. A reader comparing a retention against the policy that set
/// it should not have to convert.
pub(crate) fn compact_duration(seconds: u64) -> String {
    const TWO_DAYS: u64 = 2 * 86_400;
    let (minutes, rest) = ((seconds % 3_600) / 60, seconds % 60);
    if seconds >= TWO_DAYS {
        let (days, hours) = (seconds / 86_400, (seconds % 86_400) / 3_600);
        return if hours == 0 {
            format!("{days}d")
        } else {
            format!("{days}d{hours}h")
        };
    }
    let hours = seconds / 3_600;
    match (hours, minutes) {
        (0, 0) => format!("{rest}s"),
        (0, _) if rest == 0 => format!("{minutes}m"),
        (0, _) => format!("{minutes}m{rest}s"),
        (_, 0) => format!("{hours}h"),
        (_, _) => format!("{hours}h{minutes}m"),
    }
}

/// `a, b, c` cut to what fits, with `+n more` where it does not.
///
/// §9.5 permits a summary and §23.6's spirit forbids one that hides its own bound, so the count
/// of what was left out travels with the line that left it out.
pub(crate) fn join_fitted(items: &[String], width: usize) -> String {
    let width = width.max(MIN_WIDTH);
    let mut line = String::new();
    let mut shown = 0usize;
    for item in items {
        let candidate = if line.is_empty() {
            item.clone()
        } else {
            format!("{line}, {item}")
        };
        let remaining = items.len() - shown - 1;
        let tail = if remaining > 0 {
            format!(", +{remaining} more")
        } else {
            String::new()
        };
        if display_width(&candidate) + display_width(&tail) > width && shown > 0 {
            break;
        }
        line = candidate;
        shown += 1;
    }
    let remaining = items.len() - shown;
    if remaining > 0 {
        line.push_str(&format!(", +{remaining} more"));
    }
    line
}

/// The heading of a block, and the blank line that separates it from the block above.
pub(crate) fn heading(lines: &mut Vec<String>, text: &str) {
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.push(text.to_owned());
}
