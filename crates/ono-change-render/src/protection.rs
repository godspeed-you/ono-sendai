//! The coverage matrix (§10.3, §13.8) and the block Appendix E.8 forbids reducing to a badge.
//!
//! §10.1 refuses `reversible: true/false`, and this module is what the refusal looks like on a
//! terminal. A plan's protection is a matrix — `ono.change-plan/1`'s `protection`, one
//! `ono.protection-coverage/1` per mutation domain — and §10.3 ends with the sentence the whole
//! module is built around: *"The plan-level summary must never hide this matrix."*
//!
//! `protection_level` is **read**, never derived. The coverage algorithm of Appendix A.5 composes
//! it and Appendix A.7 caps it; a renderer that recomputed it would eventually disagree with the
//! plan it is drawing, and the disagreement would favour whichever side was more optimistic.
//!
//! Appendix E.8 makes the prohibition structural. There is **no public function here that returns
//! the protection level on its own**: [`protection_block`] emits the level and the exclusions in
//! one call, [`coverage_matrix`] emits every row and then the exclusions, and the compact form the
//! collapsed view uses is `pub(crate)` and carries the exclusions too. A caller that wants to
//! print `PROTECTED` and stop has nothing to call, which is the only way §62.6's failure mode
//! stays unreachable as the crate grows.
//!
//! §11.5 is the other claim that never goes quiet: a copy-on-write snapshot lives on the storage
//! it protects, and `ono.recovery-asset/1` carries `shares_failure_domain` so
//! [`recovery_asset_block`] states it on its own line rather than leaving the reader to infer it
//! from the word "snapshot".

use ono_value::RecordValue;

use crate::symbols::{Charset, Symbol};
use crate::{
    Item, compact_duration, counted, display_width, duration_seconds, fit, flag, heading, items,
    join_fitted, labelled, nested, strings, text,
};

/// The heading every exclusion list carries, so a reader can find it by eye (Appendix E.8).
pub(crate) const NOT_COVERED: &str = "not covered";

/// How wide the label column of an asset block is (§13.8's alignment).
const LABEL: usize = 14;

/// The protection block of §20.2, §20.4 and §64: the level, the assets, the matrix, the exclusions.
///
/// `plan` is an `ono.change-plan/1`; `assets` are the `ono.recovery-asset/1` records the plan
/// proposes. They are passed in rather than reached for, because §50.1 forbids this crate from
/// asking a provider anything and §2.1 means a proposed asset does not exist yet in any case.
///
/// Appendix E.8 is the contract: the level and the exclusions leave this function together or not
/// at all. A plan whose `coverage_exclusions` list is empty renders that fact rather than an empty
/// space, because a blank where the residual risk goes reads as "there is none" — which is exactly
/// §62.6's "protection as permission to be reckless".
///
/// §20.4 puts this block near the top of the plan view and forbids hiding it behind a verbose
/// inspector, so it takes no verbosity parameter to hide it behind.
#[must_use]
pub fn protection_block(
    plan: &RecordValue,
    assets: &[RecordValue],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let mut lines = vec![fit(
        &format!("  {}", level_line(plan, charset, true)),
        width,
    )];
    for asset in assets {
        lines.push(String::new());
        lines.push(fit(&format!("  {}", asset_title(asset)), width));
        for line in asset_facts(asset, charset) {
            lines.push(fit(&format!("  {line}"), width));
        }
    }
    let rows = items(plan, "protection");
    if !rows.is_empty() {
        lines.push(String::new());
        // §10.3's matrix has one row per mutation domain and says what each one reached, which
        // is `UNPROTECTED` as often as anything else. Heading it `covered` labelled a column of
        // `UNPROTECTED` rows as coverage — §2.5's overstatement, written by the renderer rather
        // than by the engine.
        lines.push("  by domain".to_owned());
        lines.extend(matrix_rows(&rows, 4, width, charset));
        lines.extend(not_protected_by(&rows, 2, width));
    }
    lines.push(String::new());
    lines.extend(exclusion_rows(plan, 2, width, charset));
    lines
}

