//! The default plan view (§20.1, §20.2, §20.4, §64).
//!
//! §20.1 fixes the order the view answers the operator's questions in, and the order is the
//! product: a reader scans *intent -> impact -> protection -> residual risk -> verification* in
//! seconds (Appendix E.1) because every plan puts those things in the same places. [`Section`]
//! is that order written down once, with each section carrying the §20.1 question it answers, so
//! an edit that reorders the view fails a test rather than quietly changing what the reader
//! learns first.
//!
//! Three rules are load bearing here:
//!
//! - **§20.4: protection is prominent.** It is the fifth of ten sections, above risk and above
//!   verification, and [`plan_view`] has no verbosity parameter that could hide it.
//! - **§2.1 and §62.3: planning is side-effect free.** The view closes with
//!   [`PLAN_NOT_EXECUTED`] for every state before the first mutation. Appendix F makes that
//!   predicate — [`ono_change_core::PlanState::has_mutated`] — the boundary, and the line
//!   disappears the moment something may have happened.
//! - **§50.1: a renderer computes nothing.** The risk class, the protection level, the impact
//!   counts and the outstanding acknowledgements are all read off the plan.
//!
//! Every section renders even when it is empty, and says so in words. §10.5's rule that an
//! unknown is never an empty string applies to a block as much as to a cell: a missing `targets`
//! section is indistinguishable from a plan with no targets, and one of those readings is wrong.

use ono_change_core::{
    ChangePlan, PlanAction, RecoveryAsset, RequiredAcknowledgement, VerificationClass,
};

use crate::symbols::{Charset, Symbol};
use crate::{fit, heading, labelled, safe};

/// The line §2.1 and §62.3 both turn on: nothing has happened yet.
///
/// §62.3 names silent protection mutation during planning as a failure mode, and §2.1 makes
/// planning side-effect free. An operator who reads a plan and walks away has to know which of
/// those two worlds they are in, and this line is the whole of the answer.
pub const PLAN_NOT_EXECUTED: &str = "PLAN NOT EXECUTED";

/// The nine questions §20.1 requires a plan to answer, in §20.1's order.
///
/// They are the acceptance criterion for the view: [`Section::ORDER`] answers each of them once,
/// in this sequence, and the crate's tests hold the two lists against each other.
pub const QUESTIONS: [&str; 9] = [
    "What do you intend to change?",
    "Which concrete objects will be touched?",
    "What does Ono expect to happen?",
    "What else may be affected?",
    "What does Ono not know?",
    "What protection will be created?",
    "What remains unrecoverable?",
    "What will Ono verify afterwards?",
    "What special approval is required?",
];

/// One block of the default plan view, in the order §20.1 fixes (§20.2's worked example).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    /// §20.1 question 1: what the operator asked for, in their own words.
    Intent,
    /// §20.1 question 2: the concrete objects the selectors resolved to (§7.1).
    Targets,
    /// §20.1 question 3: the actions, in the order the plan will run them.
    Planned,
    /// §20.1 questions 4 and 5: what else the plan reaches, and where Ono stops seeing (§9).
    Impact,
    /// §20.1 question 6: the coverage matrix and its exclusions (§10.3, Appendix E.8).
    Protection,
    /// §20.1 question 7: the effects nothing can reverse (§2.13, §35.2).
    NotRecoverable,
    /// §19.2's class, which §10.4 keeps on its own axis from protection.
    Risk,
    /// §13.7: whether getting back needs a reboot, which changes what the change costs.
    Reboot,
    /// §20.1 question 8: the contracts that decide whether the plan worked (§23.1).
    Verification,
    /// §20.1 question 9: the acknowledgements §19.4 requires before apply.
    Approval,
}

