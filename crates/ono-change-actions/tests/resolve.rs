#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! Turning an intent into plan actions: §6.1's nine facts, over frozen targets.

mod support;

use std::sync::Arc;

use ono_change_actions::registry::RebootRequirement;
use ono_change_core::{
    ChangeProvider, EffectConfidence, EffectDomain, EffectKind, Execution, FrozenTarget,
    Idempotency, Intent, PlanAction, PlanFragment, PreconditionKind, Support, VerificationClass,
};
use ono_value::Value;
use support::{FakeObserver, FakeProvider, change_provider, service_provider};

fn intent(text: &str) -> Intent {
    Intent::new(text, text)
}

fn resolve(text: &str) -> PlanFragment {
    let (_, change) = service_provider();
    change
        .resolve(&intent(text), &[])
        .unwrap_or_else(|error| panic!("`{text}` should resolve: {}", error.message()))
}

fn only_action(fragment: &PlanFragment) -> &PlanAction {
    assert_eq!(fragment.actions().len(), 1, "one target, one action");
    &fragment.actions()[0]
}

fn provider_for(target: &'static str, records: Vec<Value>) -> Arc<FakeProvider> {
    Arc::new(FakeProvider::new("test.provider", &[target]).holding(records))
}

fn resolve_on(
    target: &'static str,
    observer: FakeObserver,
    text: &str,
) -> Result<PlanFragment, ono_value::ErrorValue> {
    let change = change_provider(provider_for(target, Vec::new()), Arc::new(observer));
    change.resolve(&intent(text), &[])
}

// --- what a resolved action carries (§6.1) ---------------------------------------------------

#[test]
fn should_resolve_a_service_restart_into_one_action() {
    let fragment = resolve("restart service nginx");
    let action = only_action(&fragment);
    assert_eq!(action.target(), Some("nginx"));
    assert_eq!(action.summary(), "restart service nginx");
}

#[test]
fn should_carry_a_structured_provider_action_rather_than_a_command_line() {
    let fragment = resolve("restart service nginx");
    match only_action(&fragment).execution() {
        Execution::ProviderAction {
            provider,
            operation,
            ..
        } => {
            assert_eq!(provider.as_ref(), "linux.systemd");
            assert_eq!(
                operation.as_ref(),
                "ono.service.restart",
                "§2.17: a provider operation is an operation id and typed arguments"
            );
        }
        other => panic!("§2.17 forbids anything else here, and this is {other:?}"),
    }
}

#[test]
fn should_carry_the_recovery_semantics_the_contract_declares() {
    let fragment = resolve("restart service nginx");
    let declared = only_action(&fragment)
        .declared_recovery()
        .expect("§6.1 requires recovery semantics or their explicit absence");
    assert!(
        declared.contains("§33.2"),
        "§33.2: a restart restores the service and not process identity, in-memory state or \
         connections"
    );
}

#[test]
fn should_carry_the_idempotency_class_the_contract_declares() {
    assert_eq!(
        only_action(&resolve("restart service nginx")).idempotency(),
        Idempotency::Idempotent,
        "§41.1: restarting twice is restarting once"
    );
}

#[test]
fn should_mark_an_action_that_cannot_work_without_privilege() {
    assert!(
        only_action(&resolve("restart service nginx")).needs_privilege(),
        "§43.3: `restart service` is declared `elevated`"
    );
}

#[test]
fn should_carry_the_effects_the_contract_declares() {
    let fragment = resolve("restart service nginx");
    let action = only_action(&fragment);
    let domains: Vec<EffectDomain> = action.effects().iter().map(|e| e.domain()).collect();
    assert!(domains.contains(&EffectDomain::ProcessRuntime));
    assert!(
        domains.contains(&EffectDomain::NetworkRuntime),
        "§8.1's own POSSIBLE example: active client connections may be interrupted by a restart"
    );
}

#[test]
fn should_keep_the_restart_of_the_worker_set_expected_rather_than_guaranteed() {
    let fragment = resolve("restart service nginx");
    let runtime = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ProcessRuntime)
        .expect("a restart replaces the process set");
    assert_eq!(
        runtime.confidence(),
        EffectConfidence::Expected,
        "§8.1: the provider has strong semantics and external behaviour can still intervene"
    );
    assert_eq!(runtime.kind(), EffectKind::Replace);
}

