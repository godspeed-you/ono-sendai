//! What a sealed plan promised about protection, held against what can be prepared at apply
//! (v0.6 §2.3, §4.5, §10.3, §18.2).
//!
//! The coverage matrix is part of the sealed plan, and the operator approved the plan with it in
//! view. `apply` recomputes the protection actions, because a sealed plan carries the matrix
//! rather than the actions (§10.3). If the recomputation can no longer protect a domain the plan
//! showed as protected, running anyway would downgrade protected execution to unprotected
//! execution without saying so — which the lifecycle forbids.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{
    DomainCoverage, DomainProtection, EffectDomain, ProtectionSummary, RecoveryObjective,
};
use ono_change_protection::coverage::protection_lost;

fn row(domain: &str, protection: DomainProtection) -> DomainCoverage {
    DomainCoverage::new(
        EffectDomain::from_name(domain).expect("a domain of the closed list"),
        RecoveryObjective::PreserveExact,
        protection,
        "fixture",
    )
}

#[test]
fn should_name_a_domain_the_sealed_plan_protected_and_apply_no_longer_can() {
    let sealed = ProtectionSummary::of(vec![row(
        "filesystem-persistent",
        DomainProtection::Protected,
    )]);
    let fresh = ProtectionSummary::of(vec![row(
        "filesystem-persistent",
        DomainProtection::Unprotected,
    )]);
    assert_eq!(
        protection_lost(&sealed, &fresh),
        vec!["filesystem-persistent".to_owned()],
        "§2.3: a protection the plan showed and apply cannot give is named, not dropped"
    );
}

#[test]
fn should_name_a_protected_domain_the_recomputation_no_longer_mentions() {
    let sealed = ProtectionSummary::of(vec![row(
        "filesystem-persistent",
        DomainProtection::Protected,
    )]);
    assert_eq!(
        protection_lost(&sealed, &ProtectionSummary::empty()),
        vec!["filesystem-persistent".to_owned()],
        "an analysis that came back empty did not keep the promise either"
    );
}

#[test]
fn should_find_nothing_lost_when_apply_can_still_protect_what_the_plan_showed() {
    let sealed = ProtectionSummary::of(vec![row(
        "filesystem-persistent",
        DomainProtection::Protected,
    )]);
    let fresh = sealed.clone();
    assert!(protection_lost(&sealed, &fresh).is_empty());
}

#[test]
fn should_find_nothing_lost_where_the_plan_never_promised_protection() {
    let sealed = ProtectionSummary::of(vec![row(
        "filesystem-persistent",
        DomainProtection::Unprotected,
    )]);
    let fresh = ProtectionSummary::empty();
    assert!(
        protection_lost(&sealed, &fresh).is_empty(),
        "an unprotected plan applies unprotected, as it said it would"
    );
}
