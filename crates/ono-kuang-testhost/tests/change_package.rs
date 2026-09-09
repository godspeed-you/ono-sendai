//! The test host validates a package's change and recovery contributions before it is loaded
//! (v0.6 §48.2, §48.4, §39.2, §27.3), and holds the wire shapes against the domain types they
//! mirror.
//!
//! §48.4's refusals are all checkable on disk, so they are checked on disk: a provider that
//! offers to restore while holding no destructive authority, one claiming a consistency class it
//! cannot own, and a transaction reaching past its own resources are problems a publisher meets
//! before a user does.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_kuang_testhost::check_change_package;

const RECOVERY: &str = "\
recovery_providers:
  - id: dev.example.pg.recovery-provider.database
    summary: Point-in-time protection for the databases this package fronts.
    domain_kinds: [postgres-database]
    asset_type: database-dump
    consistency: application-consistent
    restore_methods: [provider-native-restore]
    shares_failure_domain: false
    capabilities: [recovery.discover, recovery.prepare, recovery.restore, recovery.cleanup, recovery.estimate-cost, recovery.quiesce, recovery.transaction]
    transaction:
      resources: [postgres-database]
      guarantee: statements inside one BEGIN either all commit or all roll back
";

const IMPACT: &str = "\
impact_providers:
  - id: dev.example.pg.impact-provider.database
    summary: Relates a database to the places that read it.
    object_types: [dev.example.pg.database/1]
    relations: [reads-database]
    confidence_ceiling: possible
";

const VERIFICATION: &str = "\
verification_providers:
  - id: dev.example.pg.verification-provider.database
    summary: Checks a database came back, and says which scope that is about.
    checks:
      - {kind: accepts-connections, equivalence: runtime-state, summary: The database answers.}
      - {kind: row-counts-match, equivalence: persistent-state, summary: The rows came back.}
";

const RULES: &str = "\
risk_rules:
  - rule_id: dev.example.pg.risk.database-restart
    dimension: downtime
    emits: high
    summary: Restoring a database interrupts every session connected to it.
";

const VIEWS: &str = "\
change_views:
  - id: dev.example.pg.change-view.database-plan
    summary: Shows a database plan beside what it would cost to undo.
    mode: static
    plan_states: [sealed, recovery-planned]
    fallback: one line per action, with its recovery coverage
";

const TARGETS: &str = "\
targets:
  - name: database
    schema: dev.example.pg.database/1
    summary: One database this package answers for.
    identity_doc: The cluster identity and the database name.
";

const CAPABILITIES: &str = "  optional:\n    - recovery.discover\n    - recovery.prepare\n    \
                            - recovery.restore\n    - recovery.cleanup\n    \
                            - recovery.estimate-cost\n    - recovery.quiesce\n    \
                            - recovery.transaction\n    - change.plan.read\n    \
                            - change.plan.contribute\n    - verification.observe\n    - ui.view\n";

/// A package directory whose manifest points at the documents given.
fn package(recovery: Option<&str>, capabilities: &str) -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    scratch.write("contributions/targets.yaml", TARGETS);
    scratch.write("contributions/impact.yaml", IMPACT);
    scratch.write("contributions/verification.yaml", VERIFICATION);
    scratch.write("contributions/rules.yaml", RULES);
    scratch.write("contributions/views.yaml", VIEWS);
    let mut declared = String::from(
        "  targets: [contributions/targets.yaml]\n  \
         impact_providers: [contributions/impact.yaml]\n  \
         verification_providers: [contributions/verification.yaml]\n  \
         risk_rules: [contributions/rules.yaml]\n  \
         change_views: [contributions/views.yaml]\n",
    );
    if let Some(document) = recovery {
        scratch.write("contributions/recovery.yaml", document);
        declared.push_str("  recovery_providers: [contributions/recovery.yaml]\n");
    }
    scratch.write(
        "manifest.yaml",
        format!(
            "format: kuang-package/1\n\
             package:\n  \
               id: dev.example.pg\n  \
               name: pg\n  \
               version: 0.1.0\n  \
               description: Fronts a database.\n  \
               publisher: dev.example\n  \
               license: MIT\n\
             compatibility:\n  \
               kuang_api: \">=11.1 <12\"\n  \
               ono_language: \">=0.2\"\n  \
               platforms: [linux-amd64, linux-arm64]\n\
             runtime:\n  \
               kind: native-process\n  \
               entry: runtime/pg\n  \
               memory_max: 64MiB\n  \
               cpu_budget: interactive\n  \
               startup: lazy\n\
             roles: [provider]\n\
             capabilities:\n{capabilities}\
             contributions:\n{declared}\
             network:\n  outbound: none\n"
        ),
    );
    scratch
}