#[test]
fn should_keep_a_cut_connection_possible_rather_than_expected() {
    let fragment = resolve("restart service nginx");
    let network = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::NetworkRuntime)
        .expect("connections being served may be cut");
    assert_eq!(
        network.confidence(),
        EffectConfidence::Possible,
        "§8.1: Ono cannot assert that it will occur"
    );
}

#[test]
fn should_name_the_inverse_action_as_compensation_rather_than_rollback() {
    let fragment = resolve("restart service nginx");
    let runtime = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ProcessRuntime)
        .expect("a restart replaces the process set");
    assert_eq!(
        runtime.compensation(),
        Some("start the service again"),
        "§27.4: compensation restores semantics, and rollback is not what this is"
    );
}

#[test]
fn should_cite_the_contract_every_effect_came_from() {
    let fragment = resolve("restart service nginx");
    assert!(
        fragment.actions()[0].effects()[0]
            .evidence()
            .iter()
            .any(|reference| reference.contains("plannable_operations")),
        "§8.2's `evidence` is the contract that supports the claim"
    );
}

#[test]
fn should_carry_the_verification_contracts_the_plan_needs() {
    let fragment = resolve("restart service nginx");
    let contract = fragment
        .verification()
        .iter()
        .find(|contract| contract.class() == VerificationClass::Required)
        .expect("§23.1: a plan that mutates carries at least one contract");
    assert_eq!(contract.subject(), "service nginx");
    assert_eq!(contract.expression(), "state == running");
}

#[test]
fn should_freeze_the_preconditions_that_detect_material_drift() {
    let fragment = resolve("restart service nginx");
    let kinds: Vec<PreconditionKind> = fragment.actions()[0]
        .preconditions()
        .iter()
        .map(ono_change_core::Precondition::kind)
        .collect();
    assert!(kinds.contains(&PreconditionKind::Existence));
    assert!(
        kinds.contains(&PreconditionKind::Generation),
        "§7.2: the service's generation or state still equals what was resolved"
    );
    assert!(kinds.contains(&PreconditionKind::ProviderAvailable));
}

#[test]
fn should_record_the_provider_version_the_fragment_was_resolved_against() {
    assert_eq!(
        resolve("restart service nginx").provider_version(),
        Some("test-1"),
        "§4.4: a plan resolved against one version of a provider is not the same plan against \
         another"
    );
}

#[test]
fn should_resolve_the_same_intent_to_the_same_action_identity() {
    let first = resolve("restart service nginx");
    let second = resolve("restart service nginx");
    assert_eq!(
        first.actions()[0].id(),
        second.actions()[0].id(),
        "§4.4: a plan built twice from the same intent, session and instant is the same plan"
    );
}

#[test]
fn should_act_on_every_frozen_target_the_caller_supplies() {
    let (_, change) = service_provider();
    let targets = vec![
        FrozenTarget::new("ono.service/1", "nginx", "nginx"),
        FrozenTarget::new("ono.service/1", "haproxy", "haproxy"),
    ];
    let fragment = change
        .resolve(&intent("restart service nginx"), &targets)
        .expect("both targets resolve");
    assert_eq!(
        fragment.actions().len(),
        2,
        "§4.3 freezes the set, and every member of it gets an action"
    );
}

#[test]
fn should_not_add_a_target_the_frozen_set_does_not_hold() {
    let (_, change) = service_provider();
    let targets = vec![FrozenTarget::new("ono.service/1", "haproxy", "haproxy")];
    let fragment = change
        .resolve(&intent("restart service nginx"), &targets)
        .expect("the frozen set decides");
    assert_eq!(fragment.actions()[0].target(), Some("haproxy"));
    assert_eq!(
        fragment.actions().len(),
        1,
        "§2.6: the set a plan acts on is the one it froze"
    );
}

// --- §6.2: what is not plannable ---------------------------------------------------------------

