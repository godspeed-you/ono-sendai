//! The coverage matrix (§10.3, §13.8) and the block Appendix E.8 forbids reducing to a badge.
//!
//! §10.1 refuses `reversible: true/false`, and this module is what the refusal looks like on a
//! terminal. A plan's protection is a matrix — one row per effect domain, each with the recovery
//! it needs and the recovery it has — and §10.3 ends with the sentence the whole module is built
//! around: *"The plan-level summary must never hide this matrix."*
//!
//! Appendix E.8 makes that structural rather than editorial. There is **no public function here
//! that returns the protection level on its own**: [`protection_block`] emits the level and the
//! exclusions in one call, [`coverage_matrix`] emits every row and then the exclusions, and the
//! compact form the collapsed view uses is `pub(crate)` and carries the exclusions too. A caller
//! that wants to print `PROTECTED` and stop has nothing to call, which is the only way §62.6's
//! failure mode stays unreachable as the crate grows.
//!
//! §11.5 is the other claim that never goes quiet: a copy-on-write snapshot lives on the storage
//! it protects, and [`recovery_asset_block`] says so on its own line rather than leaving the
//! reader to infer it from the word "snapshot".

use ono_change_core::{
    CoverageExclusion, DomainCoverage, ProtectionLevel, ProtectionSummary, RecoveryAsset,
};

use crate::symbols::{Charset, Symbol};
use crate::{counted, display_width, fit, heading, join_fitted, labelled, safe};

/// The heading every exclusion list carries, so a reader can find it by eye (Appendix E.8).
pub(crate) const NOT_COVERED: &str = "not covered";

/// How wide the label column of an asset block is (§13.8's alignment).
const LABEL: usize = 14;

/// The protection block of §20.2, §20.4 and §64: the level, the assets, the matrix, the exclusions.
///
/// Appendix E.8 is the contract: the level and the exclusions leave this function together or not
/// at all. `summary` supplies both, and an analysis that recorded no exclusions renders that fact
/// rather than an empty space, because a blank where the residual risk goes reads as "there is
/// none" — which is exactly §62.6's "protection as permission to be reckless".
///
/// §20.4 puts this block near the top of the plan view and forbids hiding it behind a verbose
/// inspector, so it takes no verbosity parameter to hide it behind.
#[must_use]
pub fn protection_block(
    summary: &ProtectionSummary,
    assets: &[RecoveryAsset],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let mut lines = vec![fit(
        &format!("  {}", level_line(summary, charset, true)),
        width,
    )];
    for asset in assets {
        lines.push(String::new());
        lines.push(fit(&format!("  {}", asset_title(asset)), width));
        for line in asset_facts(asset, charset) {
            lines.push(fit(&format!("  {line}"), width));
        }
    }
    if !summary.rows().is_empty() {
        lines.push(String::new());
        lines.push("  covered".to_owned());
        lines.extend(matrix_rows(summary, 4, width, charset));
    }
    lines.push(String::new());
    lines.extend(exclusion_rows(summary, 2, width, charset));
    lines
}

/// §10.3's matrix on its own, for `plan --protection` and the inspector's protection pane.
///
/// One row per effect domain, each with what recovery it needs and what it has, then — always —
/// the exclusions, because a matrix without them is the summary §10.3 forbids.
#[must_use]
pub fn coverage_matrix(summary: &ProtectionSummary, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = vec![fit(
        &format!("  {}", level_line(summary, charset, true)),
        width,
    )];
    lines.push(String::new());
    lines.push("coverage".to_owned());
    if summary.rows().is_empty() {
        lines.push(fit("  no domain was analysed", width));
    } else {
        lines.extend(matrix_rows(summary, 2, width, charset));
    }
    lines.push(String::new());
    lines.extend(exclusion_rows(summary, 0, width, charset));
    lines
}

/// One asset in §13.8's shape, with what it does not cover underneath it.
///
/// §11.5 forbids calling a copy-on-write snapshot a backup, so an asset that shares the failure
/// domain of what it protects says so with §20.3's risk mark. §11.4 is the other visible rule:
/// an asset that no validation has confirmed is shown in the state it is actually in.
#[must_use]
pub fn recovery_asset_block(asset: &RecoveryAsset, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = vec![fit(&asset_title(asset).to_uppercase(), width)];
    for line in asset_facts(asset, charset) {
        lines.push(fit(&line, width));
    }
    heading(&mut lines, "excluded");
    if asset.exclusions().is_empty() {
        lines.push(fit("  no exclusion was recorded for this asset", width));
    }
    for exclusion in asset.exclusions() {
        lines.push(fit(
            &labelled(&safe(exclusion.subject()), &safe(exclusion.reason()), LABEL),
            width,
        ));
    }
    lines
}

/// The compact protection footer Appendix E.2 puts under a collapsed plan.
///
/// It is `pub(crate)` on purpose. Appendix E.8 allows a *summary* of coverage and requires the
/// summary to show exclusions, so this returns both lines together and nothing outside the crate
/// can take only the first.
pub(crate) fn coverage_summary(
    summary: &ProtectionSummary,
    label: usize,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let required: Vec<&DomainCoverage> = summary.required_rows().collect();
    let short = summary.shortfall().len();
    let covered = required.len().saturating_sub(short);
    let counts = format!(
        "{}, {}",
        counted(covered, "domain covered", "domains covered"),
        counted(short, "not covered", "not covered")
    );
    let head = crate::column_pair(
        "protection",
        &format!("{}  {counts}", level_line(summary, charset, false)),
        label,
    );
    let exclusions: Vec<String> = summary
        .exclusions()
        .iter()
        .map(|exclusion| safe(exclusion.subject()))
        .collect();
    let tail = if exclusions.is_empty() {
        "no exclusion recorded".to_owned()
    } else {
        join_fitted(
            &exclusions,
            width.saturating_sub(label + NOT_COVERED.len() + 2),
        )
    };
    vec![
        fit(&head, width),
        fit(
            &format!("{}{NOT_COVERED}  {tail}", " ".repeat(label)),
            width,
        ),
    ]
}

