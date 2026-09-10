//! The recovery plan view (§24.4, §24.5, Appendix E.6).
//!
//! Appendix E.6 fixes the layout and gives the reason in one sentence: recovery *"uses a visually
//! distinct title and always shows newer-state impact above action details"*. §24.2 is why —
//! since the original plan ran, files have been written, packages updated and snapshots taken,
//! and a naive rollback destroys them. An operator who reads the restore target first and the
//! losses second has already decided by the time they reach the part that matters.
//!
//! §62.8 shapes the block that carries it. An analysis that never ran is not an analysis that
//! found nothing, so `newer_state_analysed` is checked before anything is counted, and an
//! unanalysed recovery says so with §20.3's unknown mark. §56.3 makes that a reason to block, and
//! `ono.recovery-plan/1` already answers `requires_acceptance` for it, which is what puts the
//! `requires` block on the view.
//!
//! §27.4 is the vocabulary rule: a compensating action is not a restore, and nothing here calls
//! one the other. The `method` the plan chose is printed as the method it is.

use ono_value::RecordValue;

use crate::symbols::{Charset, Symbol};
use crate::{Item, byte_size, counted, fit, flag, heading, items, list_len, strings, text};

/// The line that closes a recovery view, for the reason `PLAN NOT EXECUTED` closes a plan.
///
/// §5.8 and §24.1 make `recover` a planner. An operator who has just read what recovery would
/// destroy needs to know, without inferring it, that none of it has happened yet.
pub const RECOVERY_NOT_EXECUTED: &str = "RECOVERY NOT EXECUTED";

/// The heading Appendix E.6 puts above everything else in a recovery view.
pub const NEWER_STATE_AT_RISK: &str = "NEWER STATE AT RISK";