#[test]
fn should_refuse_an_operation_with_no_contract() {
    let (_, change) = service_provider();
    let error = change
        .resolve(&intent("install plugin dev.example.thing"), &[])
        .expect_err("§6.1 has no row for it");
    assert!(
        error.help().is_some_and(|help| help.contains("§6.2")),
        "§6.2's second sentence is the route to changing that"
    );
}

#[test]
fn should_refuse_a_statement_the_registry_does_not_answer_to() {
    let (_, change) = service_provider();
    change
        .resolve(&intent("validate config nginx"), &[])
        .expect_err("ADR-0813: `validate config nginx` is deliberately not plannable in v0.6");
}

#[test]
fn should_refuse_an_option_the_command_does_not_declare() {
    let (_, change) = service_provider();
    change
        .resolve(&intent("restart service nginx --force"), &[])
        .expect_err("`restart service` declares no `--force`");
}

// --- support (§51) ----------------------------------------------------------------------------

#[test]
fn should_answer_full_for_an_intent_the_provider_serves() {
    let (_, change) = service_provider();
    assert_eq!(
        change.supports(&intent("restart service nginx")),
        Support::Full
    );
}

#[test]
fn should_answer_none_for_a_target_this_provider_does_not_serve() {
    let (_, change) = service_provider();
    assert_eq!(
        change.supports(&intent("remove package nginx")),
        Support::None,
        "§51: a provider that does not answer about packages does not plan a package change"
    );
}

#[test]
fn should_answer_none_for_an_operation_nobody_declared() {
    let (_, change) = service_provider();
    assert_eq!(
        change.supports(&intent("validate config nginx")),
        Support::None
    );
}

#[test]
fn should_answer_partial_when_the_provider_cannot_run_here() {
    let provider = Arc::new(
        FakeProvider::new("linux.systemd", &["service"]).unavailable("systemd is not running here"),
    );
    let change = change_provider(provider, Arc::new(FakeObserver::nginx()));
    match change.supports(&intent("restart service nginx")) {
        Support::Partial(reason) => assert!(reason.contains("systemd is not running here")),
        other => panic!("§51: an unavailable provider supports it partially, not {other:?}"),
    }
}

#[test]
fn should_name_what_it_cannot_plan_when_only_part_of_a_block_resolves() {
    let (_, change) = service_provider();
    let block = "restart service nginx\nremove package nginx";
    match change.supports(&intent(block)) {
        Support::Partial(reason) => assert!(reason.contains("remove package nginx")),
        other => panic!("§51: `Partial` names what it cannot, and this is {other:?}"),
    }
}

// --- files (§31, §32) --------------------------------------------------------------------------

fn file_observer() -> FakeObserver {
    FakeObserver::new()
        .seeing(
            "/etc/nginx/nginx.conf",
            "identity",
            Value::string("/etc/nginx/nginx.conf"),
        )
        .seeing("/etc/nginx/nginx.conf", "sha256", Value::string("abc123"))
        .seeing(
            "/etc/nginx/nginx.conf",
            "persistence-domain",
            Value::string("tank/etc"),
        )
        .seeing("./nginx.conf", "identity", Value::string("./nginx.conf"))
        .seeing("./nginx.conf", "sha256", Value::string("def456"))
        .seeing(
            "./nginx.conf",
            "persistence-domain",
            Value::string("tank/home"),
        )
}

#[test]
fn should_guarantee_a_written_file_holds_what_was_written() {
    let fragment = resolve_on(
        "file",
        file_observer(),
        "write file /etc/nginx/nginx.conf --overwrite",
    )
    .expect("writing a file is plannable");
    let effect = &fragment.actions()[0].effects()[0];
    assert_eq!(effect.domain(), EffectDomain::FilesystemPersistent);
    assert_eq!(
        effect.confidence(),
        EffectConfidence::Guaranteed,
        "§8.1: the effect follows directly from the completed operation and the provider contract"
    );
    assert!(!effect.is_irreversible());
}

#[test]
fn should_put_a_copy_s_effect_on_the_destination_rather_than_on_what_it_read() {
    let fragment = resolve_on(
        "file",
        file_observer(),
        "copy file ./nginx.conf to /etc/nginx/nginx.conf",
    )
    .expect("copying a file is plannable");
    assert_eq!(
        fragment.actions()[0].effects()[0].object(),
        Some("/etc/nginx/nginx.conf"),
        "§8.2's `object` is the thing that changed"
    );
}

