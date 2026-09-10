//! Protection policy and the profiles over it (v0.6 §17, §38.3, Appendix H).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::Path;
use std::time::Duration;

use ono_change_core::{
    ConsistencyClass, EffectDomain, PlanId, ProtectionLevel, ProtectionMode, RecoveryCost,
    RecoveryObjective, RestoreMethod, RetentionPolicy,
};
use ono_change_protection::coverage::{CoverageRequest, analyse};
use ono_change_protection::policy::{
    CostLimits, FreeSpaceFloor, LimitBreach, Profile, ProtectionPolicy, effective_mode, level_rank,
};
use ono_change_protection::{MountTable, ProviderRegistry};
use ono_core::ErrorCode;
use ono_value::{ByteSize, Percent};

const GIB: u128 = 1024 * 1024 * 1024;

mod support;

use support::{TestProvider, ZFS_ROOT, candidate, config_mutation, snapshot_cost};

fn plan() -> PlanId {
    PlanId::of("session-1", "2026-09-09T10:00:00Z", "replace nginx.conf")
}

fn protecting_registry() -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    registry
        .register(
            TestProvider::new("ono.recovery.zfs")
                .offering(
                    candidate(
                        "ono.recovery.zfs",
                        "zfs-dataset",
                        "rpool/ROOT/debian",
                        &["/etc/nginx/nginx.conf"],
                        EffectDomain::FilesystemPersistent,
                        RecoveryObjective::PreserveExact,
                    )
                    .at_consistency(ConsistencyClass::FilesystemConsistent)
                    .restored_by(RestoreMethod::SelectiveFileRestore)
                    .costing(snapshot_cost()),
                )
                .shared(),
        )
        .expect("the fixture declares every §12.2 capability");
    registry
}

#[test]
fn should_default_to_prefer_with_the_reference_retention_and_limits() {
    let policy = ProtectionPolicy::default();
    assert_eq!(
        policy.mode(),
        ProtectionMode::Prefer,
        "§17.1: the default interactive policy is `prefer`"
    );
    assert_eq!(
        policy.retention().window(),
        Duration::from_secs(24 * 60 * 60),
        "§37.1: twenty-four hours after successful verification"
    );
    assert_eq!(policy.limits().max_snapshot_count(), Some(32));
    assert_eq!(policy.required_level(), ProtectionLevel::Protected);
}

#[test]
fn should_raise_the_mode_when_a_stricter_profile_is_applied() {
    let policy = ProtectionPolicy::of(ProtectionMode::Prefer).tighten_with(Profile::Cautious);
    assert_eq!(
        policy.mode(),
        ProtectionMode::Require,
        "Appendix H.2: the cautious profile requires protection"
    );
    assert_eq!(policy.profile(), Some(Profile::Cautious));
}

#[test]
fn should_never_weaken_a_plans_requirement_when_a_looser_profile_is_applied() {
    let policy = ProtectionPolicy::of(ProtectionMode::Require).tighten_with(Profile::Fleet);
    assert_eq!(
        policy.mode(),
        ProtectionMode::Require,
        "Appendix H.5: a profile MUST NOT weaken a requirement the plan already imposes"
    );
}

#[test]
fn should_raise_the_free_space_floor_and_never_lower_it() {
    let raised = ProtectionPolicy::default().tighten_with(Profile::Cautious);
    assert_eq!(
        raised.limits().min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(15.0)),
        "Appendix H.2: the cautious profile keeps fifteen percent free"
    );

    let already_stricter = ProtectionPolicy::default()
        .limited_by(CostLimits::default().above_floor(FreeSpaceFloor::Share(Percent::new(25.0))))
        .tighten_with(Profile::Interactive);
    assert_eq!(
        already_stricter.limits().min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(25.0)),
        "Appendix H.5: the interactive profile's ten percent does not lower a stricter floor"
    );
}

#[test]
fn should_keep_the_longer_retention_when_a_profile_is_applied() {
    let lengthened = ProtectionPolicy::default().tighten_with(Profile::Cautious);
    assert_eq!(
        lengthened.retention().window(),
        Duration::from_secs(72 * 60 * 60),
        "Appendix H.2: the cautious profile retains for seventy-two hours"
    );

    let already_longer = ProtectionPolicy::default()
        .retaining(RetentionPolicy::of(Duration::from_secs(96 * 60 * 60)))
        .tighten_with(Profile::Fleet);
    assert_eq!(
        already_longer.retention().window(),
        Duration::from_secs(96 * 60 * 60),
        "Appendix H.5: a profile's shorter retention does not shorten what the plan asked for"
    );
}

#[test]
fn should_keep_an_explicit_hold_through_a_profile() {
    let held = ProtectionPolicy::default()
        .retaining(RetentionPolicy::held())
        .tighten_with(Profile::Interactive);
    assert!(
        held.retention().is_held(),
        "§37.2: an explicit hold is a safety constraint, and Appendix H.5 forbids weakening it"
    );
}