/// Appendix E.6's recovery plan view, with §24.4's and §24.5's blocks under it.
///
/// `recovery` is an `ono.recovery-plan/1`. The two block titles Appendix E.6 names are upper case
/// and the rest are §24.4's lower-case headings, which is what makes the newer-state impact and
/// the restore target findable at a glance in a view whose other blocks read as prose.
///
/// §24.5's full-rollback shape is the same function: a method that discards newer state fills the
/// `will discard`, `will destroy` and `requires` blocks, and one that does not leaves them out
/// rather than printing zeroes.
#[must_use]
pub fn recovery_view(recovery: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = vec![fit(
        &format!("RECOVERY PLAN / {}", crate::plan::short(recovery, "id")),
        width,
    )];

    // Appendix E.6: newer-state impact stands above the action details, always.
    heading(&mut lines, NEWER_STATE_AT_RISK);
    lines.extend(newer_state_lines(recovery, width, charset));

    // Appendix E.6's `RESTORE TARGET` is the objects being restored — the thing an operator
    // scans this block for. `target_state` is the state they would be restored *from*, and it
    // belongs with the plan and the asset in `source` rather than under a heading that reads as
    // the destination.
    heading(&mut lines, "RESTORE TARGET");
    let restores = strings(recovery, "restores");
    if restores.is_empty() {
        lines.push(fit("  no object was named for restore", width));
    }
    for object in &restores {
        lines.push(fit(&format!("  {object}"), width));
    }

    heading(&mut lines, "source");
    match text(recovery, "source_plan") {
        Some(_) => lines.push(fit(
            &format!("  plan {}", crate::plan::short(recovery, "source_plan")),
            width,
        )),
        None => lines.push(fit("  no source plan was recorded", width)),
    }
    let assets = strings(recovery, "source_assets");
    if assets.is_empty() {
        lines.push(fit("  no recovery asset was named", width));
    }
    for asset in &assets {
        // §36.4: the reference an operator types back is the short one, and it is the same short
        // one `get recovery` printed.
        let body = asset.rsplit('/').next().unwrap_or(asset);
        let short: String = body.chars().take(crate::SHORT).collect();
        lines.push(fit(&format!("  recovery asset recovery/{short}"), width));
    }
    lines.push(fit(
        &format!(
            "  state {}",
            text(recovery, "target_state").unwrap_or_else(|| "unknown".to_owned())
        ),
        width,
    ));

    heading(&mut lines, "method");
    for field in ["method", "goal", "directory_policy"] {
        if let Some(value) = text(recovery, field) {
            lines.push(fit(&format!("  {value}"), width));
        }
    }

    let newer = items(recovery, "newer_state");
    let preserved: Vec<&Item> = newer
        .iter()
        .filter(|item| text(*item, "class").as_deref() == Some("preserved-by-method"))
        .collect();
    if !preserved.is_empty() {
        heading(&mut lines, "newer state preserved");
        for item in preserved {
            lines.push(fit(&format!("  {}", item_line(item)), width));
        }
    }

    let losses: Vec<&Item> = newer.iter().filter(|item| is_loss(item)).collect();
    if !losses.is_empty() {
        heading(&mut lines, "will discard");
        for item in losses {
            lines.push(fit(
                &format!("  {} {}", Symbol::Risk.glyph(charset), item_line(item)),
                width,
            ));
        }
        if let Some(bytes) = byte_size(recovery, "discarded_size") {
            lines.push(fit(&format!("  {bytes} of changed blocks"), width));
        }
    }

    let destroyed = strings(recovery, "destroyed_assets");
    if !destroyed.is_empty() {
        heading(&mut lines, "will destroy");
        for asset in &destroyed {
            lines.push(fit(
                &format!("  {} {asset}", Symbol::Risk.glyph(charset)),
                width,
            ));
        }
    }

    heading(&mut lines, "not recoverable");
    let unrecoverable = items(recovery, "unrecoverable_effects");
    if unrecoverable.is_empty() {
        lines.push(fit("  nothing was recorded as unrecoverable", width));
    }
    for effect in &unrecoverable {
        lines.extend(unrecoverable_lines(effect, width, charset));
    }

    let gaps = strings(recovery, "metadata_gaps");
    if !gaps.is_empty() {
        // Appendix C.7: a restore that returns the bytes and loses the SELinux label has not
        // returned the file, and the operator finds that out here or in an audit.
        heading(&mut lines, "metadata not restored");
        for gap in &gaps {
            lines.push(fit(
                &format!("  {} {gap}", Symbol::Unknown.glyph(charset)),
                width,
            ));
        }
    }

    heading(&mut lines, "risk");
    lines.push(fit(
        &format!(
            "  {}",
            text(recovery, "risk")
                .unwrap_or_else(|| "unknown".to_owned())
                .to_uppercase()
        ),
        width,
    ));

    let requirements = requirements(recovery);
    if !requirements.is_empty() {
        heading(&mut lines, "requires");
        for requirement in requirements {
            lines.push(fit(
                &format!("  {} {requirement}", Symbol::Risk.glyph(charset)),
                width,
            ));
        }
    }

    if !has_run(recovery) {
        lines.push(String::new());
        lines.push(RECOVERY_NOT_EXECUTED.to_owned());
    }
    lines
}

/// Whether the recovery itself has begun changing the system (§4.1's recovery branch).
///
/// A recovery plan is a plan (§3.8) and carries the same lifecycle, but the states that mean
/// "something happened" are not the same ones: `recovery-planned` is §24.1's *planned and not
/// applied*, which is exactly the state [`RECOVERY_NOT_EXECUTED`] exists to announce. A record
/// with no state has not been applied either, because `recover` produces this view and executes
/// nothing (§5.8).
fn has_run(recovery: &RecordValue) -> bool {
    matches!(
        text(recovery, "state").as_deref(),
        Some("recovering" | "recovered" | "recovery-failed" | "recovery-verified")
    )
}

/// Whether recovery would take this newer state away (§24.3, Appendix C.3, C.4).
fn is_loss(item: &Item) -> bool {
    matches!(
        text(item, "class").as_deref(),
        Some("discarded-by-method" | "conflicting")
    )
}

