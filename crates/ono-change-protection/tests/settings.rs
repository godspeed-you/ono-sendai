//! The reference configuration (v0.6 §53).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::HashMap;
use std::time::Duration;

use ono_change_core::{ProtectionMode, StrategyKind};
use ono_change_protection::policy::FreeSpaceFloor;
use ono_change_protection::settings::{ChangeSettings, KEYS, RootRecovery};
use ono_core::ErrorCode;
use ono_value::{ByteSize, Percent, Value};

fn source(pairs: &[(&str, Value)]) -> impl Fn(&str) -> Option<Value> + use<> {
    let table: HashMap<String, Value> = pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    move |key: &str| table.get(key).cloned()
}

#[test]
fn should_carry_the_reference_configuration_of_the_specification() {
    let settings = ChangeSettings::defaults();
    assert_eq!(settings.default_protection(), ProtectionMode::Prefer);
    assert_eq!(settings.default_strategy(), StrategyKind::Sequential);
    assert!(settings.high_risk_requires_ack());
    assert!(settings.critical_risk_requires_ack());
    assert!(!settings.allow_opaque_actions());
    assert_eq!(settings.bulk_warn_targets(), 10);
    assert_eq!(settings.bulk_high_risk_targets(), 50);
    assert_eq!(settings.retention(), Duration::from_secs(24 * 60 * 60));
    assert_eq!(settings.max_auto_snapshot_count(), 32);
    assert_eq!(
        settings.min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(10.0))
    );
    assert!(settings.prefer_read_only_snapshots());
    assert!(settings.zfs_enabled());
    assert!(settings.zfs_prefer_selective_restore());
    assert!(
        !settings.zfs_allow_destructive_rollback(),
        "§53: `recovery.zfs.allow_destructive_rollback` is false, and §13.6 is why"
    );
    assert!(settings.btrfs_enabled());
    assert!(settings.btrfs_prefer_read_only_snapshots());
    assert_eq!(settings.btrfs_root_recovery(), RootRecovery::NextBoot);
}

#[test]
fn should_default_every_key_a_configuration_does_not_answer() {
    let empty = source(&[]);
    let settings = ChangeSettings::from_settings(&empty).expect("an empty source is a valid one");
    assert_eq!(
        settings,
        ChangeSettings::defaults(),
        "§53: an unset key takes the reference value rather than an invented one"
    );
}

#[test]
fn should_name_every_key_the_specification_defines() {
    assert_eq!(KEYS.len(), 17);
    let settings = ChangeSettings::defaults();
    for (key, _) in settings.entries() {
        assert!(
            KEYS.contains(&key),
            "§53: `{key}` is rendered and so it is one of the keys"
        );
    }
    assert_eq!(settings.entries().len(), KEYS.len());
}

#[test]
fn should_read_every_key_a_configuration_does_answer() {
    let lookup = source(&[
        ("change.default_protection", Value::string("require")),
        ("change.default_strategy", Value::string("canary")),
        ("change.high_risk_requires_ack", Value::Bool(false)),
        ("change.critical_risk_requires_ack", Value::Bool(false)),
        ("change.allow_opaque_actions", Value::Bool(true)),
        ("change.bulk.warn_targets", Value::Int(5)),
        ("change.bulk.high_risk_targets", Value::Int(25)),
        ("recovery.retention", Value::string("72h")),
        ("recovery.max_auto_snapshot_count", Value::Int(8)),
        ("recovery.min_filesystem_free", Value::string("15%")),
        ("recovery.prefer_read_only_snapshots", Value::Bool(false)),
        ("recovery.zfs.enabled", Value::Bool(false)),
        ("recovery.zfs.prefer_selective_restore", Value::Bool(false)),
        ("recovery.zfs.allow_destructive_rollback", Value::Bool(true)),
        ("recovery.btrfs.enabled", Value::Bool(false)),
        (
            "recovery.btrfs.prefer_read_only_snapshots",
            Value::Bool(false),
        ),
        (
            "recovery.btrfs.root_recovery",
            Value::string("offline-replacement"),
        ),
    ]);
    let settings = ChangeSettings::from_settings(&lookup).expect("every value is well-formed");
    assert_eq!(settings.default_protection(), ProtectionMode::Require);
    assert_eq!(settings.default_strategy(), StrategyKind::Canary);
    assert!(!settings.high_risk_requires_ack());
    assert!(settings.allow_opaque_actions());
    assert_eq!(settings.bulk_warn_targets(), 5);
    assert_eq!(settings.bulk_high_risk_targets(), 25);
    assert_eq!(settings.retention(), Duration::from_secs(72 * 60 * 60));
    assert_eq!(settings.max_auto_snapshot_count(), 8);
    assert_eq!(
        settings.min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(15.0))
    );
    assert!(settings.zfs_allow_destructive_rollback());
    assert_eq!(
        settings.btrfs_root_recovery(),
        RootRecovery::OfflineSubvolumeReplacement
    );
}

