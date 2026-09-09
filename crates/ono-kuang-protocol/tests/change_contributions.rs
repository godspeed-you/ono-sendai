//! The wire surface of v0.6 §48: the eleven capabilities of §48.3, the five new contribution
//! types of §48.2, and the compatibility rule that none of them may break a package written
//! before they existed.
//!
//! Every assertion here is about what a document says or what a capability carries — the
//! declarations a host reads **before** running anything, which is where §48.4 puts its refusals.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_kuang_protocol::{
    Capability, ChangeViewDocument, CommandDocument, ConsentClass, Elevation, Enforcement,
    ImpactProviderDocument, Manifest, PermissionSet, RecoveryProviderDocument, Risk,
    RiskRuleDocument, VerificationProviderDocument,
};

// --- the capabilities of section 48.3 ----------------------------------------------------------

#[test]
fn should_resolve_every_change_and_recovery_capability_when_looked_up_by_id() {
    // §48.3 lists the family names, and an id a manifest may write is an id the registry
    // resolves. An unknown one is `package.invalid` (§31.16), so a family that never reached the
    // registry would silently be a capability nobody could ask for.
    for id in [
        "change.plan.read",
        "change.plan.contribute",
        "change.action.execute",
        "verification.observe",
        "recovery.discover",
        "recovery.prepare",
        "recovery.restore",
        "recovery.cleanup",
        "recovery.estimate-cost",
        "recovery.quiesce",
        "recovery.transaction",
    ] {
        assert!(
            Capability::from_id(id).is_some(),
            "v0.6 §48.3 names `{id}` and the registry does not carry it"
        );
    }
}

#[test]
fn should_carry_destructive_risk_and_required_elevation_for_restore_when_read() {
    // §43.4: recovery may need stronger privileges than the original mutation. §13.6 and §14.6:
    // restoring is the operation that can lose the most — newer snapshots, clones, and
    // everything written since. Both facts are in the family rather than in a comment.
    assert_eq!(Capability::RecoveryRestore.risk(), Risk::Destructive);
    assert_eq!(
        Capability::RecoveryRestore.elevation(),
        Elevation::Required,
        "§43.4: restoring may need a stronger privilege than the mutation it undoes"
    );
}

#[test]
fn should_keep_describing_impact_separate_from_executing_a_change_when_risks_are_compared() {
    // §48.4, read off the registry: the three change families a describing package holds carry
    // no mutating risk, and the one that authorises a change does. A grant for one cannot be
    // spent on another because they are not one family.
    for reader in [
        Capability::ChangePlanRead,
        Capability::ChangePlanContribute,
        Capability::VerificationObserve,
    ] {
        assert!(
            matches!(reader.risk(), Risk::Read | Risk::Observe),
            "§48.4: `{}` describes and must not authorise a mutation",
            reader.id()
        );
    }
    assert_eq!(
        Capability::ChangeActionExecute.risk(),
        Risk::Mutate,
        "§48.3: `change.action.execute` is the one that carries the authority"
    );
}

#[test]
fn should_mark_the_two_application_scopes_advisory_when_scope_keys_are_read() {
    // §31.16: "A scope that cannot be enforced reliably MUST NOT be offered as if it were a
    // security boundary." An application name is resolved by the plugin inside the system it
    // fronts, so the host records it and cannot check it.
    for family in [Capability::RecoveryQuiesce, Capability::RecoveryTransaction] {
        let keys = family.scope_keys();
        assert_eq!(keys.len(), 1, "{} declares one scope key", family.id());
        assert_eq!(keys[0].name, "applications");
        assert_eq!(
            keys[0].enforcement,
            Enforcement::Advisory,
            "§31.16: the host cannot compare an application name a plugin resolved for itself"
        );
    }
}

#[test]
fn should_enforce_every_recovery_domain_and_scope_key_at_the_broker_when_scope_keys_are_read() {
    // The other side of the same rule: a domain kind and a resolved persistence object both
    // arrive in the call's parameters, so both are checked before the operation happens.
    for family in [
        Capability::RecoveryDiscover,
        Capability::RecoveryPrepare,
        Capability::RecoveryRestore,
        Capability::RecoveryCleanup,
        Capability::RecoveryEstimateCost,
    ] {
        for key in family.scope_keys() {
            assert_eq!(
                key.enforcement,
                Enforcement::Broker,
                "`{}.{}` is a value the broker compares before the call",
                family.id(),
                key.name
            );
        }
    }
}

#[test]
fn should_place_restore_in_the_destructive_consent_class_when_classified() {
    // K11P §7.5 reserves class E for authority no unattended acceptance may enable, which is
    // where §48.4 and §43.4 put restoring.
    assert_eq!(
        ono_kuang_protocol::consent_class(Capability::RecoveryRestore, None),
        ConsentClass::Destructive
    );
}

