//! The cleanup preview of §37.3.
//!
//! §37.3: *"Before deleting an asset whose removal changes recovery capability, Ono SHOULD show
//! which plans become unrecoverable."* §2.15 is why it matters — cleanup never outruns recovery
//! policy — and the preview is the one moment the operator can see that a removal takes a way
//! back with it. The answer is computed by `ono-change-protection`'s cleanup preview; this draws
//! it, and closes by saying nothing was removed, because `--dry-run` is only worth running if the
//! operator does not have to wonder.

use ono_value::RecordValue;

use crate::symbols::Symbol;
use crate::{counted, fit, heading, text};

/// The line that closes a cleanup preview: the dry run removed nothing.
pub const NOTHING_REMOVED: &str = "NOTHING REMOVED";

/// `remove recovery --dry-run`'s answer for one asset (§37.3).
///
/// `asset` is the `ono.recovery-asset/1` that would be removed, and `unrecoverable` the short
/// references of the plans its removal would leave with no way back — the plan ids of
/// `CleanupEntry::unrecoverable_plans`, as §36.4 prints them.
#[must_use]
pub fn cleanup_preview(asset: &RecordValue, unrecoverable: &[String], width: usize) -> Vec<String> {
    let id = text(asset, "id").unwrap_or_else(|| "unknown".to_owned());
    let reference = format!("recovery/{}", crate::recovery::short_asset(&id));
    let mut lines = vec![fit(&format!("CLEANUP PREVIEW / {reference}"), width)];
    if let Some(native) = text(asset, "reference") {
        lines.push(fit(&format!("  {native}"), width));
    }

    heading(&mut lines, "recovery capability");
    if unrecoverable.is_empty() {
        lines.push(fit(
            &format!("  {reference} would be removed, and no retained plan depends on it"),
            width,
        ));
    } else {
        lines.push(fit(
            &format!(
                "  removing {reference} would make {} unrecoverable",
                counted(unrecoverable.len(), "plan", "plans")
            ),
            width,
        ));
        for plan in unrecoverable {
            lines.push(fit(
                &format!("  {} plan/{plan}", Symbol::Risk.ascii()),
                width,
            ));
        }
    }
    lines.push(String::new());
    lines.push(NOTHING_REMOVED.to_owned());
    lines
}
