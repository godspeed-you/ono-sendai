//! A remote plan's protection, per host and composed (v0.6 §29.1, §29.2, §29.3, §55.10 case 44).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{
    CoverageExclusion, DomainCoverage, DomainProtection, EffectDomain, ProtectionLevel,
    ProtectionSummary, RecoveryAssetId, RecoveryObjective,
};
use ono_change_protection::ProtectionPolicy;
use ono_change_protection::hosts::{HostCoverage, compose};

const CREATED_AT: &str = "2026-09-10T08:00:00Z";

/// A host whose configuration file a snapshot of `provider` covers.
fn protected_host(host: &str, provider: &str, dataset: &str) -> HostCoverage {
    HostCoverage::analysed(
        host,
        ProtectionSummary::of(vec![
            DomainCoverage::new(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
                format!("/etc/nginx/nginx.conf, by a snapshot of {dataset}"),
            )
            .by_asset(RecoveryAssetId::of(provider, None, dataset, CREATED_AT)),
        ]),
    )
}

/// A host whose configuration file nothing can capture.
fn unprotected_host(host: &str) -> HostCoverage {
    HostCoverage::analysed(
        host,
        ProtectionSummary::of(vec![DomainCoverage::new(
            EffectDomain::FilesystemPersistent,
            RecoveryObjective::PreserveExact,
            DomainProtection::Unprotected,
            "/etc/nginx/nginx.conf, on ext4 with no recovery provider",
        )]),
    )
}

/// §29.2's twenty hosts: twelve on ZFS, six on Btrfs, two with nothing.
fn section_twenty_nine_two_fleet() -> Vec<HostCoverage> {
    let mut hosts = Vec::new();
    for n in 1..=12 {
        hosts.push(protected_host(
            &format!("zfs-{n:02}"),
            "ono.recovery.zfs",
            "rpool/ROOT/debian",
        ));
    }
    for n in 1..=6 {
        hosts.push(protected_host(
            &format!("btrfs-{n:02}"),
            "ono.recovery.btrfs",
            "@",
        ));
    }
    hosts.push(unprotected_host("plain-01"));
    hosts.push(unprotected_host("plain-02"));
    hosts
}

#[test]
fn should_compose_twelve_zfs_six_btrfs_and_two_unprotected_hosts_to_partially_protected() {
    let plan = compose(
        &section_twenty_nine_two_fleet(),
        &ProtectionPolicy::default(),
    );

    assert_eq!(
        plan.level(),
        ProtectionLevel::PartiallyProtected,
        "§29.2: a plan two of whose twenty hosts are unprotected is partially protected"
    );
    assert_eq!(
        plan.shortfall().len(),
        2,
        "§10.3: the matrix names the two uncovered hosts' rows as the shortfall"
    );
}

#[test]
fn should_keep_each_hosts_own_protection_word_beside_the_plans() {
    let hosts = section_twenty_nine_two_fleet();

    let protected = hosts
        .iter()
        .filter(|host| host.level() == ProtectionLevel::Protected)
        .count();
    let unprotected: Vec<&str> = hosts
        .iter()
        .filter(|host| host.level() == ProtectionLevel::Unprotected)
        .map(HostCoverage::host)
        .collect();

    assert_eq!(
        protected, 18,
        "§29.1: each of the eighteen snapshot-capable hosts is protected on its own terms"
    );
    assert_eq!(
        unprotected,
        ["plain-01", "plain-02"],
        "§29.1: the two unprotected hosts are named, and the fleet's word does not hide them"
    );
    assert_eq!(
        compose(&hosts, &ProtectionPolicy::default()).rows().len(),
        20,
        "§29.2: every host's row reaches the plan's matrix"
    );
}

#[test]
fn should_protect_the_plan_only_when_policy_excludes_the_unprotected_hosts() {
    let policy = ProtectionPolicy::default()
        .excluding_host("plain-01")
        .excluding_host("plain-02");

    let plan = compose(&section_twenty_nine_two_fleet(), &policy);

    assert_eq!(
        plan.level(),
        ProtectionLevel::Protected,
        "§29.2: the plan is partially protected \"unless policy excludes the unprotected targets\""
    );
    let excluded: Vec<&str> = plan
        .exclusions()
        .into_iter()
        .map(CoverageExclusion::subject)
        .collect();
    assert_eq!(
        excluded,
        ["plain-01", "plain-02"],
        "§10.3: an excluded host stays visible among the exclusions rather than disappearing"
    );
}

#[test]
fn should_not_let_one_excluded_host_hide_the_other_unprotected_one() {
    let policy = ProtectionPolicy::default().excluding_host("plain-01");

    let plan = compose(&section_twenty_nine_two_fleet(), &policy);

    assert_eq!(
        plan.level(),
        ProtectionLevel::PartiallyProtected,
        "§2.2: a policy that excludes one unprotected host says nothing about the other"
    );
}

#[test]
fn should_not_let_reachable_hosts_speak_for_one_that_could_not_be_analysed() {
    let mut hosts: Vec<HostCoverage> = (1..=19)
        .map(|n| protected_host(&format!("zfs-{n:02}"), "ono.recovery.zfs", "rpool/ROOT"))
        .collect();
    hosts.push(HostCoverage::unestablished(
        "zfs-20",
        "the link dropped during discovery",
    ));

    let plan = compose(&hosts, &ProtectionPolicy::default());

    assert_eq!(
        plan.level(),
        ProtectionLevel::PartiallyProtected,
        "§29.3 and Appendix A.7: a host whose protection is unknown caps the plan"
    );
    assert!(
        plan.shortfall()
            .iter()
            .any(|row| row.note().contains("zfs-20") && row.note().contains("link dropped")),
        "§29.3: the matrix names the host it could not analyse and why"
    );
    assert_eq!(
        hosts[19].level(),
        ProtectionLevel::Unknown,
        "§29.3: an unanalysed host is unknown, neither protected nor failed"
    );
}

#[test]
fn should_call_a_plan_unprotected_when_no_host_is_protected() {
    let plan = compose(
        &[unprotected_host("plain-01"), unprotected_host("plain-02")],
        &ProtectionPolicy::default(),
    );

    assert_eq!(
        plan.level(),
        ProtectionLevel::Unprotected,
        "§10.2: nothing covered anywhere is unprotected"
    );
}

#[test]
fn should_carry_each_hosts_exclusions_into_the_plan_matrix_exactly_once() {
    let host = HostCoverage::analysed(
        "web-01",
        ProtectionSummary::of(vec![
            DomainCoverage::new(
                EffectDomain::FilesystemPersistent,
                RecoveryObjective::PreserveExact,
                DomainProtection::Protected,
                "/etc/nginx/nginx.conf, by a snapshot of rpool/ROOT",
            )
            .excluding(CoverageExclusion::new(
                EffectDomain::FilesystemPersistent,
                "/var/lib/app",
                "a separate dataset the snapshot does not reach (§13.4)",
            )),
        ])
        .excluding(
            CoverageExclusion::new(
                EffectDomain::ExternalSideEffect,
                "POST https://hooks.example/deploy",
                "an emitted request no snapshot recalls (§35.1)",
            )
            .irreversible(),
        ),
    );

    let plan = compose(&[host], &ProtectionPolicy::default());

    let subjects: Vec<&str> = plan
        .exclusions()
        .into_iter()
        .map(CoverageExclusion::subject)
        .collect();
    assert_eq!(
        subjects,
        ["/var/lib/app", "POST https://hooks.example/deploy"],
        "§10.3 and §62.6: every exclusion a host stated reaches the plan's matrix, once"
    );
}