#[test]
fn should_never_offer_restore_as_a_safe_default_when_a_profile_is_derived() {
    // Appendix H's `profiles.rules`: a safe default is decided at install or automatic, below
    // the `mutate` risk, over class A or B capabilities. `recovery.restore` is class E at
    // `destructive` risk, so it fails all three tests, and the host's derivation puts it in no
    // default profile without anything having to name it.
    let manifest = Manifest::parse(&manifest_requiring("recovery.restore")).expect("parses");
    let permissions = PermissionSet::of(&manifest);
    let descriptor = permissions
        .for_capability(Capability::RecoveryRestore)
        .next()
        .expect("a derived descriptor covers the declared capability");
    assert!(
        !descriptor.is_safe_default(),
        "Appendix H: a destructive class E capability can never be a safe default"
    );
    assert!(
        !descriptor.recommended,
        "K11P §9.1: `recommended` consists only of safe defaults"
    );
    let id = descriptor.id.clone();
    for profile in &permissions.profiles {
        if profile.name == "minimal" || profile.name == "recommended" {
            assert!(
                !profile.permissions.contains(&id),
                "`{}` must not carry `recovery.restore` (Appendix H, K11P §9.1)",
                profile.name
            );
        }
    }
}

#[test]
fn should_offer_reading_a_plan_as_a_safe_default_when_a_profile_is_derived() {
    // The contrast that makes the previous test mean something: an observation-class read is
    // exactly what a recommended profile may carry, so the rule is discriminating rather than
    // uniformly refusing.
    let manifest = Manifest::parse(&manifest_requiring("change.plan.read")).expect("parses");
    let permissions = PermissionSet::of(&manifest);
    let descriptor = permissions
        .for_capability(Capability::ChangePlanRead)
        .next()
        .expect("a derived descriptor covers the declared capability");
    assert!(descriptor.is_safe_default());
}

#[test]
fn should_refuse_a_capability_id_v0_6_does_not_define_when_parsed() {
    let error = "recovery.obliterate"
        .parse::<Capability>()
        .expect_err("no such family");
    assert_eq!(
        error.code(),
        ono_kuang_protocol::KuangErrorCode::PackageInvalid
    );
}

// --- the contribution documents of section 48.2 -------------------------------------------------

const RECOVERY_DOCUMENT: &str = "\
recovery_providers:
  - id: dev.example.pg.recovery-provider.database
    summary: Point-in-time protection for PostgreSQL databases.
    domain_kinds: [postgres-database]
    asset_type: database-dump
    consistency: application-consistent
    restore_methods: [provider-native-restore]
    shares_failure_domain: false
    capabilities: [recovery.discover, recovery.prepare, recovery.restore, recovery.cleanup, recovery.estimate-cost, recovery.quiesce]
    transaction:
      resources: [postgres-database]
      guarantee: statements inside one BEGIN either all commit or all roll back
";

#[test]
fn should_read_a_recovery_provider_document_as_section_48_5_describes_one() {
    let document = RecoveryProviderDocument::parse(RECOVERY_DOCUMENT).expect("reads");
    let provider = &document.recovery_providers[0];
    assert_eq!(provider.domain_kinds, vec!["postgres-database".to_owned()]);
    assert_eq!(provider.asset_type, "database-dump");
    assert_eq!(provider.consistency, "application-consistent");
    assert_eq!(
        provider.restore_methods,
        vec!["provider-native-restore".to_owned()]
    );
    assert!(
        !provider.shares_failure_domain,
        "§11.5: whether the asset shares the failure domain of what it protects is declared"
    );
    let transaction = provider.transaction.as_ref().expect("§27.1's declaration");
    assert_eq!(transaction.resources, vec!["postgres-database".to_owned()]);
}

#[test]
fn should_refuse_a_recovery_provider_document_with_an_undeclared_key_when_parsed() {
    // §31.68 wants a package's contributions readable without running it, and a typo that is
    // silently dropped is a declaration the reader believes and the host never saw.
    let error = RecoveryProviderDocument::parse("recovery_providers: []\nrecovery_providerz: []\n")
        .expect_err("an unknown key is refused");
    assert_eq!(
        error.code(),
        ono_kuang_protocol::KuangErrorCode::PackageInvalid
    );
}

#[test]
fn should_default_a_recovery_provider_to_no_transaction_when_it_declares_none() {
    // §27.1's declaration is the exception rather than the rule: most providers state no
    // atomicity, and §27.2 forbids the word once a second boundary is involved.
    let document = RecoveryProviderDocument::parse(
        "recovery_providers:\n  - id: dev.example.p.recovery-provider.files\n    \
         summary: Copies files.\n    domain_kinds: [directory]\n    asset_type: file-archive\n    \
         consistency: byte-consistent\n",
    )
    .expect("reads");
    let provider = &document.recovery_providers[0];
    assert!(provider.transaction.is_none());
    assert!(provider.restore_methods.is_empty());
    assert!(provider.capabilities.is_empty());
    assert!(!provider.shares_failure_domain);
}