/// §10.3's matrix on its own, for `plan --protection` and the inspector's protection pane.
///
/// One row per effect domain, each with what recovery it needs and what it has, then — always —
/// the exclusions, because a matrix without them is the summary §10.3 forbids.
#[must_use]
pub fn coverage_matrix(plan: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = vec![fit(
        &format!("  {}", level_line(plan, charset, true)),
        width,
    )];
    lines.push(String::new());
    lines.push("coverage".to_owned());
    let rows = items(plan, "protection");
    if rows.is_empty() {
        lines.push(fit("  no domain was analysed", width));
    } else {
        lines.extend(matrix_rows(&rows, 2, width, charset));
        lines.extend(not_protected_by(&rows, 0, width));
    }
    lines.push(String::new());
    lines.extend(exclusion_rows(plan, 0, width, charset));
    lines
}

/// §13.4's `NOT PROTECTED BY`: the enclosing objects whose snapshot would not reach a row's domain.
///
/// A row that names none draws nothing, so the block appears exactly where a snapshot of a parent
/// or of the filesystem mounted above could be mistaken for the one that protects the target.
fn not_protected_by(rows: &[Item], indent: usize, width: usize) -> Vec<String> {
    let named: Vec<(String, String)> = rows
        .iter()
        .flat_map(|row| {
            let domain = text(row, "domain").unwrap_or_default();
            strings(row, "not_protected_by")
                .into_iter()
                .map(move |object| (domain.clone(), object))
        })
        .collect();
    if named.is_empty() {
        return Vec::new();
    }
    let pad = " ".repeat(indent);
    let mut lines = vec![String::new(), format!("{pad}NOT PROTECTED BY")];
    for (domain, object) in named {
        lines.push(fit(
            &format!("{pad}  a snapshot of {object}  ({domain})"),
            width,
        ));
    }
    lines
}

/// One `ono.recovery-asset/1` in §13.8's shape, with what it does not cover underneath it.
///
/// §11.5 forbids calling a copy-on-write snapshot a backup, so an asset whose
/// `shares_failure_domain` is true says so with §20.3's risk mark. §11.4 is the other visible
/// rule: an asset with no `validation` is shown in the state it is actually in.
#[must_use]
pub fn recovery_asset_block(asset: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let mut lines = vec![fit(&asset_title(asset).to_uppercase(), width)];
    for line in asset_facts(asset, charset) {
        lines.push(fit(&line, width));
    }
    heading(&mut lines, "excluded");
    let exclusions = items(asset, "exclusions");
    if exclusions.is_empty() {
        lines.push(fit("  no exclusion was recorded for this asset", width));
    }
    for exclusion in &exclusions {
        let subject = text(exclusion, "subject").unwrap_or_else(|| "unnamed".to_owned());
        let reason = text(exclusion, "reason").unwrap_or_default();
        lines.push(fit(&labelled(&subject, &reason, LABEL), width));
    }
    lines
}

