//! The recovery plan view (§24.4, §24.5, Appendix E.6).
//!
//! Appendix E.6 fixes the layout and gives the reason in one sentence: recovery *"uses a visually
//! distinct title and always shows newer-state impact above action details"*. §24.2 is why —
//! since the original plan ran, files have been written, packages updated and snapshots taken,
//! and a naive rollback destroys them. An operator who reads the restore target first and the
//! losses second has already decided by the time they reach the part that matters.
//!
//! §62.8 shapes the block that carries it. An analysis that never ran is not an analysis that
//! found nothing, so [`ono_change_core::NewerStateImpact::is_complete`] is checked before
//! anything is counted, and an unanalysed recovery says so with §20.3's unknown mark. §56.3 makes
//! that a reason to block, and [`ono_change_core::RecoveryPlan::needs_destructive_acceptance`]
//! already answers `true` for it, which is what puts the `requires` block on the view.
//!
//! §27.4 is the vocabulary rule: a compensating action is not a restore, and nothing here calls
//! one the other. The method the plan chose is printed as the method it is.

use ono_change_core::{
    NewerStateClass, NewerStateImpact, RecoveryAssetId, RecoveryPlan, RestoreMethod,
    UnrecoverableEffect,
};

use crate::symbols::{Charset, Symbol};
use crate::{counted, fit, heading, safe};

/// The line that closes a recovery view, for the reason `PLAN NOT EXECUTED` closes a plan.
///
/// §5.8 and §24.1 make `recover` a planner. An operator who has just read what recovery would
/// destroy needs to know, without inferring it, that none of it has happened yet.
pub const RECOVERY_NOT_EXECUTED: &str = "RECOVERY NOT EXECUTED";

/// The heading Appendix E.6 puts above everything else in a recovery view.
pub const NEWER_STATE_AT_RISK: &str = "NEWER STATE AT RISK";