#[test]
fn should_give_a_move_both_halves_of_what_it_does() {
    let fragment = resolve_on(
        "file",
        file_observer(),
        "move file ./nginx.conf to /etc/nginx/nginx.conf",
    )
    .expect("moving a file is plannable");
    let kinds: Vec<EffectKind> = fragment.actions()[0]
        .effects()
        .iter()
        .map(ono_change_core::ProposedEffect::kind)
        .collect();
    assert!(kinds.contains(&EffectKind::Remove));
    assert!(kinds.contains(&EffectKind::Create));
}

#[test]
fn should_make_a_move_non_idempotent() {
    let fragment = resolve_on(
        "file",
        file_observer(),
        "move file ./nginx.conf to /etc/nginx/nginx.conf",
    )
    .expect("plannable");
    assert_eq!(
        fragment.actions()[0].idempotency(),
        Idempotency::NonIdempotent,
        "§41.1: the source is gone the second time, so running it twice is not running it once"
    );
}

#[test]
fn should_freeze_the_persistence_domain_a_file_lives_in() {
    let fragment = resolve_on("file", file_observer(), "remove file /etc/nginx/nginx.conf")
        .expect("plannable");
    let domain = fragment.actions()[0]
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::PersistenceDomain)
        .expect("§7.2: the path still resolves to the same dataset or subvolume");
    assert_eq!(domain.expected(), &Value::string("tank/etc"));
}

#[test]
fn should_freeze_a_file_s_content_digest() {
    let fragment = resolve_on("file", file_observer(), "remove file /etc/nginx/nginx.conf")
        .expect("plannable");
    let digest = fragment.actions()[0]
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::ContentDigest)
        .expect("§7.2: the file hash still equals X");
    assert_eq!(digest.expected(), &Value::string("abc123"));
}

#[test]
fn should_keep_mounted_children_in_the_recovery_semantics_of_a_recursive_delete() {
    let observer = FakeObserver::new().seeing("/srv/data", "identity", Value::string("/srv/data"));
    let fragment =
        resolve_on("dir", observer, "remove dir /srv/data --recursive").expect("plannable");
    let declared = fragment.actions()[0]
        .declared_recovery()
        .expect("§6.1 requires them");
    assert!(
        declared.contains("§32.3"),
        "§32.3: recursive path operations MUST NOT assume mounted filesystems or nested \
         subvolumes belong to the same recovery scope"
    );
    assert!(
        declared.contains("§32.2"),
        "§32.2: persistence boundaries and size are calculated before plan sealing"
    );
}

#[test]
fn should_show_a_mounted_child_as_a_possible_effect_of_a_recursive_delete() {
    let observer = FakeObserver::new().seeing("/srv/data", "identity", Value::string("/srv/data"));
    let fragment =
        resolve_on("dir", observer, "remove dir /srv/data --recursive").expect("plannable");
    let application = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ApplicationPersistent)
        .expect("§32.3: a mounted child may hold application data");
    assert_eq!(application.confidence(), EffectConfidence::Possible);
}

// --- packages (§30) ---------------------------------------------------------------------------

fn package_observer() -> FakeObserver {
    FakeObserver::new()
        .seeing("nginx", "identity", Value::string("nginx"))
        .seeing("nginx", "version", Value::string("1.22.0"))
}

fn resolve_package(observer: FakeObserver, text: &str) -> PlanFragment {
    resolve_on("package", observer, text).expect("package changes are plannable")
}

#[test]
fn should_show_a_repository_fetch_as_an_irreversible_external_effect() {
    let fragment = resolve_package(package_observer(), "add package nginx");
    let outbound = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ExternalSideEffect)
        .expect("§35.1: the package manager fetches from a repository");
    assert!(
        outbound.is_irreversible(),
        "§35.2: an outbound request is not recoverable through a local snapshot"
    );
    assert_eq!(outbound.kind(), EffectKind::Emit);
}

#[test]
fn should_verify_the_installed_version_when_one_was_asked_for() {
    let fragment = resolve_package(package_observer(), "add package nginx --version 1.24.0");
    assert!(
        fragment
            .verification()
            .iter()
            .any(|contract| contract.expression() == "version == 1.24.0"),
        "§30.4: package verification includes the installed package version"
    );
}