impl Section {
    /// Every section, in the order §20.1 and §20.2 fix. Reordering this reorders the view.
    pub const ORDER: &'static [Section] = &[
        Section::Intent,
        Section::Targets,
        Section::Planned,
        Section::Impact,
        Section::Protection,
        Section::NotRecoverable,
        Section::Risk,
        Section::Reboot,
        Section::Verification,
        Section::Approval,
    ];

    /// The heading the section carries, spelled as §20.2's example spells it.
    #[must_use]
    pub const fn heading(self) -> &'static str {
        match self {
            Section::Intent => "intent",
            Section::Targets => "targets",
            Section::Planned => "planned",
            Section::Impact => "impact",
            Section::Protection => "protection",
            Section::NotRecoverable => "not recoverable",
            Section::Risk => "risk",
            Section::Reboot => "reboot",
            Section::Verification => "verification",
            Section::Approval => "approval",
        }
    }

    /// The §20.1 questions this section answers, in §20.1's order.
    ///
    /// `impact` answers two of them, because §20.2 puts "what else may be affected" and "what
    /// does Ono not know" in one block: the unknown row is the honest edge of the same graph.
    /// `risk` and `reboot` answer none — §19 and §13.7 put them in the view for their own
    /// reasons, and §20.1's list is a floor rather than a ceiling.
    #[must_use]
    pub const fn questions(self) -> &'static [&'static str] {
        match self {
            Section::Intent => &[QUESTIONS[0]],
            Section::Targets => &[QUESTIONS[1]],
            Section::Planned => &[QUESTIONS[2]],
            Section::Impact => &[QUESTIONS[3], QUESTIONS[4]],
            Section::Protection => &[QUESTIONS[5]],
            Section::NotRecoverable => &[QUESTIONS[6]],
            Section::Risk | Section::Reboot => &[],
            Section::Verification => &[QUESTIONS[7]],
            Section::Approval => &[QUESTIONS[8]],
        }
    }
}

/// The default plan view of §20.2 and §64, laid out at `width` columns.
///
/// `assets` are the recovery assets the plan proposes; they are passed in rather than reached for
/// because §50.1 forbids this crate from asking a provider anything, and §2.1 means a proposed
/// asset does not exist yet in any case.
///
/// Appendix E.1 asks for a calm engineering instrument: the structure carries the meaning, colour
/// is secondary, and nothing here is a warning dialog. The one raised voice is the closing
/// [`PLAN_NOT_EXECUTED`].
#[must_use]
pub fn plan_view(
    plan: &ChangePlan,
    assets: &[RecoveryAsset],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    let mut lines = vec![fit(&title(plan), width)];
    for section in Section::ORDER {
        heading(&mut lines, section.heading());
        lines.extend(body(*section, plan, assets, width, charset));
    }
    // Appendix F: everything before the first mutation is told as "nothing happened", and
    // everything at or after it is not. The predicate is the plan's, never this view's guess.
    if !plan.state().has_mutated() {
        lines.push(String::new());
        lines.push(PLAN_NOT_EXECUTED.to_owned());
    }
    lines
}

/// `PLAN / a82f  rev 3  sealed` — the identity §36.4 lets an operator type back (§20.2).
fn title(plan: &ChangePlan) -> String {
    format!(
        "PLAN / {}  rev {}  {}",
        plan.id().short(),
        plan.revision(),
        plan.state().as_str()
    )
}

/// The body of one section, always non-empty (§10.5: silence is not an answer).
fn body(
    section: Section,
    plan: &ChangePlan,
    assets: &[RecoveryAsset],
    width: usize,
    charset: Charset,
) -> Vec<String> {
    match section {
        Section::Intent => vec![fit(&format!("  {}", safe(plan.intent().text())), width)],
        Section::Targets => or_else(
            plan.targets()
                .iter()
                .map(|target| fit(&format!("  {}", safe(target.label())), width))
                .collect(),
            "no selector has been resolved to an object yet",
            width,
        ),
        Section::Planned => or_else(
            plan.actions()
                .iter()
                .map(|action| fit(&action_line(action, charset), width))
                .collect(),
            "the plan carries no action",
            width,
        ),
        Section::Impact => crate::impact::impact_rows(plan.impact(), width, charset),
        Section::Protection => {
            crate::protection::protection_block(plan.protection(), assets, width, charset)
        }
        Section::NotRecoverable => or_else(
            unrecoverable(plan, width, charset),
            "nothing was recorded as irreversible",
            width,
        ),
        Section::Risk => risk_lines(plan, width),
        Section::Reboot => vec![fit(&format!("  {}", reboot(plan, assets)), width)],
        Section::Verification => or_else(
            plan.verification()
                .contracts()
                .iter()
                .map(|contract| {
                    let mut line = format!(
                        "  {} {}",
                        safe(contract.subject()),
                        safe(contract.expression())
                    );
                    if contract.class() != VerificationClass::Required {
                        line.push_str(&format!("  ({})", contract.class().as_str()));
                    }
                    fit(&line, width)
                })
                .collect(),
            "no verification contract is attached",
            width,
        ),
        Section::Approval => or_else(
            plan.outstanding_acknowledgements()
                .into_iter()
                .map(|acknowledgement| fit(&approval_line(acknowledgement), width))
                .collect(),
            "none required",
            width,
        ),
    }
}

