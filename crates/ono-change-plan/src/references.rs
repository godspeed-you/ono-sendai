//! Plan and recovery-asset references (spec v0.6 §36.4, §37.5).
//!
//! §36.4 fixes three forms with three examples:
//!
//! ```text
//! get plan a82f
//! apply plan/a82f
//! recover plan/a82f
//! ```
//!
//! A bare identity, a prefix of one, and the same thing written with its marker. The marker is
//! what tells a plan reference from a recovery reference, so `recovery/9f21` is parsed here too:
//! §37.5's `get recovery` names assets in exactly the same shape.
//!
//! Anything that is not lowercase hexadecimal is **refused rather than searched for**. A store
//! scan over `../etc/passwd` would answer "not found" eventually, and the difference between
//! "no plan matches that identity" and "that is not an identity" is the difference between a
//! typo and a probe. [`ono_change_core::PlanId::parse`] draws the line; what is owned here is
//! reading the marker and turning a miss into the right refusal.

use std::sync::Arc;

use ono_change_core::{PlanId, RecoveryAssetId, SHORT, error, shortest_unique_prefixes};
use ono_value::ErrorValue;

/// What kind of thing a reference names (§36.4, §37.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceKind {
    /// `plan/a82f` — a change plan or recovery plan (§36.4).
    Plan,
    /// `recovery/9f21` — a recovery asset (§37.5).
    Recovery,
    /// `a82f` — a bare identity, whose kind is decided by the command that resolves it (§36.4).
    Unqualified,
}

/// A parsed reference: a kind and a hexadecimal body (§36.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    kind: ReferenceKind,
    body: Arc<str>,
}

impl Reference {
    /// Reads `text` as a reference (§36.4).
    ///
    /// # Errors
    ///
    /// Returns `change.plan_not_found` for anything that is not an identity, and
    /// `recovery.asset_not_found` where the text carried the recovery marker: the refusal names
    /// the space the caller was writing in, so a mistyped asset reference does not read as a
    /// missing plan.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let trimmed = text.trim();
        let (kind, body) = if let Some(body) = trimmed.strip_prefix(PlanId::PREFIX) {
            (ReferenceKind::Plan, body)
        } else if let Some(body) = trimmed.strip_prefix(RecoveryAssetId::PREFIX) {
            (ReferenceKind::Recovery, body)
        } else {
            (ReferenceKind::Unqualified, trimmed)
        };
        let parsed = match kind {
            ReferenceKind::Recovery => RecoveryAssetId::parse(body).is_some(),
            _ => PlanId::parse(body).is_some(),
        };
        if !parsed {
            return Err(not_an_identity(kind, text));
        }
        Ok(Self {
            kind,
            body: Arc::from(body),
        })
    }

    /// What the reference names.
    #[must_use]
    pub const fn kind(&self) -> ReferenceKind {
        self.kind
    }

    /// The hexadecimal body, without its marker.
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Whether this reference may name a plan (§36.4).
    ///
    /// A recovery reference may not: `apply recovery/9f21` names an asset, and resolving it
    /// against the plan table would either miss or, worse, hit.
    #[must_use]
    pub const fn names_a_plan(&self) -> bool {
        matches!(self.kind, ReferenceKind::Plan | ReferenceKind::Unqualified)
    }

    /// Whether this reference may name a recovery asset (§37.5).
    #[must_use]
    pub const fn names_an_asset(&self) -> bool {
        matches!(
            self.kind,
            ReferenceKind::Recovery | ReferenceKind::Unqualified
        )
    }

    /// The reference as it was written, with its marker where it had one.
    #[must_use]
    pub fn render(&self) -> String {
        match self.kind {
            ReferenceKind::Plan => format!("{}{}", PlanId::PREFIX, self.body),
            ReferenceKind::Recovery => format!("{}{}", RecoveryAssetId::PREFIX, self.body),
            ReferenceKind::Unqualified => self.body.to_string(),
        }
    }
}

impl std::fmt::Display for Reference {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.render())
    }
}

/// The width a printed plan reference needs so that it still resolves to one plan (§36.4).
///
/// §36.4 asks for references "stable enough" to type back. A reference that was unambiguous when
/// it was printed and ambiguous when it was typed is not stable, so the width is computed against
/// everything the store holds and widened exactly as far as a collision requires — never further,
/// because the shortest form is the one a person can retype.
#[must_use]
pub fn plan_reference_width(identities: &[&str]) -> usize {
    shortest_unique_prefixes(identities, SHORT)
}

/// One plan reference, rendered at `width` with §36.4's `plan/` marker.
#[must_use]
pub fn render_plan(plan: &PlanId, width: usize) -> String {
    format!("{}{}", PlanId::PREFIX, truncate(plan.as_str(), width))
}

/// One recovery-asset reference, rendered at `width` with §37.5's `recovery/` marker.
#[must_use]
pub fn render_asset(asset: &RecoveryAssetId, width: usize) -> String {
    format!("{}{}", RecoveryAssetId::PREFIX, truncate(asset.as_str(), width))
}

/// The first `width` characters of `identity`, or all of it when it is shorter.
fn truncate(identity: &str, width: usize) -> &str {
    identity.get(..width.min(identity.len())).unwrap_or(identity)
}