/// Appendix E.6's recovery plan view, with §24.4's and §24.5's blocks under it.
///
/// The two block titles Appendix E.6 names are upper case and the rest are §24.4's lower-case
/// headings, which is what makes the newer-state impact and the restore target findable at a
/// glance in a view whose other blocks read as prose.
///
/// §24.5's full-rollback shape is the same function: a method that discards newer state fills the
/// `will discard`, `will destroy` and `requires` blocks, and one that does not leaves them out
/// rather than printing zeroes.
#[must_use]
pub fn recovery_view(recovery: &RecoveryPlan, width: usize, charset: Charset) -> Vec<String> {
    let plan = recovery.plan();
    let mut lines = vec![fit(
        &format!("RECOVERY PLAN / {}", plan.id().short()),
        width,
    )];

    // Appendix E.6: newer-state impact stands above the action details, always.
    heading(&mut lines, NEWER_STATE_AT_RISK);
    lines.extend(newer_state_lines(recovery.newer_state(), width, charset));

    heading(&mut lines, "RESTORE TARGET");
    lines.push(fit(&format!("  {}", safe(recovery.target_state())), width));

    heading(&mut lines, "source");
    match recovery.source_plan() {
        Some(source) => lines.push(fit(&format!("  plan {}", source.short()), width)),
        None => lines.push(fit("  no source plan was recorded", width)),
    }
    if recovery.source_assets().is_empty() {
        lines.push(fit("  no recovery asset was named", width));
    }
    for asset in recovery.source_assets() {
        // §36.4: the reference an operator types back is the short one, and it is the same
        // short one `get recovery` printed.
        lines.push(fit(
            &format!(
                "  recovery asset {}{}",
                RecoveryAssetId::PREFIX,
                asset.short()
            ),
            width,
        ));
    }

    heading(&mut lines, "restore");
    if recovery.restores().is_empty() {
        lines.push(fit("  no object was named for restore", width));
    }
    for object in recovery.restores() {
        lines.push(fit(&format!("  {}", safe(object)), width));
    }

    heading(&mut lines, "method");
    lines.push(fit(&format!("  {}", recovery.method().as_str()), width));
    lines.push(fit(&format!("  {}", recovery.goal().as_str()), width));
    lines.push(fit(
        &format!("  {}", recovery.directory_restore_policy().as_str()),
        width,
    ));

    let preserved = recovery.newer_state().preserved();
    if !preserved.is_empty() {
        heading(&mut lines, "newer state preserved");
        for item in preserved {
            lines.push(fit(&format!("  {}", item_line(item)), width));
        }
    }

    let losses = recovery.newer_state().losses();
    if !losses.is_empty() {
        heading(&mut lines, "will discard");
        for item in losses {
            lines.push(fit(
                &format!("  {} {}", Symbol::Risk.glyph(charset), item_line(item)),
                width,
            ));
        }
        if let Some(bytes) = recovery.newer_state().discarded_bytes() {
            lines.push(fit(&format!("  {bytes} of changed blocks"), width));
        }
    }

    if !recovery.newer_state().destroyed_assets().is_empty() {
        heading(&mut lines, "will destroy");
        for asset in recovery.newer_state().destroyed_assets() {
            lines.push(fit(
                &format!("  {} {}", Symbol::Risk.glyph(charset), safe(asset)),
                width,
            ));
        }
    }

    heading(&mut lines, "not recoverable");
    if recovery.unrecoverable().is_empty() {
        lines.push(fit("  nothing was recorded as unrecoverable", width));
    }
    for effect in recovery.unrecoverable() {
        lines.extend(unrecoverable_lines(effect, width, charset));
    }

    let gaps = recovery.metadata().gaps();
    if !gaps.is_empty() {
        // Appendix C.7: a restore that returns the bytes and loses the SELinux label has not
        // returned the file, and the operator finds that out here or in an audit.
        heading(&mut lines, "metadata not restored");
        for gap in gaps {
            lines.push(fit(
                &format!("  {} {gap}", Symbol::Unknown.glyph(charset)),
                width,
            ));
        }
    }

    heading(&mut lines, "risk");
    lines.push(fit(
        &format!("  {}", plan.risk().classify().as_str().to_uppercase()),
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

    if !plan.state().has_mutated() {
        lines.push(String::new());
        lines.push(RECOVERY_NOT_EXECUTED.to_owned());
    }
    lines
}

/// Appendix E.6's counts, or §62.8's admission that nothing counted them.
///
/// The order matters: the unanalysed case is checked first and returns, so there is no path on
/// which an empty item list renders as "nothing would be lost". §62.8 names recovering hours
/// later without considering newer state as unacceptable, and a view that reports the absence of
/// an analysis as an absence of risk is how that happens.
fn newer_state_lines(impact: &NewerStateImpact, width: usize, charset: Charset) -> Vec<String> {
    if !impact.is_complete() {
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
    let losses = impact.losses().len();
    let preserved = impact.preserved().len();
    let destroyed = impact.destroyed_assets().len();
    let mut lines = vec![fit(
        &format!(
            "  {} after the recovery point",
            counted(impact.items().len(), "object changed", "objects changed")
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
    if let Some(bytes) = impact.discarded_bytes() {
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
fn item_line(item: &ono_change_core::NewerStateItem) -> String {
    let mut line = safe(item.object());
    if item.class() == NewerStateClass::Conflicting {
        // Appendix C.4: the recovery target and the newer state are the same object, so this is
        // not a bystander that happens to be in the way.
        line.push_str("  conflicting with the restore set");
    }
    let detail = safe(item.detail());
    if !detail.is_empty() {
        line.push_str(&format!("  {detail}"));
    }
    line
}

/// `! TCP sessions - a live session cannot be re-established` (§24.3, §35.2).
///
/// §35.3 keeps a named compensation on the same line rather than moving the effect out of the
/// list: a compensating action makes the domain compensatable and leaves the original effect
/// exactly as irreversible as it was.
fn unrecoverable_lines(
    effect: &UnrecoverableEffect,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let mut head = format!(
        "  {} {}  {}",
        Symbol::Risk.glyph(charset),
        safe(effect.subject()),
        effect.domain().as_str()
    );
    let reason = safe(effect.reason());
    if !reason.is_empty() {
        head.push_str(&format!(" - {reason}"));
    }
    let mut lines = vec![fit(&head, width)];
    if let Some(compensation) = effect.compensation() {
        lines.push(fit(
            &format!(
                "    {} compensated by {}",
                Symbol::CompensationOnly.glyph(charset),
                safe(compensation)
            ),
            width,
        ));
    }
    lines
}

/// §24.5's gate and §13.7's operational preconditions, in one block.
fn requirements(recovery: &RecoveryPlan) -> Vec<String> {
    let mut requirements = Vec::new();
    if recovery.needs_destructive_acceptance() {
        requirements.push("explicit accept-newer-state-loss".to_owned());
    }
    if recovery.requires_reboot() {
        requirements.push("a reboot".to_owned());
    }
    if recovery.requires_offline() {
        requirements.push("the filesystem offline".to_owned());
    }
    if recovery.method() == RestoreMethod::DatasetRollback {
        requirements
            .push("acceptance that the whole dataset returns to the captured point".to_owned());
    }
    requirements
}