#[test]
fn should_read_an_impact_provider_document_with_its_confidence_ceiling() {
    let document = ImpactProviderDocument::parse(
        "impact_providers:\n  - id: dev.example.pg.impact-provider.database\n    \
         summary: Relates databases to the services that read them.\n    \
         object_types: [dev.example.pg.database/1]\n    relations: [reads-database]\n    \
         confidence_ceiling: possible\n",
    )
    .expect("reads");
    let provider = &document.impact_providers[0];
    assert_eq!(provider.relations, vec!["reads-database".to_owned()]);
    assert_eq!(
        provider.confidence_ceiling.as_deref(),
        Some("possible"),
        "§8.1: a declared ceiling is one the host lowers to and never raises from"
    );
}

#[test]
fn should_read_a_verification_provider_document_with_one_equivalence_domain_per_check() {
    // §25.3 forbids "rollback successful" without a scope, so each check kind names exactly the
    // scope it is evidence about.
    let document = VerificationProviderDocument::parse(
        "verification_providers:\n  - id: dev.example.pg.verification-provider.db\n    \
         summary: Checks a database came back.\n    checks:\n      \
         - {kind: accepts-connections, equivalence: runtime-state, summary: It answers.}\n      \
         - {kind: row-counts-match, equivalence: persistent-state, summary: The rows came back.}\n",
    )
    .expect("reads");
    let checks = &document.verification_providers[0].checks;
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[0].equivalence, "runtime-state");
    assert_eq!(checks[1].equivalence, "persistent-state");
}

#[test]
fn should_read_a_risk_rule_document_with_its_dimension_and_its_ceiling() {
    let document = RiskRuleDocument::parse(
        "risk_rules:\n  - rule_id: dev.example.pg.risk.restart\n    dimension: downtime\n    \
         emits: high\n    summary: Restoring interrupts every connected session.\n",
    )
    .expect("reads");
    let rule = &document.risk_rules[0];
    assert_eq!(rule.dimension, "downtime");
    assert_eq!(rule.emits, "high");
}

#[test]
fn should_read_a_change_view_document_with_the_plan_states_it_renders() {
    let document = ChangeViewDocument::parse(
        "change_views:\n  - id: dev.example.pg.change-view.plan\n    summary: A database plan.\n    \
         mode: static\n    plan_states: [sealed]\n    fallback: one line per action\n",
    )
    .expect("reads");
    let view = &document.change_views[0];
    assert_eq!(view.mode, "static");
    assert_eq!(view.plan_states, vec!["sealed".to_owned()]);
}

// --- compatibility: nothing that worked before may stop working --------------------------------

#[test]
fn should_load_a_manifest_written_before_v0_6_unchanged() {
    // The constraint that matters most: a `kuang-package/1` manifest from 0.4.3 declares none of
    // the five new contribution paths, and it still parses to a manifest with all of them absent.
    let manifest = Manifest::parse(
        "format: kuang-package/1\npackage:\n  id: dev.example.old\n  name: old\n  \
         version: 0.4.3\n  description: A package from before v0.6.\n  publisher: dev.example\n  \
         license: MIT\ncompatibility:\n  kuang_api: \">=11.1 <12\"\n  ono_language: \">=0.2\"\n  \
         platforms: [linux-amd64]\nroles: [provider]\ncontributions:\n  \
         commands: [contributions/commands.yaml]\nnetwork:\n  outbound: none\n",
    )
    .expect("a pre-v0.6 manifest still loads");
    let contributions = manifest.contributions.expect("the section it declared");
    assert_eq!(
        contributions.commands,
        Some(vec!["contributions/commands.yaml".to_owned()])
    );
    assert!(contributions.recovery_providers.is_none());
    assert!(contributions.impact_providers.is_none());
    assert!(contributions.verification_providers.is_none());
    assert!(contributions.risk_rules.is_none());
    assert!(contributions.change_views.is_none());
}

#[test]
fn should_read_an_action_declaring_only_the_old_effect_classes_when_parsed() {
    // v0.6 §0.1 leaves earlier specifications authoritative for what they define: a command
    // document written against the provider contract's loose `effects` list keeps working, and
    // gains an empty `effect_classes` rather than a refusal.
    let document = CommandDocument::parse(
        "commands:\n  - id: dev.example.p.command.restart\n    verb: restart\n    \
         target: thing\n    summary: Restarts it.\n    output: stream<int>\n    \
         argument_mode: expression\n    risk: mutate\n    examples: []\n    \
         capabilities: [provider.mutate]\n    action:\n      mutates: true\n      \
         effects: [restarts-workload]\n",
    )
    .expect("an action declaring only the loose effect list still reads");
    let action = document.commands[0].action.as_ref().expect("the action");
    assert_eq!(action.effects, vec!["restarts-workload".to_owned()]);
    assert!(
        action.effect_classes.is_empty(),
        "v0.6 §0.1: the richer form is additive, and its absence is not a failure"
    );
}