/// Appendix E.6's counts, or §62.8's admission that nothing counted them.
///
/// The order matters: the unanalysed case is checked first and returns, so there is no path on
/// which an empty item list renders as "nothing would be lost". §62.8 names recovering hours
/// later without considering newer state as unacceptable, and a view that reports the absence of
/// an analysis as an absence of risk is how that happens.
fn newer_state_lines(recovery: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    if !flag(recovery, "newer_state_analysed") {
        return vec![
            fit(
                &format!(
                    "  {} the newer-state analysis did not run",
                    Symbol::Unknown.glyph(charset)
                ),
                width,
            ),
            fit(
                &format!(
                    "  {} what recovery would discard is unknown, not nothing",
                    Symbol::Unknown.glyph(charset)
                ),
                width,
            ),
        ];
    }
    let items = items(recovery, "newer_state");
    let losses = items.iter().filter(|item| is_loss(item)).count();
    let preserved = items
        .iter()
        .filter(|item| text(*item, "class").as_deref() == Some("preserved-by-method"))
        .count();
    let destroyed = list_len(recovery, "destroyed_assets");
    let mut lines = vec![fit(
        &format!(
            "  {} after the recovery point",
            counted(items.len(), "object changed", "objects changed")
        ),
        width,
    )];
    if losses > 0 {
        lines.push(fit(
            &format!(
                "  {} {} would be discarded",
                Symbol::Risk.glyph(charset),
                counted(losses, "object", "objects")
            ),
            width,
        ));
    }
    if destroyed > 0 {
        lines.push(fit(
            &format!(
                "  {} {} would be destroyed",
                Symbol::Risk.glyph(charset),
                counted(destroyed, "newer snapshot", "newer snapshots")
            ),
            width,
        ));
    }
    if let Some(bytes) = byte_size(recovery, "discarded_size") {
        lines.push(fit(&format!("  {bytes} of changed blocks"), width));
    }
    if losses == 0 && destroyed == 0 {
        lines.push(fit(
            &format!("  {preserved} of them would be left alone by this method"),
            width,
        ));
    }
    lines
}

/// `/etc/hosts  changed after plan` (§24.4's preserved and discarded lists).
fn item_line(item: &Item) -> String {
    let mut line = text(item, "object").unwrap_or_else(|| "unnamed".to_owned());
    if text(item, "class").as_deref() == Some("conflicting") {
        // Appendix C.4: the recovery target and the newer state are the same object, so this is
        // not a bystander that happens to be in the way.
        line.push_str("  conflicting with the restore set");
    }
    if let Some(detail) = text(item, "detail") {
        line.push_str(&format!("  {detail}"));
    }
    line
}

/// `! TCP sessions - a live session cannot be re-established` (§24.3, §35.2).
///
/// §35.3 keeps a named compensation on its own line rather than moving the effect out of the
/// list: a compensating action makes the domain compensatable and leaves the original effect
/// exactly as irreversible as it was.
fn unrecoverable_lines(effect: &Item, width: usize, charset: Charset) -> Vec<String> {
    let subject = text(effect, "subject").unwrap_or_else(|| "unnamed".to_owned());
    let domain = text(effect, "domain").unwrap_or_else(|| "unknown".to_owned());
    let mut head = format!("  {} {subject}  {domain}", Symbol::Risk.glyph(charset));
    if let Some(reason) = text(effect, "reason") {
        head.push_str(&format!(" - {reason}"));
    }
    let mut lines = vec![fit(&head, width)];
    if let Some(compensation) = text(effect, "compensation") {
        lines.push(fit(
            &format!(
                "    {} compensated by {compensation}",
                Symbol::CompensationOnly.glyph(charset)
            ),
            width,
        ));
    }
    lines
}

/// §24.5's gate and §13.7's operational preconditions, in one block.
fn requirements(recovery: &RecordValue) -> Vec<String> {
    let mut requirements = Vec::new();
    if flag(recovery, "requires_acceptance") {
        requirements.push("explicit accept-newer-state-loss".to_owned());
    }
    if flag(recovery, "requires_reboot") {
        requirements.push("a reboot".to_owned());
    }
    if flag(recovery, "requires_offline") {
        requirements.push("the filesystem offline".to_owned());
    }
    if text(recovery, "method").as_deref() == Some("dataset-rollback") {
        requirements
            .push("acceptance that the whole dataset returns to the captured point".to_owned());
    }
    requirements
}
