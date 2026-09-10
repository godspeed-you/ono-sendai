//! The reference configuration (v0.6 §53).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::HashMap;
use std::time::Duration;

use ono_change_core::{ProtectionMode, RiskClass, StrategyKind};
use ono_change_protection::policy::{FreeSpaceFloor, Profile};
use ono_change_protection::settings::{ChangeSettings, KEYS, RootRecovery, Source};
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
    // §53's seventeen, and `change.profile`, the extension that selects an Appendix H profile.
    assert_eq!(KEYS.len(), 18);
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
fn should_say_so_when_configuration_answers_a_plan_with_a_mode_it_did_not_ask_for() {
    // §17.2's four modes are ordered so that `require` — the one that refuses on a shortfall — is
    // the strictest. `maximize` is not weaker; it is a different property, and a plan that asked
    // for breadth and got refusal instead lost something. §53 permits the choice and forbids it
    // being silent.
    let lookup = source(&[("change.default_protection", Value::string("require"))]);
    let settings = ChangeSettings::from_settings(&lookup).expect("require is a mode");
    let policy = settings.policy_for(Some(ProtectionMode::Maximize));
    assert_eq!(policy.mode(), ProtectionMode::Require);
    assert_eq!(
        policy.narrowed(),
        Some(ProtectionMode::Maximize),
        "§17.3: the plan asked for `maximize` and this is the only record that it did"
    );
    assert!(
        policy
            .narrowing_note()
            .is_some_and(|note| note.contains("maximize") && note.contains("require")),
        "an operator is told which mode was asked for and which one runs"
    );
}