#[test]
fn should_read_an_action_declaring_the_richer_effect_classes_when_parsed() {
    // What §8.1 and Appendix A.1 need before a contributed action's effects can enter the
    // coverage algorithm at all.
    let document = CommandDocument::parse(
        "commands:\n  - id: dev.example.p.command.restart\n    verb: restart\n    \
         target: thing\n    summary: Restarts it.\n    output: stream<int>\n    \
         argument_mode: expression\n    risk: mutate\n    examples: []\n    \
         capabilities: [provider.mutate]\n    action:\n      mutates: true\n      \
         effect_classes:\n        - domain: process-runtime\n          kind: replace\n          \
         confidence: guaranteed\n          explanation: the worker set is new after a restart\n          \
         irreversible: true\n",
    )
    .expect("reads");
    let effect = &document.commands[0]
        .action
        .as_ref()
        .expect("the action")
        .effect_classes[0];
    assert_eq!(effect.domain, "process-runtime");
    assert_eq!(effect.confidence, "guaranteed");
    assert!(
        effect.irreversible,
        "§33.1: process identity does not come back"
    );
    assert!(
        effect.compensation.is_none(),
        "§27.4: null means the package offers no inverse action, which is not the same as none being needed"
    );
}

#[test]
fn should_keep_an_empty_contribution_set_serialising_to_nothing_when_a_package_contributes_none() {
    // Every new contribution list is skipped when empty, so a package that contributes none of
    // them puts the same bytes on the wire it always did.
    let json = serde_json::to_value(ono_kuang_protocol::ContributionSet::default()).expect("json");
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn should_default_a_recovery_cost_to_estimated_when_the_wire_omits_the_field() {
    // §37.5 keeps an estimate labelled as one. A cost nobody measured must not arrive claiming
    // it was measured, so the default is the honest direction rather than `false`.
    let cost: ono_kuang_protocol::RecoveryCostWire =
        serde_json::from_value(serde_json::json!({})).expect("an empty cost reads");
    assert!(cost.estimated);
    assert!(!cost.requires_reboot);
    assert!(cost.initial_bytes.is_none());
}

#[test]
fn should_refuse_a_plan_contribution_that_states_the_plan_class_when_parsed() {
    // §19.2 makes the class the fold of its findings and nothing else. There is deliberately no
    // field by which a package could state one, and the closed shape makes inventing one a
    // protocol violation rather than a key that is quietly ignored.
    let refused = serde_json::from_value::<ono_kuang_protocol::PlanContributeParams>(
        serde_json::json!({"plan": "plan-1", "plan_class": "low"}),
    );
    assert!(
        refused.is_err(),
        "§19.2: nothing on this wire may set a plan's risk class directly"
    );
}

#[test]
fn should_mirror_the_recovery_scope_field_names_the_domain_type_uses_when_parsed() {
    // The wire shape is a mirror of `RecoveryScope`, and the protocol crate deliberately does
    // not depend on the crate that defines it. A field renamed on either side has to fail
    // somewhere; the closed shape is what makes it fail here.
    let scope: ono_kuang_protocol::RecoveryScopeWire = serde_json::from_value(serde_json::json!({
        "domain": "tank/var",
        "domain_kind": "zfs-dataset",
        "covers": ["/var/lib"],
        "host": "localhost",
    }))
    .expect("reads");
    assert_eq!(scope.domain_kind, "zfs-dataset");
    assert!(
        serde_json::from_value::<ono_kuang_protocol::RecoveryScopeWire>(serde_json::json!({
            "domain": "tank/var", "domain_kind": "zfs-dataset", "host": "localhost",
            "dataset": "tank/var",
        }))
        .is_err(),
        "a field the domain type does not carry is refused rather than dropped"
    );
}

/// A `kuang-package/2` manifest requiring exactly one capability, for the profile derivation.
fn manifest_requiring(capability: &str) -> String {
    format!(
        "format: kuang-package/2\npackage:\n  id: dev.example.pg\n  name: pg\n  version: 0.1.0\n  \
         description: A database package.\n  publisher: dev.example\n  license: MIT\n\
         compatibility:\n  kuang_api: \">=11.1 <12\"\n  ono_language: \">=0.2\"\n  \
         platforms: [linux-amd64]\nroles: [provider]\ncapabilities:\n  required:\n    \
         - {capability}\nnetwork:\n  outbound: none\n"
    )
}