/// The compact protection footer Appendix E.2 puts under a collapsed plan.
///
/// It is `pub(crate)` on purpose. Appendix E.8 allows a *summary* of coverage and requires the
/// summary to show exclusions, so this returns both lines together and nothing outside the crate
/// can take only the first.
pub(crate) fn coverage_summary(
    plan: &RecordValue,
    label: usize,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let rows = items(plan, "protection");
    let required: Vec<&Item> = rows.iter().filter(|row| flag(*row, "required")).collect();
    let covered = required
        .iter()
        .filter(|row| flag(**row, "satisfied"))
        .count();
    let short = required.len().saturating_sub(covered);
    let counts = format!(
        "{}, {}",
        counted(covered, "domain covered", "domains covered"),
        counted(short, "not covered", "not covered")
    );
    let head = crate::column_pair(
        "protection",
        &format!("{}  {counts}", level_line(plan, charset, false)),
        label,
    );
    let exclusions: Vec<String> = items(plan, "coverage_exclusions")
        .iter()
        .filter_map(|exclusion| text(exclusion, "subject"))
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

/// `PROTECTED <->` — the word §10.2 spells and the mark §20.3 gives it, read off the record.
///
/// Private, and it stays private: this is the badge Appendix E.8 forbids, and the only callers
/// are the functions above, each of which emits the exclusions in the same breath.
fn level_line(plan: &RecordValue, charset: Charset, consistency: bool) -> String {
    let level = text(plan, "protection_level").unwrap_or_else(|| "unknown".to_owned());
    let mark = Symbol::for_protection(&level).glyph(charset);
    let mut line = format!("{} {mark}", level_word(&level));
    // The weakest consistency any covering row claims. §11.3's classes are not interchangeable and
    // Appendix D.7 forbids inventing cross-mechanism atomicity, so the weakest is the only honest
    // one to print beside a plan-level word.
    if consistency && let Some(class) = weakest_consistency(plan) {
        line.push_str(&format!("  {class}"));
    }
    line
}

/// §10.2 spells the plan-level words in upper case with underscores; the wire uses hyphens.
fn level_word(level: &str) -> String {
    level.to_uppercase().replace('-', "_")
}

/// The weakest consistency claim among the rows that actually cover something (§11.3).
fn weakest_consistency(plan: &RecordValue) -> Option<String> {
    const ORDER: [&str; 6] = [
        "unknown",
        "byte-consistent",
        "crash-consistent",
        "filesystem-consistent",
        "application-consistent",
        "transaction-consistent",
    ];
    items(plan, "protection")
        .iter()
        .filter(|row| flag(*row, "required") && flag(*row, "satisfied"))
        .filter_map(|row| text(row, "consistency"))
        .min_by_key(|class| ORDER.iter().position(|name| *name == class).unwrap_or(0))
}

/// One line per domain: what it needs, what covers it, and the mark that says which (§10.3).
fn matrix_rows(rows: &[Item], indent: usize, width: usize, charset: Charset) -> Vec<String> {
    let pad = " ".repeat(indent);
    // Every column is as wide as its widest word plus a gap, rather than a number chosen against
    // the words that existed when it was written. `no-recovery-required` is twenty columns and
    // the objective field was eighteen, so `no-recovery-requiredUNPROTECTED` is what an operator
    // saw — two of Appendix E.8's cells with no boundary between them.
    let column = |field: &str, least: usize| {
        rows.iter()
            .filter_map(|row| text(row, field))
            .map(|value| display_width(&value))
            .max()
            .unwrap_or(0)
            .max(least)
            + 2
    };
    let domain_column = column("domain", 20);
    let objective_column = column("objective", 16);
    let protection_column = rows
        .iter()
        .filter_map(|row| text(row, "protection"))
        .map(|value| display_width(&value))
        .max()
        .unwrap_or(0)
        .max(13)
        + 2;
    rows.iter()
        .map(|row| {
            let domain = text(row, "domain").unwrap_or_else(|| "unknown".to_owned());
            let objective = text(row, "objective").unwrap_or_else(|| "unknown".to_owned());
            let protection = text(row, "protection").unwrap_or_else(|| "unknown".to_owned());
            // §2.4: an unmet requirement is marked as risk rather than left to the reader to
            // notice that one word in a column of similar words is the wrong one.
            let mark = if flag(row, "required") && !flag(row, "satisfied") {
                Symbol::Risk
            } else {
                Symbol::for_protection(&protection)
            };
            let mut line = format!(
                "{pad}{domain:<domain_column$}{objective:<objective_column$}\
                 {:<protection_column$}{}",
                protection.to_uppercase(),
                mark.glyph(charset),
            );
            if flag(row, "declared_irrelevant") {
                line.push_str("  policy: irrelevant");
            }
            fit(&line, width)
        })
        .collect()
}

/// The `not covered` block, which every path through this module emits (Appendix E.8, §62.6).
///
/// The list is `ono.change-plan/1`'s own `coverage_exclusions`, which Appendix A.5 requires to
/// travel beside the class. An empty list is rendered as the sentence that it is empty: §10.5's
/// rule that an unknown is never an empty string applies to a whole block as much as to a cell,
/// and silence here would be read as a promise.
fn exclusion_rows(
    plan: &RecordValue,
    indent: usize,
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let pad = " ".repeat(indent);
    let mut lines = vec![format!("{pad}{NOT_COVERED}")];
    let exclusions = items(plan, "coverage_exclusions");
    if exclusions.is_empty() {
        lines.push(fit(
            &format!("{pad}  no exclusion was recorded for this plan"),
            width,
        ));
        return lines;
    }
    for exclusion in &exclusions {
        lines.push(fit(&exclusion_line(exclusion, &pad, charset), width));
    }
    lines
}

/// `! active TCP sessions - a live session cannot be re-established` (§2.13, §20.3).
fn exclusion_line(exclusion: &Item, pad: &str, charset: Charset) -> String {
    let mark = if flag(exclusion, "irreversible") {
        format!("{} ", Symbol::Risk.glyph(charset))
    } else {
        String::new()
    };
    let subject = text(exclusion, "subject").unwrap_or_else(|| "unnamed".to_owned());
    match text(exclusion, "reason") {
        Some(reason) => format!("{pad}  {mark}{subject} - {reason}"),
        None => format!("{pad}  {mark}{subject}"),
    }
}

/// `proposed recovery asset` before it exists, `recovery asset` once it does (§2.1, §11.1).
fn asset_title(asset: &RecordValue) -> String {
    match text(asset, "state").as_deref() {
        Some("ready") => "recovery asset".to_owned(),
        Some(state) => format!("{state} recovery asset"),
        None => "recovery asset".to_owned(),
    }
}

/// The facts §13.8 prints about one asset, indented two under its title.
fn asset_facts(asset: &RecordValue, charset: Charset) -> Vec<String> {
    let scope = nested(asset, "scope");
    let domain = scope
        .as_ref()
        .and_then(|scope| text(scope, "domain"))
        .unwrap_or_else(|| "unknown".to_owned());
    let retention = if flag(asset, "held") {
        "held until released".to_owned()
    } else {
        duration_seconds(asset, "retention").map_or_else(
            || "unknown".to_owned(),
            |seconds| format!("{} after verification", compact_duration(seconds)),
        )
    };
    let mut facts = vec![
        labelled(
            "type",
            &text(asset, "type").unwrap_or_else(|| "unknown".to_owned()),
            LABEL,
        ),
        labelled("scope", &domain, LABEL),
        labelled(
            "reference",
            &text(asset, "reference").unwrap_or_else(|| "unknown".to_owned()),
            LABEL,
        ),
        labelled(
            "consistency",
            &text(asset, "consistency").unwrap_or_else(|| "unknown".to_owned()),
            LABEL,
        ),
        labelled(
            "state",
            &text(asset, "state").unwrap_or_else(|| "unknown".to_owned()),
            LABEL,
        ),
        labelled(
            "restore",
            &text(asset, "restore_method").unwrap_or_else(|| "unknown".to_owned()),
            LABEL,
        ),
        labelled("retained", &retention, LABEL),
    ];
    let covers = scope.as_ref().map(|scope| strings(scope, "covers"));
    if let Some(covers) = covers.filter(|covers| !covers.is_empty()) {
        facts.push(labelled("covers", &covers.join(", "), LABEL));
    }
    // §11.5 and §14.7: a local snapshot is a recovery point and not a backup, and the difference
    // is a fact the record carries rather than a caveat somebody remembered to add.
    if flag(asset, "shares_failure_domain") {
        facts.push(labelled(
            "",
            &format!(
                "{} shares the failure domain of what it protects",
                Symbol::Risk.glyph(charset)
            ),
            LABEL,
        ));
    }
    if nested(asset, "validation").is_none() {
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