#[test]
fn should_leave_out_a_version_check_nobody_asked_for() {
    let fragment = resolve_package(package_observer(), "add package nginx");
    assert!(
        !fragment
            .verification()
            .iter()
            .any(|contract| contract.expression().starts_with("version ==")),
        "§23.1 asks for checks that can be answered, and a version nobody named is not one"
    );
}

#[test]
fn should_check_package_manager_consistency() {
    let fragment = resolve_package(package_observer(), "add package nginx");
    assert!(
        fragment
            .verification()
            .iter()
            .any(|contract| contract.expression() == "consistent"),
        "§30.4: package update verification includes package-manager consistency"
    );
}

#[test]
fn should_freeze_the_installed_version_as_a_precondition() {
    let fragment = resolve_package(package_observer(), "remove package nginx");
    let version = fragment.actions()[0]
        .preconditions()
        .iter()
        .find(|p| p.kind() == PreconditionKind::Version)
        .expect("§7.2: package installed version still equals Z");
    assert_eq!(version.expected(), &Value::string("1.22.0"));
}

#[test]
fn should_report_a_reboot_requirement_as_an_expected_effect() {
    let observer = package_observer().rebooting("nginx", RebootRequirement::Required);
    let fragment = resolve_package(observer, "set package nginx --version 1.24.0");
    let reboot = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::KernelRuntime)
        .expect("§30.5: the provider may mark a reboot requirement as a ProposedEffect");
    assert_eq!(reboot.confidence(), EffectConfidence::Expected);
    assert!(reboot.explanation().contains("requires a reboot"));
}

#[test]
fn should_report_a_reboot_recommendation_as_a_possible_effect() {
    let observer = package_observer().rebooting("nginx", RebootRequirement::Recommended);
    let fragment = resolve_package(observer, "set package nginx --version 1.24.0");
    let reboot = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::KernelRuntime)
        .expect("§30.5 distinguishes requirement from suggestion");
    assert_eq!(reboot.confidence(), EffectConfidence::Possible);
    assert!(reboot.explanation().contains("recommends a reboot"));
}

#[test]
fn should_leave_out_a_reboot_effect_when_the_provider_says_none_is_needed() {
    let observer = package_observer().rebooting("nginx", RebootRequirement::NotNeeded);
    let fragment = resolve_package(observer, "add package nginx");
    assert!(
        !fragment.actions()[0]
            .effects()
            .iter()
            .any(|effect| effect.domain() == EffectDomain::KernelRuntime),
        "a provider that says no reboot is involved has said something, and it is not an effect"
    );
}

#[test]
fn should_answer_unknown_rather_than_nothing_when_the_provider_cannot_say() {
    let fragment = resolve_package(package_observer(), "add package nginx");
    let reboot = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::KernelRuntime)
        .expect("§30.5: a provider that cannot say still has the dimension");
    assert_eq!(
        reboot.confidence(),
        EffectConfidence::Unknown,
        "§2.4: unknown MUST NOT be silently promoted, and silence is not `no reboot needed`"
    );
}

#[test]
fn should_not_ask_about_a_reboot_where_the_operation_has_no_such_dimension() {
    let fragment = resolve("restart service nginx");
    assert!(
        !fragment.actions()[0]
            .effects()
            .iter()
            .any(|effect| effect.domain() == EffectDomain::KernelRuntime),
        "§30.5's question belongs to a package change; a restart has no reboot dimension at all"
    );
}

#[test]
fn should_show_the_scope_a_package_change_needs_protection_for() {
    let fragment = resolve_package(package_observer(), "add package nginx");
    let declared = fragment.actions()[0]
        .declared_recovery()
        .expect("§6.1 requires them");
    assert!(
        declared.contains("§30.3"),
        "§30.3: a root snapshot may include application data unintentionally, and the layout MUST \
         be shown"
    );
}

// --- network (§34) -----------------------------------------------------------------------------

fn route_observer() -> FakeObserver {
    FakeObserver::new().seeing("10.0.0.0/8", "identity", Value::string("10.0.0.0/8"))
}