/// The refusal for text that is not an identity at all.
fn not_an_identity(kind: ReferenceKind, text: &str) -> ErrorValue {
    match kind {
        ReferenceKind::Recovery => error::asset_not_found(text),
        _ => error::plan_not_found(text),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use ono_core::ErrorCode;

    use super::*;

    #[test]
    fn should_read_a_bare_identity_as_a_reference_whose_kind_the_command_decides() {
        let reference = Reference::parse("a82f").expect("§36.4: `get plan a82f` is a reference");
        assert_eq!(reference.kind(), ReferenceKind::Unqualified);
        assert_eq!(reference.body(), "a82f");
        assert!(reference.names_a_plan());
        assert!(reference.names_an_asset());
    }

    #[test]
    fn should_read_a_marked_plan_reference() {
        let reference =
            Reference::parse("plan/a82f").expect("§36.4: `apply plan/a82f` is a reference");
        assert_eq!(reference.kind(), ReferenceKind::Plan);
        assert_eq!(reference.body(), "a82f");
        assert!(reference.names_a_plan());
        assert!(
            !reference.names_an_asset(),
            "§37.5: `plan/a82f` names a plan, and resolving it against the asset table would be a \
             different object"
        );
    }

    #[test]
    fn should_read_a_marked_recovery_reference() {
        let reference =
            Reference::parse("recovery/9f21").expect("§37.5: `get recovery 9f21` is a reference");
        assert_eq!(reference.kind(), ReferenceKind::Recovery);
        assert_eq!(reference.body(), "9f21");
        assert!(reference.names_an_asset());
        assert!(!reference.names_a_plan());
    }

    #[test]
    fn should_refuse_something_that_is_not_hexadecimal_rather_than_search_for_it() {
        for text in ["../etc/passwd", "ZZZZ", "plan/", "a b", "", "0x1f"] {
            let refusal = Reference::parse(text)
                .err()
                .unwrap_or_else(|| panic!("`{text}` is not an identity and must be refused"));
            assert_eq!(
                refusal.code(),
                ErrorCode::ChangePlanNotFound,
                "§36.4: a reference is an identity or a prefix of one, and `{text}` is neither"
            );
        }
    }

    #[test]
    fn should_refuse_a_malformed_recovery_reference_in_the_recovery_space() {
        let refusal = Reference::parse("recovery/zzzz")
            .expect_err("§37.5: a recovery reference is hexadecimal too");
        assert_eq!(
            refusal.code(),
            ErrorCode::RecoveryAssetNotFound,
            "§37.5: the refusal names the space the caller was writing in"
        );
    }

    #[test]
    fn should_refuse_a_reference_longer_than_an_identity() {
        let refusal = Reference::parse("00112233445566778")
            .expect_err("an identity is sixteen characters wide");
        assert_eq!(refusal.code(), ErrorCode::ChangePlanNotFound);
    }

    #[test]
    fn should_render_a_reference_back_the_way_it_was_written() {
        for text in ["a82f", "plan/a82f", "recovery/9f21"] {
            let reference = Reference::parse(text).expect("a valid reference parses");
            assert_eq!(
                reference.render(),
                text,
                "§36.4: a printed reference is the one an operator types back"
            );
        }
    }

    #[test]
    fn should_ignore_surrounding_whitespace_a_shell_left_behind() {
        let reference = Reference::parse("  plan/a82f  ").expect("a valid reference parses");
        assert_eq!(reference.body(), "a82f");
    }

    #[test]
    fn should_keep_a_reference_four_characters_wide_when_nothing_collides() {
        assert_eq!(
            plan_reference_width(&["a82f0000", "91cc0000"]),
            SHORT,
            "§36.4's own examples are four characters, and nothing widens them for free"
        );
    }

    #[test]
    fn should_widen_a_printed_reference_only_as_far_as_a_collision_requires() {
        assert_eq!(
            plan_reference_width(&["a82f0000", "a82f1111"]),
            5,
            "§36.4: a printed reference must stay unambiguous when it is typed back"
        );
    }

    #[test]
    fn should_render_a_plan_reference_with_its_marker_at_the_width_the_store_needs() {
        let plan = PlanId::derive(&["one"]);
        let rendered = render_plan(&plan, 5);
        assert!(rendered.starts_with("plan/"), "got {rendered}");
        assert_eq!(rendered.len(), "plan/".len() + 5);
        assert!(
            plan.matches(&rendered),
            "§36.4: what was printed must resolve back to the plan it named"
        );
    }

    #[test]
    fn should_render_an_asset_reference_with_its_marker() {
        let asset = RecoveryAssetId::derive(&["one"]);
        let rendered = render_asset(&asset, SHORT);
        assert!(rendered.starts_with("recovery/"), "got {rendered}");
        assert!(asset.matches(&rendered));
    }

    #[test]
    fn should_render_a_whole_identity_when_the_width_exceeds_it() {
        let plan = PlanId::derive(&["one"]);
        assert_eq!(
            render_plan(&plan, 64),
            format!("plan/{}", plan.as_str()),
            "a width wider than the identity renders the identity, not padding"
        );
    }
}
