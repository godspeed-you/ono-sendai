//! Where a v0.6 record becomes characters (spec v0.6 §20, §39.3, Appendix E; v0.2 §13.1).
//!
//! v0.2 §13.1 keeps presentation out of the language and out of providers: a pipeline carries
//! typed values, and `crate::sink` is the one place they become lines. `ono-change-render` is the
//! renderer that knows the v0.6 schemas, and this module is the two-way join between them — which
//! schema draws which view, and which sections `inspect plan` asked for.
//!
//! **Machine-readable output keeps working whatever is drawn.** Every change command answers with
//! a typed value first and a rendering second, so `plan … | to json`, `get plan | where state ==
//! "failed"` and `impact a82f | to json` see records rather than the lines below.

use std::sync::{Arc, OnceLock, RwLock};

use ono_change_render::Charset;
use ono_command::BoundArguments;
use ono_value::{RecordValue, Value};

/// Which sections of a plan `inspect plan` asked to expand (§5.5, §9.5, §10.3, §11.1, §23.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sections {
    /// `--protection`: the coverage matrix, one row per mutation domain (§10.3).
    pub protection: bool,
    /// `--impact`: the graph rather than its summary (§9.5).
    pub impact: bool,
    /// `--actions`: every action with its execution, preconditions and effects (§46.2).
    pub actions: bool,
    /// `--resolution`: Appendix B.10's persistence resolution per target.
    pub resolution: bool,
    /// `--verification`: the contracts the plan will check (§23.1).
    pub verification: bool,
    /// `--recovery`: the assets the plan proposes or created (§11.1).
    pub recovery: bool,
}

impl Sections {
    /// The sections `arguments` asked for.
    #[must_use]
    pub fn of(arguments: &BoundArguments) -> Self {
        Self {
            protection: arguments.flag("protection"),
            impact: arguments.flag("impact"),
            actions: arguments.flag("actions"),
            resolution: arguments.flag("resolution"),
            verification: arguments.flag("verification"),
            recovery: arguments.flag("recovery"),
        }
    }

    /// Whether any section was named, which is what makes the default view the default.
    #[must_use]
    pub const fn any(self) -> bool {
        self.protection
            || self.impact
            || self.actions
            || self.resolution
            || self.verification
            || self.recovery
    }
}

/// The sections the last `inspect plan` in this process asked for (§5.5).
///
/// §46.1's record is the value and the sections are a statement *about how to draw it*, so they
/// travel beside the stream rather than inside it — the same arrangement `timeline` uses for its
/// window (ADR-0778). One slot per process, written by `inspect plan` and read on the same turn.
fn published() -> &'static RwLock<Sections> {
    static SECTIONS: OnceLock<RwLock<Sections>> = OnceLock::new();
    SECTIONS.get_or_init(|| RwLock::new(Sections::default()))
}

/// Records which sections the plan about to be drawn should expand.
pub fn publish_sections(sections: Sections) {
    if let Ok(mut held) = published().write() {
        *held = sections;
    }
}

/// The sections to expand, or none where the lock is poisoned.
///
/// None is the default view, which answers all nine of §20.1's questions: degrading toward *more*
/// of the plan is the safe direction for a display whose reader is about to decide something.
#[must_use]
pub fn sections() -> Sections {
    published()
        .read()
        .map_or_else(|_| Sections::default(), |held| *held)
}

/// The `ono.change-plan/1` view, expanded as the last `inspect plan` asked (§20.2, §5.5).
///
/// `assets` are the `ono.recovery-asset/1` records the plan proposes; §2.1 means a proposed asset
/// does not exist yet, so they are passed in rather than reached for.
#[must_use]
pub fn plan_lines(plan: &RecordValue, assets: &[RecordValue], width: usize) -> Vec<String> {
    let charset = charset();
    let asked = sections();
    if !asked.any() {
        let plan = with_outstanding_acknowledgements(plan);
        return ono_change_render::plan_view(&plan, assets, width, charset);
    }
    let mut lines = Vec::new();
    if asked.actions {
        lines.extend(ono_change_render::collapsed_plan(plan, width, charset));
    }
    if asked.impact {
        blank(&mut lines);
        lines.extend(ono_change_render::impact_block(plan, width, charset));
    }
    if asked.protection {
        blank(&mut lines);
        lines.extend(ono_change_render::coverage_matrix(plan, width, charset));
    }
    if asked.recovery {
        blank(&mut lines);
        lines.extend(ono_change_render::protection_block(
            plan, assets, width, charset,
        ));
    }
    if asked.verification {
        blank(&mut lines);
        lines.extend(ono_change_render::verification_view(plan, &[], width));
    }
    if asked.resolution {
        blank(&mut lines);
        lines.extend(resolution_lines(plan, width));
    }
    lines
}