#[test]
fn should_keep_the_narrower_cost_cap_when_a_profile_is_applied() {
    let policy = ProtectionPolicy::default()
        .limited_by(CostLimits::default().sized(ByteSize::from_bytes(1024 * 1024)))
        .tighten_with(Profile::Cautious);
    assert_eq!(
        policy.limits().max_estimated_size(),
        Some(ByteSize::from_bytes(1024 * 1024)),
        "Appendix H.5: applying a profile narrows what automatic protection may do, never widens it"
    );
}

#[test]
fn should_expand_every_profile_into_inspectable_settings() {
    for profile in Profile::ALL {
        let settings = profile.settings();
        for key in ["protection", "retention", "risk gate", "strategy"] {
            assert!(
                settings.iter().any(|(name, _)| *name == key),
                "Appendix H: the {} profile MUST expand to inspectable settings, and `{key}` is \
                 one of them",
                profile.as_str()
            );
        }
        assert_eq!(Profile::from_name(profile.as_str()), Some(*profile));
    }
    let fleet: Vec<String> = Profile::Fleet
        .settings()
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    assert!(
        fleet.iter().any(|line| line.contains("canary")),
        "Appendix H.3: the fleet profile's strategy is canary 1 then batch 10%"
    );
    assert!(
        Profile::Scripted
            .settings()
            .iter()
            .any(|(key, value)| *key == "prompts" && value == "no"),
        "Appendix H.4: a scripted profile never stops for a yes/no question (§17.4)"
    );
}

#[test]
fn should_refuse_the_plan_when_require_cannot_be_met() {
    let registry = ProviderRegistry::new();
    let policy = ProtectionPolicy::of(ProtectionMode::Require);
    let analysis = analyse(&CoverageRequest::new(&registry, &policy).mutating(config_mutation()));

    let refusal = policy
        .enforce(&plan(), &analysis)
        .expect_err("§17.2: `require` refuses to apply when a domain cannot reach the class");
    assert_eq!(refusal.code(), ErrorCode::RecoveryCoverageInsufficient);
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("Nothing was changed")),
        "§17.2: the refusal happens before PREPARE, and the message says so"
    );
}

#[test]
fn should_let_a_require_plan_through_when_every_domain_is_covered() {
    let registry = protecting_registry();
    let policy = ProtectionPolicy::of(ProtectionMode::Require);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(MountTable::from_text(ZFS_ROOT).resolve(Path::new("/etc/nginx/nginx.conf")))
            .mutating(config_mutation()),
    );

    assert_eq!(analysis.level(), ProtectionLevel::Protected);
    assert!(policy.enforce(&plan(), &analysis).is_ok());
}

#[test]
fn should_not_refuse_a_shortfall_in_any_mode_but_require() {
    for mode in [
        ProtectionMode::Off,
        ProtectionMode::Prefer,
        ProtectionMode::Maximize,
    ] {
        let registry = ProviderRegistry::new();
        let policy = ProtectionPolicy::of(mode);
        let analysis =
            analyse(&CoverageRequest::new(&registry, &policy).mutating(config_mutation()));
        assert!(
            policy.enforce(&plan(), &analysis).is_ok(),
            "§17.2: only `require` refuses to apply on unmet coverage, and {mode} is not it"
        );
    }
}

#[test]
fn should_report_what_was_available_when_protection_is_off() {
    let registry = protecting_registry();
    let policy = ProtectionPolicy::of(ProtectionMode::Off);
    let analysis = analyse(
        &CoverageRequest::new(&registry, &policy)
            .over(MountTable::from_text(ZFS_ROOT).resolve(Path::new("/etc/nginx/nginx.conf")))
            .mutating(config_mutation()),
    );

    let note = policy.availability_note(&analysis);
    assert!(
        note.contains("1 protection opportunity was available"),
        "§17.2: `off` still shows available protection opportunities: {note}"
    );
}

#[test]
fn should_keep_an_explicit_plan_requirement_against_a_looser_configuration() {
    assert_eq!(
        effective_mode(ProtectionMode::Prefer, Some(ProtectionMode::Require)),
        ProtectionMode::Require,
        "§53: configuration MUST NOT silently weaken explicit plan requirements"
    );
    assert_eq!(
        effective_mode(ProtectionMode::Require, None),
        ProtectionMode::Require,
        "§17.3: a plan that asks for nothing takes the configured mode"
    );
    assert_eq!(
        effective_mode(ProtectionMode::Require, Some(ProtectionMode::Off)),
        ProtectionMode::Require,
        "§53: a plan does not silently weaken a configuration that requires protection either"
    );
}

#[test]
fn should_rank_unknown_protection_below_every_level_that_says_something() {
    assert!(
        level_rank(ProtectionLevel::Unknown) < level_rank(ProtectionLevel::Protected),
        "§2.4: an unestablished recovery property never satisfies a requirement for coverage"
    );
    assert!(
        level_rank(ProtectionLevel::PartiallyProtected) < level_rank(ProtectionLevel::Protected)
    );
    assert!(level_rank(ProtectionLevel::Protected) <= level_rank(ProtectionLevel::Transactional));
}