/// `  1  ~ replace nginx.conf` — the ordinal, §20.3's mark, and what the action does (§20.2).
///
/// The mark is the one §20.3 gives the action's first declared effect. An action that declares no
/// effect gets no mark rather than a guessed one: §1.3 forbids inventing the future, and a `~`
/// beside an action nobody described would be exactly that.
fn action_line(action: &PlanAction, charset: Charset) -> String {
    let mark = action.effects().first().map_or(" ", |effect| {
        Symbol::for_effect(effect.kind()).glyph(charset)
    });
    let mut line = format!(
        "  {:>2}  {mark} {}",
        action.ordinal(),
        safe(action.summary())
    );
    if let Some(target) = action.target() {
        line.push_str(&format!("  {}", safe(target)));
    }
    line
}

/// What §20.1's question 7 answers: the effects nothing brings back (§2.13, §35.2).
///
/// Two sources say it and both are read rather than derived — an exclusion the protection matrix
/// marked irreversible, and an effect the provider declared irreversible. A subject named by both
/// appears once, because a reader counting the list would otherwise read one loss as two.
fn unrecoverable(plan: &ChangePlan, width: usize, charset: Charset) -> Vec<String> {
    let mark = Symbol::Risk.glyph(charset);
    let mut subjects: Vec<String> = Vec::new();
    for exclusion in plan.protection().exclusions() {
        if exclusion.is_irreversible() {
            subjects.push(safe(exclusion.subject()));
        }
    }
    for effect in plan.effects() {
        if effect.is_irreversible() {
            subjects.push(
                effect
                    .object()
                    .map_or_else(|| safe(effect.explanation()), safe),
            );
        }
    }
    subjects.dedup();
    let mut seen: Vec<String> = Vec::new();
    for subject in subjects {
        if !seen.contains(&subject) {
            seen.push(subject);
        }
    }
    seen.into_iter()
        .map(|subject| fit(&format!("  {mark} {subject}"), width))
        .collect()
}

/// §19.2's class and the findings that produced it, which §40.2 shows instead of "Are you sure?".
fn risk_lines(plan: &ChangePlan, width: usize) -> Vec<String> {
    let assessment = plan.risk();
    let mut lines = vec![fit(
        &format!("  {}", assessment.classify().as_str().to_uppercase()),
        width,
    )];
    for finding in assessment.leading() {
        lines.push(fit(
            &format!(
                "    {} - {}",
                finding.dimension().as_str(),
                safe(finding.reason())
            ),
            width,
        ));
    }
    lines
}

/// Whether getting the system back needs a reboot (§13.7, §14.6, §19.1).
///
/// Two independent facts say yes and either is enough: a rule raised the reboot dimension, or an
/// asset's own cost says restoring from it needs one. §10.4 is why the line exists at all — a
/// strongly protected plan whose way back is a reboot is not a cheap plan.
fn reboot(plan: &ChangePlan, assets: &[RecoveryAsset]) -> &'static str {
    let by_rule =
        plan.risk().findings().iter().any(|finding| {
            finding.dimension() == ono_change_core::RiskDimension::RebootRequirement
        });
    let by_asset = assets.iter().any(|asset| asset.cost().requires_reboot());
    if by_rule || by_asset { "yes" } else { "no" }
}

/// `  --accept-risk        high risk` — the flag §40.3 takes and what it acknowledges.
fn approval_line(acknowledgement: RequiredAcknowledgement) -> String {
    labelled(acknowledgement.flag(), &acknowledgement.to_string(), 24)
}

/// `lines`, or one line saying in words that there were none (§10.5).
fn or_else(lines: Vec<String>, empty: &str, width: usize) -> Vec<String> {
    if lines.is_empty() {
        return vec![fit(&format!("  {empty}"), width)];
    }
    lines
}