/// The plan record with the gates `apply` would raise attached (§19.4, §40.2).
///
/// Which flag answers a gate is the shell's policy — §40.2's worked gate is answered by
/// `--accept-service-outage` — so the view is handed the gates `crate::change::gates` computes
/// rather than working them out again from the record. A record that does not decode as a plan
/// is drawn as it is.
fn with_outstanding_acknowledgements(plan: &RecordValue) -> RecordValue {
    let Ok(decoded) = ono_change_core::value::plan_from_record(plan) else {
        return plan.clone();
    };
    let gates = super::gates::outstanding(&decoded, super::gates::Acknowledgements::default());
    let rows = gates.iter().map(|gate| {
        let mut row = ono_value::MapValue::new();
        row.insert("flag".into(), Value::string(gate.flag));
        row.insert("reason".into(), Value::string(&gate.reasons.join("; ")));
        Value::Map(Arc::new(row))
    });
    super::extended(
        plan,
        ono_change_render::OUTSTANDING_ACKNOWLEDGEMENTS,
        Value::list(rows),
    )
}

/// Appendix B.10's expansion, drawn from the namespaced extension `inspect plan` attached.
///
/// It is drawn here rather than in `ono-change-render` because the resolution is not a field of
/// §46.1's schema: it is what the shell's mount table answered, and a renderer that reads a
/// provider extension is reading something the contract does not promise (v0.2 §10.4).
fn resolution_lines(plan: &RecordValue, width: usize) -> Vec<String> {
    let mut lines = vec![fit("PERSISTENCE RESOLUTION", width)];
    let Some(ono_value::Value::List(rows)) = plan.extra().get("ono.change/resolution") else {
        lines.push(fit(
            "  no target of this plan resolves to a path, so there is nothing to resolve",
            width,
        ));
        return lines;
    };
    for row in rows.iter() {
        let Ok(map) = row.as_map() else {
            continue;
        };
        let field = |name: &str| -> String {
            map.get(name)
                .and_then(|value| value.as_str().ok())
                .unwrap_or("unknown")
                .to_owned()
        };
        lines.push(String::new());
        lines.push(fit(&format!("  {}", field("path")), width));
        lines.push(fit(
            &format!(
                "    mount {} ({}) on {}",
                field("mount_point"),
                field("filesystem"),
                field("source")
            ),
            width,
        ));
        lines.push(fit(
            &format!(
                "    filesystem root {}, object {} ({})",
                field("filesystem_root"),
                field("object"),
                field("object_kind")
            ),
            width,
        ));
        lines.push(fit(
            &format!("    recovery boundary {}", field("recovery_boundary")),
            width,
        ));
    }
    lines
}

/// A blank separator, unless the block is still empty.
fn blank(lines: &mut Vec<String>) {
    if !lines.is_empty() {
        lines.push(String::new());
    }
}

/// One line, cut to `width` so a narrow terminal wraps nothing.
fn fit(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    text.chars().take(width).collect()
}

/// Whether the terminal can be promised box-drawing characters (§20.3, Appendix E.7).
///
/// The same question `crate::sink::map_charset` asks about a map, answered from the same
/// environment: §20.3 requires an ASCII fallback to exist, and this is when it is taken.
#[must_use]
pub fn charset() -> Charset {
    match crate::sink::map_charset() {
        ono_spatial_render::Charset::Unicode => Charset::Unicode,
        ono_spatial_render::Charset::Ascii => Charset::Ascii,
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use ono_change_core::{
        ChangePlan, Intent, RiskAssessment, RiskClass, RiskDimension, RiskFinding,
    };

    use super::*;

    fn drawn(risk: RiskAssessment) -> Vec<String> {
        let plan = ChangePlan::draft(
            Intent::new("restart nginx", "plan restart service nginx.service"),
            "session-1",
            jiff::Timestamp::UNIX_EPOCH,
        )
        .with_risk(risk);
        let record = ono_change_core::value::plan_record(&plan).expect("the contract is in build");
        plan_lines(&record, &[], 120)
    }

    fn approval(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .skip_while(|line| line.trim() != "approval")
            .skip(1)
            .take_while(|line| !line.trim().is_empty())
            .cloned()
            .collect()
    }

    #[test]
    fn should_ask_for_the_service_outage_flag_when_only_downtime_makes_the_plan_high() {
        let lines = drawn(RiskAssessment::of(vec![RiskFinding::new(
            RiskDimension::Downtime,
            RiskClass::High,
            "risk.downtime.service-restart",
            "restarting `nginx.service` interrupts what it was serving",
        )]));
        let asked = approval(&lines).join("\n");
        assert!(
            asked.contains("--accept-service-outage"),
            "v0.6 §40.2: the view names the flag `apply` will demand. Got {lines:?}"
        );
        assert!(
            !asked.contains("--accept-risk"),
            "and not a flag `apply` would not ask for. Got {lines:?}"
        );
        assert!(
            asked.contains("interrupts what it was serving"),
            "§40.2: with the rule's own reason. Got {lines:?}"
        );
    }

    #[test]
    fn should_ask_for_nothing_when_no_gate_is_raised() {
        let lines = drawn(RiskAssessment::of(vec![RiskFinding::new(
            RiskDimension::Scope,
            RiskClass::Moderate,
            "risk.scope.single-object",
            "the plan changes one object",
        )]));
        assert_eq!(
            approval(&lines),
            vec!["  none required".to_owned()],
            "§40.1: a moderate plan applies on `apply` alone. Got {lines:?}"
        );
    }
}
