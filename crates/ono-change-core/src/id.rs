//! The identities of the prospective-change model (spec v0.6 §3.2, §36.4).
//!
//! Every identity here is a truncated SHA-256 over the facts that make the thing what it is,
//! rendered as lowercase hexadecimal. Two consequences follow, and both are deliberate:
//!
//! - **A plan built twice from the same intent, session and instant is the same plan.** That is
//!   what makes §55.1's "planning is side-effect free" testable without a clock fixture per run.
//! - **A reference is a prefix.** §36.4 requires `get plan a82f` to work, and the spec's own
//!   examples are four characters long. [`PlanId::short`] is that prefix, and
//!   [`shortest_unique_prefixes`] widens it exactly as far as the store requires, the way
//!   `ono-temporal-ledger` does for event references (ADR-0783).

use std::sync::Arc;

use sha2::{Digest, Sha256};

/// The hexadecimal width of a full identity.
const FULL: usize = 16;

/// The hexadecimal width a rendered reference uses when nothing collides.
pub const SHORT: usize = 4;

/// Digests `parts` into `width` hexadecimal characters, domain-separated by `tag`.
fn digest(tag: &str, parts: &[&str], width: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tag.as_bytes());
    hasher.update([0x1f]);
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    let bytes = hasher.finalize();
    let mut text = String::with_capacity(width);
    for byte in bytes.iter().take(width.div_ceil(2)) {
        use std::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
    }
    text.truncate(width);
    text
}

/// Whether `text` is a plausible identity body: lowercase hexadecimal, non-empty, bounded.
fn is_hex(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= FULL
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

macro_rules! hex_identity {
    ($name:ident, $tag:literal, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        ///
        /// The body is lowercase hexadecimal. Equality is over the whole body, so a short
        /// reference is resolved against a store rather than compared directly.
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(Arc<str>);

        impl $name {
            /// The prefix a rendered reference of this kind carries.
            pub const PREFIX: &'static str = $prefix;

            /// Derives an identity from the facts that make this thing what it is.
            #[must_use]
            pub fn derive(parts: &[&str]) -> Self {
                Self(Arc::from(digest($tag, parts, FULL).as_str()))
            }

            /// Reads an identity back, with or without its rendered prefix.
            ///
            /// Returns `None` for anything that is not lowercase hexadecimal of a plausible
            /// width, so a reference typed at a prompt is refused rather than searched for.
            #[must_use]
            pub fn parse(text: &str) -> Option<Self> {
                let body = text.strip_prefix($prefix).unwrap_or(text);
                is_hex(body).then(|| Self(Arc::from(body)))
            }

            /// The full hexadecimal body.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// The short reference §36.4's examples use, or the whole body when it is shorter.
            #[must_use]
            pub fn short(&self) -> &str {
                let width = SHORT.min(self.0.len());
                self.0.get(..width).unwrap_or(&self.0)
            }

            /// Whether `reference` is a prefix of this identity, with or without the marker.
            #[must_use]
            pub fn matches(&self, reference: &str) -> bool {
                let body = reference.strip_prefix($prefix).unwrap_or(reference);
                !body.is_empty() && self.0.starts_with(body)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "{}{}", $prefix, self.0)
            }
        }
    };
}

hex_identity!(
    PlanId,
    "ono.change-plan",
    "plan/",
    "The stable identity of a [`crate::ChangePlan`] (§3.2)."
);
hex_identity!(
    ActionId,
    "ono.plan-action",
    "action/",
    "The identity of one [`crate::PlanAction`] inside a plan (§3.3)."
);
hex_identity!(
    EffectId,
    "ono.proposed-effect",
    "effect/",
    "The identity of one [`crate::ProposedEffect`] (§8.2)."
);
hex_identity!(
    RecoveryAssetId,
    "recovery/",
    "recovery/",
    "The identity of one [`crate::RecoveryAsset`] (§11.1)."
);
hex_identity!(
    CheckId,
    "ono.verification-check",
    "check/",
    "The identity of one [`crate::VerificationContract`] (§23.3)."
);

impl PlanId {
    /// The identity a plan created from `intent` in `session` at `requested_at` carries.
    #[must_use]
    pub fn of(session: &str, requested_at: &str, intent: &str) -> Self {
        Self::derive(&[session, requested_at, intent])
    }
}

impl ActionId {
    /// The identity of the `ordinal`-th action of `plan`, described by `description`.
    #[must_use]
    pub fn of(plan: &PlanId, ordinal: usize, description: &str) -> Self {
        Self::derive(&[plan.as_str(), &ordinal.to_string(), description])
    }
}