#[test]
fn should_name_the_limit_a_candidate_exceeded() {
    let limits = CostLimits::default()
        .sized(ByteSize::from_bytes(1024))
        .quiesced(Duration::from_secs(2))
        .scoped(1);
    let cost = RecoveryCost::unknown()
        .with_space(Some(ByteSize::from_bytes(8192)), None, true)
        .with_quiesce(Duration::from_secs(30));
    let breaches = limits.breaches(&cost, 4, 0);
    assert_eq!(
        breaches.len(),
        3,
        "§38.3: estimated size, target scope and quiesce duration are three separate bounds"
    );
    assert!(matches!(breaches[0], LimitBreach::EstimatedSize { .. }));
    assert!(breaches[2].describe().contains("§18.4"));
}

#[test]
fn should_measure_a_free_space_floor_as_a_share_or_as_a_quantity() {
    let share = FreeSpaceFloor::Share(Percent::new(10.0));
    assert!(share.is_cleared_by(ByteSize::from_bytes(20), ByteSize::from_bytes(100)));
    assert!(!share.is_cleared_by(ByteSize::from_bytes(5), ByteSize::from_bytes(100)));

    let absolute = FreeSpaceFloor::Absolute(ByteSize::from_bytes(1024));
    assert!(absolute.is_cleared_by(ByteSize::from_bytes(2048), ByteSize::from_bytes(4096)));
    assert!(!absolute.is_cleared_by(ByteSize::from_bytes(512), ByteSize::from_bytes(4096)));
    assert!(
        !share.is_cleared_by(ByteSize::ZERO, ByteSize::ZERO),
        "Appendix D.3: a filesystem of unknown size does not clear a floor by default"
    );
}

#[test]
fn should_take_the_stricter_of_two_free_space_floors() {
    let ten = FreeSpaceFloor::Share(Percent::new(10.0));
    let fifteen = FreeSpaceFloor::Share(Percent::new(15.0));
    assert_eq!(ten.stricter_of(fifteen), fifteen);
    assert_eq!(
        FreeSpaceFloor::Absolute(ByteSize::from_bytes(10))
            .stricter_of(FreeSpaceFloor::Absolute(ByteSize::from_bytes(20))),
        FreeSpaceFloor::Absolute(ByteSize::from_bytes(20))
    );
    assert_eq!(
        FreeSpaceFloor::Absolute(ByteSize::from_bytes(10)).stricter_of(ten),
        FreeSpaceFloor::Both {
            share: Percent::new(10.0),
            absolute: ByteSize::from_bytes(10)
        },
        "Appendix H.5: without a size neither a share nor a quantity is the stricter one, so \
         both stay in force"
    );
}

#[test]
fn should_bound_nothing_when_the_limits_are_explicitly_unbounded() {
    let limits = CostLimits::unbounded();
    let cost = RecoveryCost::unknown()
        .with_space(Some(ByteSize::from_bytes(u128::from(u64::MAX))), None, true)
        .with_quiesce(Duration::from_secs(3600));
    assert!(
        limits.breaches(&cost, 10_000, 10_000).is_empty(),
        "§38.3: the bounds are configuration, and a caller may state that there are none"
    );
}

#[test]
fn should_pick_whichever_floor_demands_more_room_on_a_filesystem_of_known_size() {
    let share = FreeSpaceFloor::Share(Percent::new(10.0));
    let absolute = FreeSpaceFloor::Absolute(ByteSize::from_bytes(50 * GIB));

    assert_eq!(
        share.stricter_at(absolute, ByteSize::from_bytes(100 * GIB)),
        absolute,
        "Appendix H.5: on 100 GiB, 50 GiB free is stricter than ten percent"
    );
    assert_eq!(
        absolute.stricter_at(share, ByteSize::from_bytes(2048 * GIB)),
        share,
        "Appendix H.5: on 2 TiB, ten percent is stricter than 50 GiB"
    );
}

#[test]
fn should_keep_both_floors_in_force_when_no_filesystem_size_is_known() {
    let combined = FreeSpaceFloor::Share(Percent::new(10.0))
        .stricter_of(FreeSpaceFloor::Absolute(ByteSize::from_bytes(50 * GIB)));

    assert!(
        !combined.is_cleared_by(
            ByteSize::from_bytes(20 * GIB),
            ByteSize::from_bytes(100 * GIB)
        ),
        "Appendix H.5: twenty percent free clears the share and not the 50 GiB floor, and a \
         profile may not weaken either"
    );
    assert!(
        !combined.is_cleared_by(
            ByteSize::from_bytes(60 * GIB),
            ByteSize::from_bytes(1024 * GIB)
        ),
        "Appendix H.5: 60 GiB clears the absolute floor and not ten percent of 1 TiB"
    );
    assert!(combined.is_cleared_by(
        ByteSize::from_bytes(60 * GIB),
        ByteSize::from_bytes(100 * GIB)
    ));
}