#[test]
fn should_report_every_contribution_type_when_the_package_declares_all_five() {
    // §48.2 lists six types; `ActionProvider` is a command's own declaration and the other five
    // are documents. A package contributing all of them is readable without being run (§31.68).
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report.problems.is_empty(),
        "problems: {:?}",
        report.problems
    );
    assert_eq!(
        report.recovery_providers,
        vec!["dev.example.pg.recovery-provider.database".to_owned()]
    );
    assert_eq!(
        report.impact_providers,
        vec!["dev.example.pg.impact-provider.database".to_owned()]
    );
    assert_eq!(
        report.verification_providers,
        vec!["dev.example.pg.verification-provider.database".to_owned()]
    );
    assert_eq!(
        report.risk_rules,
        vec!["dev.example.pg.risk.database-restart".to_owned()]
    );
    assert_eq!(
        report.change_views,
        vec!["dev.example.pg.change-view.database-plan".to_owned()]
    );
}

#[test]
fn should_report_that_a_provider_can_restore_when_it_declares_the_destructive_capability() {
    // §62.1: a candidate nobody can use is snapshot theatre. The report answers the question
    // directly rather than leaving a publisher to infer it from a capability list.
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(report.can_restore);
}

#[test]
fn should_refuse_a_provider_that_offers_a_restore_method_without_destructive_authority() {
    // §48.4 at load. §43.4 lets recovery need a stronger privilege than the mutation it undoes,
    // and §13.6 and §14.6 make restoring the operation that can lose the most; a candidate an
    // operator relies on and a restore denied when they need it is what this refusal prevents.
    let document = RECOVERY.replace(", recovery.restore", "");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report.problems.iter().any(|problem| {
            problem.contains("offers to restore") && problem.contains("destructive")
        }),
        "§48.4: the refusal names the provider and the missing authority, got {:?}",
        report.problems
    );
    assert!(!report.can_restore);
}

#[test]
fn should_refuse_a_provider_claiming_application_consistency_without_quiesce() {
    // §39.2: Ono MUST NOT independently label a snapshot `APPLICATION_CONSISTENT` unless an
    // application-aware provider asserts the guarantee, and §16.4 says the provider must own the
    // claim. Owning it means being able to quiesce.
    let document = RECOVERY.replace(", recovery.quiesce", "");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("application-consistent")
                && problem.contains("recovery.quiesce")),
        "§39.2: the refusal says which capability owns the claim, got {:?}",
        report.problems
    );
}

#[test]
fn should_report_the_consistency_a_provider_can_actually_own_when_it_cannot_quiesce() {
    // The ceiling reflects §39.2 rather than the package's ambition: a provider without quiesce
    // may claim `crash-consistent`, which PostgreSQL's own WAL semantics can recover from
    // (§39.2), and the report says so instead of the class the document asked for.
    let document = RECOVERY.replace(", recovery.quiesce", "").replace(
        "consistency: application-consistent",
        "consistency: crash-consistent",
    );
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report.problems.is_empty(),
        "problems: {:?}",
        report.problems
    );
    assert_eq!(
        report.consistency_ceiling.as_deref(),
        Some("crash-consistent")
    );
}