impl EffectId {
    /// The identity of an effect of `action` in `domain` over `object`.
    #[must_use]
    pub fn of(action: &ActionId, domain: &str, object: &str) -> Self {
        Self::derive(&[action.as_str(), domain, object])
    }
}

impl RecoveryAssetId {
    /// The identity of an asset `provider` created for `plan` over `scope` at `created_at`.
    #[must_use]
    pub fn of(provider: &str, plan: Option<&PlanId>, scope: &str, created_at: &str) -> Self {
        Self::derive(&[provider, plan.map_or("", PlanId::as_str), scope, created_at])
    }
}

impl CheckId {
    /// The identity of the check `expression` against `subject` in `plan`.
    #[must_use]
    pub fn of(plan: &PlanId, subject: &str, expression: &str) -> Self {
        Self::derive(&[plan.as_str(), subject, expression])
    }
}

/// The shortest prefix width that tells each of `identities` apart, never below `minimum`.
///
/// §36.4 asks for references that stay stable enough to type. A reference is therefore as short
/// as the store allows and no shorter: two plans that share four characters both render five,
/// so a printed reference never resolves to something the reader did not mean (ADR-0783's rule,
/// applied to plans).
#[must_use]
pub fn shortest_unique_prefixes(identities: &[&str], minimum: usize) -> usize {
    let mut width = minimum.max(1);
    while width < FULL {
        let mut prefixes: Vec<&str> = identities
            .iter()
            .map(|id| id.get(..width.min(id.len())).unwrap_or(id))
            .collect();
        prefixes.sort_unstable();
        let before = prefixes.len();
        prefixes.dedup();
        if prefixes.len() == before {
            return width;
        }
        width += 1;
    }
    FULL
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_give_the_same_plan_the_same_identity_when_nothing_about_it_changed() {
        let first = PlanId::of("session-1", "2026-09-09T10:00:00Z", "restart service nginx");
        let second = PlanId::of("session-1", "2026-09-09T10:00:00Z", "restart service nginx");
        assert_eq!(
            first, second,
            "a plan is identified by what it is, so building it twice must not mint two ids"
        );
    }

    #[test]
    fn should_give_different_intents_different_identities() {
        let restart = PlanId::of("session-1", "2026-09-09T10:00:00Z", "restart service nginx");
        let stop = PlanId::of("session-1", "2026-09-09T10:00:00Z", "stop service nginx");
        assert_ne!(
            restart, stop,
            "two different intents are two different plans (§3.2)"
        );
    }

    #[test]
    fn should_read_back_a_reference_written_with_or_without_its_marker() {
        let plan = PlanId::of("s", "t", "i");
        let rendered = plan.to_string();
        assert!(rendered.starts_with("plan/"), "got {rendered}");
        let bare = PlanId::parse(plan.as_str()).expect("a bare body parses");
        let marked = PlanId::parse(&rendered).expect("a marked reference parses");
        assert_eq!(bare, plan, "`{}` must read back", plan.as_str());
        assert_eq!(marked, plan, "`{rendered}` must read back");
    }

    #[test]
    fn should_refuse_a_reference_that_is_not_an_identity() {
        for text in [
            "",
            "ZZZZ",
            "plan/",
            "0123456789abcdef0",
            "../etc/passwd",
            "a b",
        ] {
            assert!(
                PlanId::parse(text).is_none(),
                "`{text}` is not an identity and must be refused rather than searched for"
            );
        }
    }

    #[test]
    fn should_match_a_prefix_of_the_identity_and_nothing_longer() {
        let plan = PlanId::of("s", "t", "i");
        assert!(plan.matches(plan.short()), "the short form must resolve");
        assert!(plan.matches(plan.as_str()), "the full form must resolve");
        assert!(
            !plan.matches(&format!("{}0", plan.as_str())),
            "a reference longer than the identity is not this plan"
        );
        assert!(
            !plan.matches(""),
            "an empty reference matches nothing, or it would match everything"
        );
    }

    #[test]
    fn should_widen_a_reference_only_as_far_as_a_collision_requires() {
        assert_eq!(
            shortest_unique_prefixes(&["a82f0000", "91cc0000"], 4),
            4,
            "two identities that differ at once stay four characters wide"
        );
        assert_eq!(
            shortest_unique_prefixes(&["a82f0000", "a82f1111"], 4),
            5,
            "identities sharing four characters must render five (§36.4)"
        );
    }

    #[test]
    fn should_keep_each_kind_of_identity_in_its_own_space() {
        let plan = PlanId::derive(&["x"]);
        let action = ActionId::derive(&["x"]);
        assert_ne!(
            plan.as_str(),
            action.as_str(),
            "a plan and an action derived from the same word must not collide (§3.2, §3.3)"
        );
    }
}