/// `PROTECTED <->` — the level word §10.2 spells and the mark §20.3 gives it.
///
/// Private, and it stays private: this is the badge Appendix E.8 forbids, and the only callers
/// are the functions above, each of which emits the exclusions in the same breath.
fn level_line(summary: &ProtectionSummary, charset: Charset, consistency: bool) -> String {
    let level = summary.level();
    let mark = Symbol::for_protection(level).glyph(charset);
    let mut line = format!("{} {mark}", level_word(level));
    if consistency && let Some(class) = summary.consistency() {
        line.push_str(&format!("  {}", class.as_str()));
    }
    line
}

/// §10.2 spells the plan-level words in upper case with underscores; the vocabulary uses hyphens.
fn level_word(level: ProtectionLevel) -> String {
    level.as_str().to_uppercase().replace('-', "_")
}

/// One line per domain: what it needs, what covers it, and the mark that says which (§10.3).
fn matrix_rows(
    summary: &ProtectionSummary,
    indent: usize,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let pad = " ".repeat(indent);
    let domain_column = summary
        .rows()
        .iter()
        .map(|row| display_width(row.domain().as_str()))
        .max()
        .unwrap_or(0)
        .max(20)
        + 2;
    let objective_column = 18;
    summary
        .rows()
        .iter()
        .map(|row| {
            // §2.4: an unmet requirement is marked as risk rather than left to the reader to
            // notice that one word in a column of similar words is the wrong one.
            let mark = if row.is_required() && !row.is_satisfied() {
                Symbol::Risk
            } else {
                Symbol::for_protection(row.protection().level())
            };
            let mut line = format!(
                "{pad}{:<domain_column$}{:<objective_column$}{:<15}{}",
                row.domain().as_str(),
                row.objective().as_str(),
                row.protection().as_str().to_uppercase(),
                mark.glyph(charset),
            );
            if row.is_declared_irrelevant() {
                line.push_str("  policy: irrelevant");
            }
            fit(&line, width)
        })
        .collect()
}

/// The `not covered` block, which every path through this module emits (Appendix E.8, §62.6).
///
/// An empty exclusion list is rendered as the sentence that it is empty. §10.5's rule that an
/// unknown is never an empty string applies to a whole block as much as to a cell: silence here
/// would be read as a promise.
fn exclusion_rows(
    summary: &ProtectionSummary,
    indent: usize,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let pad = " ".repeat(indent);
    let mut lines = vec![format!("{pad}{NOT_COVERED}")];
    let exclusions = summary.exclusions();
    if exclusions.is_empty() {
        lines.push(fit(
            &format!("{pad}  no exclusion was recorded for this plan"),
            width,
        ));
        return lines;
    }
    for exclusion in exclusions {
        lines.push(fit(&exclusion_line(exclusion, &pad, charset), width));
    }
    lines
}

/// `! active TCP sessions — nothing restores a live session` (§2.13, §20.3).
fn exclusion_line(exclusion: &CoverageExclusion, pad: &str, charset: Charset) -> String {
    let mark = if exclusion.is_irreversible() {
        format!("{} ", Symbol::Risk.glyph(charset))
    } else {
        String::new()
    };
    let subject = safe(exclusion.subject());
    let reason = safe(exclusion.reason());
    if reason.is_empty() {
        format!("{pad}  {mark}{subject}")
    } else {
        format!("{pad}  {mark}{subject} - {reason}")
    }
}

/// `planned recovery asset` before it exists, `recovery asset` once it does (§2.1, §11.1).
fn asset_title(asset: &RecoveryAsset) -> String {
    if asset.state().is_usable() {
        "recovery asset".to_owned()
    } else {
        format!("{} recovery asset", asset.state().as_str())
    }
}

/// The facts §13.8 prints about one asset, indented two under its title.
fn asset_facts(asset: &RecoveryAsset, charset: Charset) -> Vec<String> {
    let retention = if asset.retention().is_held() {
        "held until released".to_owned()
    } else {
        format!(
            "{} after verification",
            crate::compact_duration(asset.retention().window().as_secs())
        )
    };
    let mut facts = vec![
        labelled("type", asset.asset_type().as_str(), LABEL),
        labelled("scope", &safe(asset.scope().domain()), LABEL),
        labelled("reference", &safe(asset.reference()), LABEL),
        labelled("consistency", asset.consistency().as_str(), LABEL),
        labelled("state", asset.state().as_str(), LABEL),
        labelled("restore", asset.restore_method().as_str(), LABEL),
        labelled("retained", &retention, LABEL),
    ];
    // §11.5 and §14.7: a local snapshot is a recovery point and not a backup, and the difference
    // is a fact of the mechanism rather than a caveat somebody remembered to add.
    if asset.asset_type().shares_failure_domain() {
        facts.push(labelled(
            "",
            &format!(
                "{} shares the failure domain of what it protects",
                Symbol::Risk.glyph(charset)
            ),
            LABEL,
        ));
    }
    if asset.validation().is_none() {
        facts.push(labelled(
            "",
            &format!(
                "{} no validation has confirmed it",
                Symbol::Unknown.glyph(charset)
            ),
            LABEL,
        ));
    }
    facts
}