#[test]
fn should_refuse_a_transaction_that_spans_a_resource_the_provider_does_not_own() {
    // §27.1 scopes atomicity to the provider's own resource scope, §27.2 forbids the word once a
    // second boundary is involved, and §27.3 makes generic distributed two-phase commit an
    // explicit non-goal. A declaration reaching a ZFS dataset from a database provider is the
    // exact shape §27.3 refuses.
    let document = RECOVERY.replace(
        "resources: [postgres-database]",
        "resources: [postgres-database, zfs-dataset]",
    );
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("zfs-dataset") && problem.contains("27.3")),
        "§27.3: the refusal names the resource outside the provider's scope, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_transaction_declared_without_the_transaction_capability() {
    // §12.2 makes `recovery.transaction` the capability that carries the guarantee of §27.1.
    // Stating an atomicity without it is a claim with no mechanism behind it.
    let document = RECOVERY.replace(", recovery.transaction", "");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("recovery.transaction")),
        "§12.2: a stated atomicity without `recovery.transaction` is refused, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_provider_id_outside_the_package_namespace() {
    let document = RECOVERY.replace("dev.example.pg.recovery-provider", "ono.recovery-provider");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("31.5")),
        "spec §31.5: `ono.*` belongs to the project, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_consistency_class_v0_6_does_not_define() {
    let document = RECOVERY.replace("application-consistent", "perfectly-consistent");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("perfectly-consistent")),
        "§11.3 is a closed list, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_restore_method_outside_appendix_c1() {
    let document = RECOVERY.replace("provider-native-restore", "wave-a-wand");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("wave-a-wand")),
        "Appendix C.1 is the list of methods, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_declared_capability_outside_the_seven_of_section_12_2() {
    let document = RECOVERY.replace("recovery.discover,", "filesystem.write,");
    let scratch = package(Some(&document), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("filesystem.write")),
        "§12.2 fixes the recovery capabilities, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_risk_rule_outside_the_package_namespace() {
    // §19.2: a contributed rule is a rule, and it is inspectable exactly as a built-in one is.
    // A rule id in another namespace is a rule nobody can attribute.
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    scratch.write(
        "contributions/rules.yaml",
        RULES.replace("dev.example.pg.risk", "ono.risk").as_str(),
    );
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("namespaced")),
        "got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_risk_rule_naming_a_dimension_v0_6_does_not_define() {
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    scratch.write(
        "contributions/rules.yaml",
        RULES
            .replace("dimension: downtime", "dimension: vibes")
            .as_str(),
    );
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("vibes")),
        "§19.1 lists ten dimensions, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_verification_check_naming_an_equivalence_domain_outside_section_25_1() {
    // §25.3 forbids "rollback successful" without a scope, and §25.1 names the three scopes. A
    // fourth would be a claim with no defined meaning.
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    scratch.write(
        "contributions/verification.yaml",
        VERIFICATION
            .replace("equivalence: runtime-state", "equivalence: everything")
            .as_str(),
    );
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("25.1")),
        "got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_an_impact_provider_relating_a_type_nothing_carries() {
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    scratch.write(
        "contributions/impact.yaml",
        IMPACT
            .replace("dev.example.pg.database/1", "dev.example.pg.phantom/1")
            .as_str(),
    );
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("phantom")),
        "an impact provider that names a type nothing carries cannot draw an edge to it, got {:?}",
        report.problems
    );
}

#[test]
fn should_refuse_a_change_view_for_a_state_a_plan_cannot_be_in() {
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    scratch.write(
        "contributions/views.yaml",
        VIEWS
            .replace(
                "plan_states: [sealed, recovery-planned]",
                "plan_states: [almost-done]",
            )
            .as_str(),
    );
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("almost-done")),
        "§4.1's states are the ones a plan can be in, got {:?}",
        report.problems
    );
}

#[test]
fn should_say_a_package_may_not_execute_when_it_only_describes_and_contributes() {
    // §48.4 as a fact the report states rather than one a reader has to derive: this package
    // holds `change.plan.read` and `change.plan.contribute` and executes nothing.
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        !report.may_execute,
        "§48.4: a plugin that can describe impact does not thereby gain permission to execute"
    );
}

#[test]
fn should_say_a_package_may_execute_only_when_it_requests_the_capability_that_authorises_it() {
    let capabilities = format!("{CAPABILITIES}    - change.action.execute\n");
    let scratch = package(Some(RECOVERY), &capabilities);
    let report = check_change_package(scratch.path());
    assert!(report.may_execute);
}

#[test]
fn should_say_restoring_never_reaches_a_package_under_the_default_policy() {
    // §31.19's deny-by-default floor, asserted for the most consequential family v0.6 defines.
    let scratch = package(Some(RECOVERY), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(
        !report.restore_by_default,
        "§31.19 and Appendix H: `recovery.restore` is granted, never assumed"
    );
}

#[test]
fn should_say_a_protection_claim_can_never_land_when_the_package_asks_for_no_capability() {
    // A provider declaring everything and requesting nothing would offer candidates that no
    // call could ever act on. The report says so rather than reporting a healthy package.
    let scratch = package(Some(RECOVERY), "  optional:\n    - ui.view\n");
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("recovery.discover")),
        "got {:?}",
        report.problems
    );
    assert!(!report.can_restore);
}

#[test]
fn should_say_a_package_contributes_nothing_to_a_plan_when_it_declares_no_document() {
    let scratch = ono_testkit::scratch();
    scratch.write(
        "manifest.yaml",
        "format: kuang-package/1\npackage:\n  id: dev.example.pg\n  name: pg\n  version: 0.1.0\n  \
         description: Fronts a database.\n  publisher: dev.example\n  license: MIT\n\
         compatibility:\n  kuang_api: \">=11.1 <12\"\n  ono_language: \">=0.2\"\n  \
         platforms: [linux-amd64]\nroles: [provider]\nnetwork:\n  outbound: none\n",
    );
    let report = check_change_package(scratch.path());
    assert!(
        report
            .problems
            .iter()
            .any(|problem| problem.contains("contributes")),
        "got {:?}",
        report.problems
    );
}

#[test]
fn should_report_a_document_that_does_not_read_rather_than_ignoring_it() {
    let scratch = package(Some("recovery_providers: not-a-list\n"), CAPABILITIES);
    let report = check_change_package(scratch.path());
    assert!(!report.problems.is_empty());
    assert!(report.recovery_providers.is_empty());
}