#[test]
fn should_keep_an_explicit_plan_requirement_when_configuration_asks_for_less() {
    let lookup = source(&[("change.default_protection", Value::string("prefer"))]);
    let settings = ChangeSettings::from_settings(&lookup).expect("prefer is a mode");
    assert_eq!(
        settings.protection_mode_for(Some(ProtectionMode::Require)),
        ProtectionMode::Require,
        "§53: configuration MUST NOT silently weaken explicit plan requirements"
    );
    assert_eq!(
        settings.policy_for(Some(ProtectionMode::Require)).mode(),
        ProtectionMode::Require,
        "§17.3: the plan's `--protection require` survives into the policy it runs under"
    );
}

#[test]
fn should_take_the_configured_mode_when_the_plan_asks_for_nothing() {
    let lookup = source(&[("change.default_protection", Value::string("maximize"))]);
    let settings = ChangeSettings::from_settings(&lookup).expect("maximize is a mode");
    assert_eq!(
        settings.protection_mode_for(None),
        ProtectionMode::Maximize,
        "§17.1: configuration supplies the default a plan did not state"
    );
}

#[test]
fn should_wire_the_configured_retention_and_limits_into_the_policy() {
    let lookup = source(&[
        ("recovery.retention", Value::string("6h")),
        ("recovery.max_auto_snapshot_count", Value::Int(4)),
        ("recovery.min_filesystem_free", Value::string("20%")),
    ]);
    let settings = ChangeSettings::from_settings(&lookup).expect("every value is well-formed");
    let policy = settings.policy_for(None);
    assert_eq!(
        policy.retention().window(),
        Duration::from_secs(6 * 60 * 60)
    );
    assert_eq!(policy.limits().max_snapshot_count(), Some(4));
    assert_eq!(
        policy.limits().min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(20.0))
    );
}

#[test]
fn should_refuse_a_protection_mode_the_specification_does_not_define() {
    let lookup = source(&[("change.default_protection", Value::string("paranoid"))]);
    let refusal =
        ChangeSettings::from_settings(&lookup).expect_err("§17.2 fixes four modes and no more");
    assert_eq!(refusal.code(), ErrorCode::TypeMismatch);
    assert!(
        refusal
            .help()
            .is_some_and(|help| help.contains("weaken explicit plan requirements")),
        "§53: a key Ono cannot read is refused rather than ignored"
    );
}

#[test]
fn should_refuse_a_value_of_the_wrong_shape() {
    for (key, value) in [
        ("change.high_risk_requires_ack", Value::string("yes")),
        ("change.bulk.warn_targets", Value::string("ten")),
        ("recovery.retention", Value::Bool(true)),
        ("recovery.min_filesystem_free", Value::Bool(true)),
        ("recovery.btrfs.root_recovery", Value::string("whenever")),
        ("change.default_strategy", Value::string("stampede")),
    ] {
        let lookup = source(&[(key, value)]);
        assert!(
            ChangeSettings::from_settings(&lookup).is_err(),
            "§53: `{key}` carrying the wrong shape of value is a structured refusal"
        );
    }
}

#[test]
fn should_read_a_free_space_floor_stated_as_a_quantity() {
    let lookup = source(&[(
        "recovery.min_filesystem_free",
        Value::ByteSize(ByteSize::from_bytes(20 * 1024 * 1024 * 1024)),
    )]);
    let settings = ChangeSettings::from_settings(&lookup).expect("a size is a floor");
    assert_eq!(
        settings.min_filesystem_free(),
        FreeSpaceFloor::Absolute(ByteSize::from_bytes(20 * 1024 * 1024 * 1024)),
        "Appendix D.3: a floor may be a quantity where a share of a large pool is meaningless"
    );
}

#[test]
fn should_read_a_retention_stated_as_a_duration_value() {
    let lookup = source(&[(
        "recovery.retention",
        Value::Duration(ono_value::Duration::from_nanoseconds(3_600 * 1_000_000_000)),
    )]);
    let settings = ChangeSettings::from_settings(&lookup).expect("a duration is a retention");
    assert_eq!(settings.retention(), Duration::from_secs(3600));
}

#[test]
fn should_name_every_root_recovery_method_the_spec_distinguishes() {
    assert_eq!(RootRecovery::ALL.len(), 3);
    for method in RootRecovery::ALL {
        assert_eq!(RootRecovery::from_name(method.as_str()), Some(*method));
    }
    assert_eq!(
        RootRecovery::NextBoot.as_str(),
        "next-boot",
        "§53: `recovery.btrfs.root_recovery = \"next-boot\"`"
    );
}

#[test]
fn should_refuse_a_negative_count() {
    let lookup = source(&[("change.bulk.warn_targets", Value::Int(-1))]);
    assert!(
        ChangeSettings::from_settings(&lookup).is_err(),
        "§28.3: a bulk guard counts targets, and there are never fewer than none"
    );
}