#[test]
fn should_name_the_inverse_action_a_network_provider_exposes() {
    let fragment = resolve_on("route", route_observer(), "remove route 10.0.0.0/8")
        .expect("route changes are plannable");
    let network = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::NetworkRuntime)
        .expect("removing a route changes the routing table");
    assert_eq!(
        network.compensation(),
        Some("add the route back"),
        "§34.1: providers MAY expose inverse actions, and the compensation field is where it goes"
    );
}

#[test]
fn should_show_remote_management_lockout_as_its_own_effect() {
    let fragment =
        resolve_on("route", route_observer(), "remove route 10.0.0.0/8").expect("plannable");
    let remote = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::RemoteSystem)
        .expect("§34.2: removing the path the active remote link uses is a landmark");
    assert_eq!(remote.kind(), EffectKind::Interrupt);
}

#[test]
fn should_make_stopping_an_interface_cut_sessions_irreversibly() {
    let observer = FakeObserver::new()
        .seeing("eth0", "identity", Value::string("eth0"))
        .seeing("eth0", "state", Value::string("up"));
    let fragment = resolve_on("interface", observer, "stop interface eth0").expect("plannable");
    let cut = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.kind() == EffectKind::Interrupt && effect.is_irreversible())
        .expect("a cut session does not come back when the interface does");
    assert_eq!(cut.domain(), EffectDomain::NetworkRuntime);
}

// --- identity ----------------------------------------------------------------------------------

#[test]
fn should_keep_a_removed_uid_irreversible_after_protection() {
    let observer = FakeObserver::new().seeing("deploy", "identity", Value::string("deploy"));
    let fragment = resolve_on("user", observer, "remove user deploy").expect("plannable");
    let identity = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::IdentitySecurityState)
        .expect("removing a user changes the account database");
    assert!(
        identity.is_irreversible(),
        "§2.13: the UID is not put back, and the flag is what keeps a protected plan honest"
    );
}

#[test]
fn should_write_creating_a_user_as_compensation_that_is_often_incomplete() {
    let observer = FakeObserver::new().seeing("deploy", "identity", Value::Null);
    let fragment = resolve_on("user", observer, "add user deploy").expect("plannable");
    let declared = fragment.actions()[0]
        .declared_recovery()
        .expect("§6.1 requires them");
    assert!(
        declared.contains("§27.4"),
        "§27.4's own example: create user <-> remove user, often incomplete"
    );
}

#[test]
fn should_allocate_a_uid_as_an_effect_nobody_can_undo() {
    let observer = FakeObserver::new().seeing("deploy", "identity", Value::Null);
    let fragment = resolve_on("user", observer, "add user deploy").expect("plannable");
    assert!(
        fragment.actions()[0]
            .effects()
            .iter()
            .any(|effect| effect.is_irreversible()
                && effect.explanation().contains("UID is allocated")),
        "§2.13: removing the user frees the number and does not undo what was recorded against it"
    );
}

// --- mounts (§32.3) ----------------------------------------------------------------------------

#[test]
fn should_treat_a_mounted_filesystem_as_its_own_recovery_scope() {
    let observer = FakeObserver::new().seeing("/mnt/data", "identity", Value::string("/mnt/data"));
    let fragment =
        resolve_on("filesystem", observer, "unmount filesystem /mnt/data").expect("plannable");
    assert!(
        fragment.actions()[0]
            .declared_recovery()
            .expect("§6.1 requires them")
            .contains("§32.3"),
        "§32.3: the filesystem being unmounted is its own recovery scope"
    );
}

#[test]
fn should_show_a_forced_unmount_as_a_possible_irreversible_data_loss() {
    let observer = FakeObserver::new().seeing("/mnt/data", "identity", Value::string("/mnt/data"));
    let fragment = resolve_on(
        "filesystem",
        observer,
        "unmount filesystem /mnt/data --force",
    )
    .expect("plannable");
    let unflushed = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ApplicationPersistent)
        .expect("§32: writes that had not reached the device are unaccounted for");
    assert!(unflushed.is_irreversible());
    assert_eq!(unflushed.confidence(), EffectConfidence::Possible);
}
