//! The collapsed default for long plans (Appendix E.2).
//!
//! Appendix E.2 is a scaling rule: eighty-three lines of individual actions is not a view, it is
//! a log. So identical actions collapse to one line with a count, the lifecycle phases stay as
//! headings, and the footer carries the three facts that decide whether to read further — risk,
//! protection and strategy.
//!
//! [`action_groups`] is the collapsed model itself rather than a private step of the drawing.
//! Appendix E.2 says the lines are expandable and Appendix E.3 gives `Space` to expand them, so
//! the inspector needs the group *and* the action identities inside it; a renderer that folded
//! them into text would make expansion a second, disagreeing traversal.
//!
//! Appendix E.8 reaches the footer too. The `protection` line is a coverage summary, and a
//! coverage summary shows its exclusions — so the footer's protection entry is two lines, not
//! one, and it comes from the same `pub(crate)` function the full plan view uses.

use ono_value::RecordValue;

use crate::progress::Phase;
use crate::symbols::Charset;
use crate::{column_pair, counted, display_width, fit, heading, items, list_len, text};

/// How wide the footer's label column is (Appendix E.2's alignment).
const LABEL: usize = 12;

/// Identical actions of one phase, collapsed to a count and expandable back (Appendix E.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionGroup {
    phase: Phase,
    summary: String,
    actions: Vec<String>,
    status: Option<String>,
}

impl ActionGroup {
    /// The lifecycle phase the group's actions belong to (§4.5, §4.7, §4.8).
    #[must_use]
    pub const fn phase(&self) -> Phase {
        self.phase
    }

    /// What every action in the group does, which is what makes them one group.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// How many actions the group stands for.
    #[must_use]
    pub fn count(&self) -> usize {
        self.actions.len()
    }

    /// The identities of the actions themselves, so Appendix E.3's `Space` can expand the line.
    #[must_use]
    pub fn actions(&self) -> &[String] {
        &self.actions
    }

    /// The state word the group carries, where its actions agree on one.
    ///
    /// Appendix E.2 shows `ready-to-create` beside a group of proposed recovery assets: §2.1
    /// makes that the truthful word for protection a plan has described and not created. A group
    /// whose actions disagree carries a settled count instead, because one word for a mixed group
    /// would be wrong for some of it.
    #[must_use]
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }
}

/// The plan's actions, collapsed as Appendix E.2 collapses them.
///
/// Grouping is by phase and by summary, in the order the record holds them, so the view never
/// reorders what the planner decided. Two actions with the same summary in different phases stay
/// apart: `restart service` as a mutation and `restart service` as a recovery step are not the
/// same line.
#[must_use]
pub fn action_groups(plan: &RecordValue) -> Vec<ActionGroup> {
    let mut groups: Vec<ActionGroup> = Vec::new();
    let mut statuses: Vec<Vec<String>> = Vec::new();
    for action in items(plan, "actions") {
        let phase = Phase::of(text(&action, "role").as_deref().unwrap_or("mutate"));
        let summary = text(&action, "summary").unwrap_or_else(|| "unnamed action".to_owned());
        let id = text(&action, "id").unwrap_or_default();
        let status = text(&action, "status").unwrap_or_else(|| "pending".to_owned());
        match groups
            .iter()
            .position(|group| group.phase == phase && group.summary == summary)
        {
            Some(index) => {
                if let Some(group) = groups.get_mut(index) {
                    group.actions.push(id);
                }
                if let Some(seen) = statuses.get_mut(index) {
                    seen.push(status);
                }
            }
            None => {
                groups.push(ActionGroup {
                    phase,
                    summary,
                    actions: vec![id],
                    status: None,
                });
                statuses.push(vec![status]);
            }
        }
    }
    for (group, seen) in groups.iter_mut().zip(statuses) {
        group.status = group_status(group.phase, &seen);
    }
    groups
}

/// Appendix E.2's collapsed plan, laid out at `width` columns.
///
/// The VERIFY block falls back to §23's contracts when the plan expresses verification as
/// contracts rather than as verify actions, because a plan that verifies must show that it does
/// (§23.1) and an empty heading would say the opposite.
#[must_use]
pub fn collapsed_plan(plan: &RecordValue, width: usize, charset: Charset) -> Vec<String> {
    let groups = action_groups(plan);
    let mut lines = vec![fit(
        &format!(
            "PLAN {} / {}",
            crate::plan::short(plan, "id"),
            counted(list_len(plan, "actions"), "action", "actions")
        ),
        width,
    )];
    for phase in Phase::ORDER {
        let of_phase: Vec<&ActionGroup> = groups
            .iter()
            .filter(|group| group.phase == *phase)
            .collect();
        if of_phase.is_empty() {
            let contracts = list_len(plan, "verification_contracts");
            if *phase == Phase::Verify && contracts > 0 {
                heading(&mut lines, phase.heading());
                lines.push(fit(
                    &format!(
                        "  {}",
                        counted(contracts, "verification contract", "verification contracts")
                    ),
                    width,
                ));
            }
            continue;
        }
        heading(&mut lines, phase.heading());
        for group in of_phase {
            lines.push(group_line(group, width));
        }
    }

    lines.push(String::new());
    lines.push(fit(
        &column_pair(
            "risk",
            &text(plan, "risk")
                .unwrap_or_else(|| "unknown".to_owned())
                .to_uppercase(),
            LABEL,
        ),
        width,
    ));
    lines.extend(crate::protection::coverage_summary(
        plan, LABEL, width, charset,
    ));
    lines.push(fit(
        &column_pair(
            "strategy",
            &text(plan, "strategy").unwrap_or_else(|| "unknown".to_owned()),
            LABEL,
        ),
        width,
    ));
    lines
}

/// `  20 update config                    ready-to-create` (Appendix E.2).
///
/// The state word is right-aligned to the layout width, which is why the width is a parameter
/// rather than a constant: the same group reads the same way at eighty columns and at two
/// hundred, and neither is a snapshot anything depends on.
fn group_line(group: &ActionGroup, width: usize) -> String {
    let head = format!("  {} {}", group.count(), group.summary);
    let Some(status) = group.status() else {
        return fit(&head, width);
    };
    let width = width.max(crate::MIN_WIDTH);
    let room = width
        .saturating_sub(display_width(&head))
        .saturating_sub(display_width(status));
    if room < 2 {
        return fit(&format!("{head}  {status}"), width);
    }
    fit(&format!("{head}{}{status}", " ".repeat(room)), width)
}

/// The word a group of actions in one phase carries, where they agree (Appendix E.2).
fn group_status(phase: Phase, statuses: &[String]) -> Option<String> {
    let first = statuses.first()?;
    if statuses.iter().all(|status| status == first) {
        return match (phase, first.as_str()) {
            // §2.1: a prepare action nobody has run has *described* a recovery asset. Appendix
            // E.2 spells that state `ready-to-create`, which says both halves of it.
            (Phase::Prepare, "pending") => Some("ready-to-create".to_owned()),
            (_, "pending") => None,
            (_, status) => Some(status.to_owned()),
        };
    }
    let settled = statuses
        .iter()
        .filter(|status| crate::progress::is_settled(status))
        .count();
    Some(format!("{settled}/{}", statuses.len()))
}