#[test]
fn should_report_no_narrowing_when_the_plan_got_the_mode_it_asked_for() {
    let lookup = source(&[("change.default_protection", Value::string("prefer"))]);
    let settings = ChangeSettings::from_settings(&lookup).expect("prefer is a mode");
    assert_eq!(
        settings
            .policy_for(Some(ProtectionMode::Require))
            .narrowed(),
        None,
        "there is nothing to report when the requirement survived"
    );
    assert_eq!(
        settings.policy_for(None).narrowed(),
        None,
        "a plan that asked for nothing was not narrowed"
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

/// §28.4 gives `batch` a size, and `batch 2` is how a strategy with its parameter is written. A
/// reader that accepted only the bare word refused the documented form.
#[test]
fn should_read_a_default_strategy_written_with_its_parameter() {
    let lookup = source(&[("change.default_strategy", Value::string("batch 2"))]);
    let (settings, problems) = ChangeSettings::read(&lookup);
    assert!(
        problems.is_empty(),
        "no problem with `batch 2`: {problems:?}"
    );
    assert_eq!(settings.default_strategy(), StrategyKind::Batch);
}

/// §53: a configuration nobody can read is reported rather than silently ignored — and one
/// unreadable key does not throw away every other key the operator set, least of all a
/// protection requirement.
#[test]
fn should_report_an_unreadable_key_and_keep_every_other_one() {
    let lookup = source(&[
        ("change.default_protection", Value::string("require")),
        ("change.default_strategy", Value::string("sideways")),
        ("change.allow_opaque_actions", Value::Bool(true)),
    ]);
    let (settings, problems) = ChangeSettings::read(&lookup);
    assert_eq!(
        problems.len(),
        1,
        "exactly the bad key is reported: {problems:?}"
    );
    assert!(
        problems[0].message().contains("change.default_strategy"),
        "the report names the key: {}",
        problems[0].message()
    );
    assert_eq!(settings.default_protection(), ProtectionMode::Require);
    assert!(settings.allow_opaque_actions());
    assert_eq!(
        settings.default_strategy(),
        ChangeSettings::defaults().default_strategy(),
        "the unreadable key keeps its default"
    );
}

// --- Appendix H's profiles, selected by `change.profile` ------------------------------------

fn under(profile: &str, pairs: &[(&str, Value)]) -> ChangeSettings {
    let mut all = vec![("change.profile", Value::string(profile))];
    all.extend(pairs.iter().cloned());
    let (settings, problems) = ChangeSettings::read(&source(&all));
    assert!(
        problems.is_empty(),
        "the configuration is readable: {problems:?}"
    );
    settings
}

#[test]
fn should_apply_no_profile_until_one_is_chosen() {
    assert_eq!(ChangeSettings::defaults().profile(), None);
    assert_eq!(under("none", &[]), ChangeSettings::defaults());
}

#[test]
fn should_select_each_profile_appendix_h_defines_by_its_name() {
    for profile in Profile::ALL {
        assert_eq!(under(profile.as_str(), &[]).profile(), Some(*profile));
    }
}

#[test]
fn should_raise_protection_retention_and_the_floor_under_the_cautious_profile() {
    let settings = under("cautious", &[]);
    assert_eq!(settings.default_protection(), ProtectionMode::Require);
    assert_eq!(settings.retention(), Duration::from_secs(72 * 60 * 60));
    assert_eq!(
        settings.min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(15.0)),
        "Appendix H.2: minimum free space 15%"
    );
    assert_eq!(
        settings.policy_for(None).mode(),
        ProtectionMode::Require,
        "and the policy a plan runs under is the tightened one"
    );
}

#[test]
fn should_never_loosen_a_stricter_setting_the_operator_wrote() {
    let settings = under(
        "fleet",
        &[
            ("change.default_protection", Value::string("require")),
            ("recovery.retention", Value::string("168h")),
            ("recovery.min_filesystem_free", Value::string("25%")),
        ],
    );
    assert_eq!(
        settings.default_protection(),
        ProtectionMode::Require,
        "Appendix H.5: `fleet` asks for `prefer`, and a profile never loosens"
    );
    assert_eq!(settings.retention(), Duration::from_secs(168 * 60 * 60));
    assert_eq!(
        settings.min_filesystem_free(),
        FreeSpaceFloor::Share(Percent::new(25.0))
    );
}

#[test]
fn should_keep_both_floors_when_the_profile_states_a_share_and_the_operator_a_quantity() {
    let settings = under(
        "cautious",
        &[("recovery.min_filesystem_free", Value::string("20GiB"))],
    );
    assert_eq!(
        settings.min_filesystem_free(),
        FreeSpaceFloor::Both {
            share: Percent::new(15.0),
            absolute: ByteSize::parse("20GiB").expect("a size"),
        },
        "Appendix H.5: without a filesystem size neither floor may be dropped"
    );
}

#[test]
fn should_not_let_a_plan_ask_for_less_than_the_profile_requires() {
    let settings = under("cautious", &[]);
    assert_eq!(
        settings.protection_mode_for(Some(ProtectionMode::Off)),
        ProtectionMode::Require,
        "ADR-0810: the strictest requirement in force applies, whichever source states it"
    );
}

#[test]
fn should_let_a_plan_ask_for_more_than_the_profile_requires() {
    let settings = under("interactive", &[]);
    assert_eq!(
        settings.protection_mode_for(Some(ProtectionMode::Require)),
        ProtectionMode::Require,
        "Appendix H.5: a plan may impose stricter requirements than a profile"
    );
}

#[test]
fn should_disable_opaque_actions_under_every_profile() {
    for profile in Profile::ALL {
        let settings = under(
            profile.as_str(),
            &[("change.allow_opaque_actions", Value::Bool(true))],
        );
        assert!(
            !settings.allow_opaque_actions(),
            "Appendix H.1 – H.3: opaque actions are disabled under `{}`",
            profile.as_str()
        );
    }
}

#[test]
fn should_require_acknowledgement_from_moderate_risk_under_the_cautious_profile() {
    let settings = under("cautious", &[]);
    assert_eq!(settings.risk_gate(), Some(RiskClass::Moderate));
    assert!(!settings.requires_acknowledgement(RiskClass::Low));
    for class in [
        RiskClass::Moderate,
        RiskClass::Unknown,
        RiskClass::High,
        RiskClass::Critical,
    ] {
        assert!(
            settings.requires_acknowledgement(class),
            "Appendix H.2: `moderate+` gates {}",
            class.as_str()
        );
    }
}

#[test]
fn should_gate_from_high_risk_without_a_profile() {
    let settings = ChangeSettings::defaults();
    assert_eq!(settings.risk_gate(), Some(RiskClass::High));
    for class in [RiskClass::Low, RiskClass::Moderate, RiskClass::Unknown] {
        assert!(!settings.requires_acknowledgement(class));
    }
    assert!(settings.requires_acknowledgement(RiskClass::High));
    assert!(settings.requires_acknowledgement(RiskClass::Critical));
}

#[test]
fn should_restore_the_high_risk_gate_a_configuration_switched_off_under_a_profile() {
    let lenient = [
        ("change.high_risk_requires_ack", Value::Bool(false)),
        ("change.critical_risk_requires_ack", Value::Bool(false)),
    ];
    let (unprofiled, _) = ChangeSettings::read(&source(&lenient));
    assert_eq!(
        unprofiled.risk_gate(),
        None,
        "the operator may switch it off"
    );
    let settings = under("interactive", &lenient);
    assert!(
        settings.high_risk_requires_ack() && settings.critical_risk_requires_ack(),
        "Appendix H.1: the profile's risk gate is `high+`, and it tightens the configuration"
    );
    assert_eq!(settings.risk_gate(), Some(RiskClass::High));
}

#[test]
fn should_not_prompt_under_the_scripted_profile() {
    assert!(ChangeSettings::defaults().prompts());
    assert!(
        !under("scripted", &[]).prompts(),
        "Appendix H.4: no prompts"
    );
    assert!(under("cautious", &[]).prompts());
}

#[test]
fn should_carry_the_fleet_strategy_for_the_planner_to_apply() {
    assert_eq!(
        under("fleet", &[]).profile_strategy(),
        Some("canary 1 then batch 10%")
    );
    assert_eq!(under("cautious", &[]).profile_strategy(), None);
    assert_eq!(ChangeSettings::defaults().profile_strategy(), None);
}

#[test]
fn should_report_a_profile_nobody_defined_and_apply_none() {
    let (settings, problems) =
        ChangeSettings::read(&source(&[("change.profile", Value::string("paranoid"))]));
    assert_eq!(
        problems.len(),
        1,
        "the unknown profile is reported: {problems:?}"
    );
    assert!(
        problems[0].message().contains("change.profile")
            && problems[0].message().contains("paranoid"),
        "the report names the key and the value: {}",
        problems[0].message()
    );
    assert_eq!(settings.profile(), None);
}

#[test]
fn should_expand_the_profile_into_the_values_in_force_and_where_each_came_from() {
    let settings = under("cautious", &[("recovery.retention", Value::string("168h"))]);
    let expansion = settings.profile_expansion();
    let row = |key: &str| {
        expansion
            .iter()
            .find(|row| row.key == key)
            .unwrap_or_else(|| panic!("`{key}` is in the expansion: {expansion:?}"))
    };

    let protection = row("change.default_protection");
    assert_eq!(protection.configured.as_deref(), Some("prefer"));
    assert_eq!(protection.profile, "require");
    assert_eq!(protection.effective.as_deref(), Some("require"));
    assert_eq!(protection.source, Source::Profile(Profile::Cautious));

    let retention = row("recovery.retention");
    assert_eq!(
        retention.source,
        Source::Configuration,
        "the operator's longer retention stands, and the expansion says so"
    );
    assert_eq!(retention.effective, retention.configured.clone());

    let gate = row("risk gate");
    assert_eq!(gate.configured, None, "the risk gate is not a §53 key");
    assert_eq!(gate.effective.as_deref(), Some("moderate+"));

    let strategy = row("change.default_strategy");
    assert_eq!(
        strategy.effective, None,
        "the strategy is the planner's to apply, and the expansion does not claim it is in force"
    );

    assert!(
        ChangeSettings::defaults().profile_expansion().is_empty(),
        "no profile, nothing to expand"
    );
}

#[test]
fn should_name_the_profile_among_the_rendered_settings() {
    let entries = under("cautious", &[]).entries();
    assert!(entries.contains(&("change.profile", "cautious".to_owned())));
    assert!(
        ChangeSettings::defaults()
            .entries()
            .contains(&("change.profile", "none".to_owned()))
    );
}

// --- ADR-0834: a plan may lower the built-in default, never what an operator configured -------

#[test]
fn should_let_a_plan_lower_the_built_in_default_when_nothing_was_configured() {
    let (settings, problems) = ChangeSettings::read(&source(&[]));
    assert!(problems.is_empty());
    assert_eq!(
        settings.protection_mode_for(Some(ProtectionMode::Off)),
        ProtectionMode::Off,
        "§17.3: a plan MAY override configuration, and no configuration was written"
    );
    let policy = settings.policy_for(Some(ProtectionMode::Off));
    assert_eq!(policy.mode(), ProtectionMode::Off);
    assert_eq!(policy.raised(), None, "the plan got what it asked for");
    assert_eq!(
        settings.protection_mode_for(None),
        ProtectionMode::Prefer,
        "§17.1: a plan that states nothing gets the built-in default"
    );
}

#[test]
fn should_keep_a_configured_mode_a_plan_asks_to_lower_and_say_so() {
    let settings = ChangeSettings::from_settings(&source(&[(
        "change.default_protection",
        Value::string("prefer"),
    )]))
    .expect("prefer is a mode");
    let policy = settings.policy_for(Some(ProtectionMode::Off));
    assert_eq!(
        policy.mode(),
        ProtectionMode::Prefer,
        "§53: configuration the operator wrote MUST NOT be weakened, even when it says the default"
    );
    assert_eq!(
        policy.raised(),
        Some(ProtectionMode::Off),
        "the plan asked for less and is told it got more"
    );
    assert!(
        policy
            .raising_note()
            .is_some_and(|note| note.contains("off") && note.contains("prefer")),
        "the note names both modes: {:?}",
        policy.raising_note()
    );
    assert_eq!(
        policy.narrowed(),
        None,
        "a raise is not a narrowing (ADR-0815)"
    );
}

#[test]
fn should_keep_the_mode_a_profile_brings_when_a_plan_asks_to_lower_it() {
    let settings = under("interactive", &[]);
    assert_eq!(
        settings.protection_mode_for(Some(ProtectionMode::Off)),
        ProtectionMode::Prefer,
        "Appendix H.5: a profile is configuration, and a plan does not lower it"
    );
}

// --- ADR-0815: only a request the mode in force does not contain is a narrowing ---------------

#[test]
fn should_not_call_a_stricter_mode_than_the_plan_asked_for_a_narrowing() {
    let settings = ChangeSettings::from_settings(&source(&[(
        "change.default_protection",
        Value::string("require"),
    )]))
    .expect("require is a mode");
    for asked in [ProtectionMode::Off, ProtectionMode::Prefer] {
        let policy = settings.policy_for(Some(asked));
        assert_eq!(policy.mode(), ProtectionMode::Require);
        assert_eq!(
            policy.narrowed(),
            None,
            "ADR-0815: `require` contains everything `{asked}` asked for"
        );
        assert_eq!(policy.narrowing_note(), None);
        assert_eq!(policy.raised(), Some(asked));
    }
}

#[test]
fn should_not_call_prefer_under_maximize_a_narrowing() {
    let settings = ChangeSettings::from_settings(&source(&[(
        "change.default_protection",
        Value::string("maximize"),
    )]))
    .expect("maximize is a mode");
    let policy = settings.policy_for(Some(ProtectionMode::Prefer));
    assert_eq!(policy.mode(), ProtectionMode::Maximize);
    assert_eq!(
        policy.narrowed(),
        None,
        "§17.2: `maximize` attempts everything `prefer` would, and more"
    );
}

#[test]
fn should_not_call_the_one_narrowing_a_raise() {
    let settings = ChangeSettings::from_settings(&source(&[(
        "change.default_protection",
        Value::string("require"),
    )]))
    .expect("require is a mode");
    let policy = settings.policy_for(Some(ProtectionMode::Maximize));
    assert_eq!(policy.narrowed(), Some(ProtectionMode::Maximize));
    assert_eq!(
        policy.raised(),
        None,
        "ADR-0815: `require` does not attempt every mechanism, so this lost something"
    );
}
