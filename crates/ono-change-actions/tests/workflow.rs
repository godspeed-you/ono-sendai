#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! The four claims this crate exists to make good on.
//!
//! §31's reference workflow resolves end to end; §33.1 holds for a `SIGKILL` whatever arguments
//! it is given; §30.5's three answers stay three answers; and §51's prohibition on mutating
//! during `supports` and `resolve` is countable.

mod support;

use std::sync::Arc;

use ono_change_actions::ProviderChangeProvider;
use ono_change_actions::registry::RebootRequirement;
use ono_change_core::{
    ChangeProvider, EffectConfidence, EffectDomain, EffectKind, FrozenTarget, Intent, PlanFragment,
    Support, VerificationClass,
};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, Value};
use support::{FakeObserver, FakeProvider, change_provider, service, socket};

// --- §31: the reference configuration change workflow -------------------------------------------

/// The provider set §31 resolves through: `path -> mount -> filesystem`, `service -> systemd`,
/// `socket -> network`. One fake answers about all three, because the workflow is one plan.
fn nginx_host() -> Arc<FakeProvider> {
    Arc::new(
        FakeProvider::new("linux.host", &["file", "service", "socket"])
            .holding(vec![service("nginx", "running"), socket(443)]),
    )
}

fn nginx_observer() -> FakeObserver {
    FakeObserver::nginx()
        .seeing("./nginx.conf", "identity", Value::string("./nginx.conf"))
        .seeing("./nginx.conf", "sha256", Value::string("def456"))
        .seeing(
            "./nginx.conf",
            "persistence-domain",
            Value::string("tank/home"),
        )
}

/// §31's block, in the Ono spellings ADR-0813 fixes.
const WORKFLOW: &str = "copy file ./nginx.conf to /etc/nginx/nginx.conf --overwrite\n\
                        restart service nginx\n\
                        verify service nginx state == running\n\
                        verify socket :443 exists";

fn workflow() -> (Arc<FakeProvider>, ProviderChangeProvider, PlanFragment) {
    let provider = nginx_host();
    let change = change_provider(Arc::clone(&provider), Arc::new(nginx_observer()));
    let fragment = change
        .resolve(&Intent::new(WORKFLOW, "plan { … }"), &[])
        .expect("§31's reference workflow resolves");
    (provider, change, fragment)
}

#[test]
fn should_resolve_the_reference_workflow_into_its_two_changes() {
    let (_, _, fragment) = workflow();
    assert_eq!(
        fragment.actions().len(),
        2,
        "§31: the file is replaced and the service is restarted (ADR-0813)"
    );
}

#[test]
fn should_give_every_action_of_the_reference_workflow_effects() {
    let (_, _, fragment) = workflow();
    for action in fragment.actions() {
        assert!(
            !action.effects().is_empty(),
            "§6.1: `{}` was planned without expected direct effects",
            action.summary()
        );
    }
}

#[test]
fn should_reach_exactly_the_three_mutation_domains_the_workflow_touches() {
    let (_, _, fragment) = workflow();
    let mut domains: Vec<EffectDomain> = fragment
        .actions()
        .iter()
        .flat_map(ono_change_core::PlanAction::effects)
        .map(ono_change_core::ProposedEffect::domain)
        .collect();
    domains.sort_unstable_by_key(|domain| domain.as_str());
    domains.dedup();
    assert_eq!(
        domains,
        vec![
            EffectDomain::FilesystemPersistent,
            EffectDomain::NetworkRuntime,
            EffectDomain::ProcessRuntime,
        ],
        "§31: config -> service -> process -> listeners -> clients, in Appendix A.1's vocabulary"
    );
}

#[test]
fn should_carry_the_two_checks_the_reference_workflow_writes() {
    let (_, _, fragment) = workflow();
    let written: Vec<(&str, &str)> = fragment
        .verification()
        .iter()
        .map(|contract| (contract.subject(), contract.expression()))
        .collect();
    assert!(written.contains(&("service nginx", "state == running")));
    assert!(written.contains(&("socket :443", "exists")));
}

#[test]
fn should_answer_both_checks_of_the_reference_workflow_against_the_provider() {
    let (_, change, fragment) = workflow();
    for contract in fragment.verification().iter().filter(|contract| {
        contract.subject().starts_with("service ") || contract.subject().starts_with("socket ")
    }) {
        let result = change
            .verify(contract)
            .expect("the observation is possible");
        assert_eq!(
            result.status(),
            ono_change_core::VerificationStatus::Passed,
            "§31's verify step: `{}` `{}` should hold after the change",
            contract.subject(),
            contract.expression()
        );
    }
}

#[test]
fn should_refuse_the_configuration_validation_step_rather_than_invent_it() {
    let (_, change, _) = workflow();
    let error = change
        .resolve(
            &Intent::new("validate config nginx", "validate config nginx"),
            &[],
        )
        .expect_err(
            "ADR-0813: there is no `validate` verb and no `config` target a provider serves",
        );
    assert_eq!(
        error.code().name(),
        "resolve.command_not_found",
        "§6.2: an operation with no declared contract refuses rather than being guessed at"
    );
}

#[test]
fn should_support_the_whole_reference_workflow() {
    let (_, change, _) = workflow();
    assert_eq!(
        change.supports(&Intent::new(WORKFLOW, "plan { … }")),
        Support::Full
    );
}

#[test]
fn should_order_the_reference_workflow_as_it_was_written() {
    let (_, _, fragment) = workflow();
    assert!(fragment.actions()[0].summary().starts_with("copy file"));
    assert!(
        fragment.actions()[1]
            .summary()
            .starts_with("restart service")
    );
    assert_eq!(fragment.actions()[0].ordinal(), 0);
    assert_eq!(fragment.actions()[1].ordinal(), 1);
}

#[test]
fn should_protect_the_configuration_file_and_say_what_that_does_not_cover() {
    let (_, _, fragment) = workflow();
    let restart = &fragment.actions()[1];
    assert!(
        restart
            .declared_recovery()
            .expect("§6.1 requires recovery semantics")
            .contains("does not restore process identity"),
        "§31's recovery step restores the config and restarts; §33.2 says what comes back and \
         what does not"
    );
}

#[test]
fn should_give_the_workflow_a_required_check_for_every_change_it_makes() {
    let (_, _, fragment) = workflow();
    let required = fragment
        .verification()
        .iter()
        .filter(|contract| contract.class() == VerificationClass::Required)
        .count();
    assert!(
        required >= fragment.actions().len(),
        "§23.1: every plan containing a MUTATE action has at least one verification contract"
    );
}

// --- §33.1: runtime state is generally not recoverable ------------------------------------------

fn kill(arguments: &str) -> PlanFragment {
    let provider = Arc::new(FakeProvider::new("linux.procfs", &["process"]));
    let observer = FakeObserver::new().seeing("4421", "identity", Value::Int(4_421));
    let change = change_provider(provider, Arc::new(observer));
    let line = format!("kill process 4421{arguments}");
    change
        .resolve(&Intent::new(line.as_str(), line.as_str()), &[])
        .unwrap_or_else(|error| panic!("`{line}` should resolve: {}", error.message()))
}

/// Every way §33.3's rendering can be asked for.
const KILL_ARGUMENTS: &[&str] = &[
    "",
    " --signal SIGKILL",
    " --signal SIGTERM",
    " --signal 9",
    " --signal=SIGKILL",
];

#[test]
fn should_keep_a_signal_s_runtime_effect_irreversible_whatever_the_arguments_are() {
    for arguments in KILL_ARGUMENTS {
        let fragment = kill(arguments);
        let runtime = fragment.actions()[0]
            .effects()
            .iter()
            .find(|effect| effect.domain() == EffectDomain::ProcessRuntime)
            .unwrap_or_else(|| panic!("`kill process 4421{arguments}` ends a process"));
        assert!(
            runtime.is_irreversible(),
            "§33.1: signals, especially SIGKILL, are normally irreversible in v0.6, and \
             `{arguments}` does not change that"
        );
    }
}

#[test]
fn should_never_offer_a_signal_an_inverse_action() {
    for arguments in KILL_ARGUMENTS {
        let fragment = kill(arguments);
        for effect in fragment.actions()[0].effects() {
            assert_eq!(
                effect.compensation(),
                None,
                "§33.1: there is no inverse action for a signal, and `{arguments}` supplies none"
            );
        }
    }
}

#[test]
fn should_say_a_filesystem_snapshot_does_not_change_the_classification() {
    let fragment = kill(" --signal SIGKILL");
    assert!(
        fragment.actions()[0]
            .declared_recovery()
            .expect("§6.1 requires them")
            .contains("filesystem snapshot does not change that classification"),
        "§33.1's own sentence, carried where the operator reads it"
    );
}

#[test]
fn should_keep_the_memory_a_killed_process_held_irrecoverable() {
    let fragment = kill(" --signal SIGKILL");
    let held = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ApplicationPersistent)
        .expect("§33.3: temporary data held only in memory");
    assert!(held.is_irreversible());
    assert_eq!(held.confidence(), EffectConfidence::Possible);
}

#[test]
fn should_show_open_connections_as_possible_impact_of_a_kill() {
    let fragment = kill(" --signal SIGKILL");
    let connections = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::NetworkRuntime)
        .expect("§33.3: open connections");
    assert_eq!(connections.kind(), EffectKind::Interrupt);
    assert_eq!(connections.confidence(), EffectConfidence::Possible);
}

#[test]
fn should_distinguish_a_catchable_signal_from_one_that_cannot_be_caught() {
    let stopping = {
        let provider = Arc::new(FakeProvider::new("linux.procfs", &["process"]));
        let observer = FakeObserver::new().seeing("4421", "identity", Value::Int(4_421));
        let change = change_provider(provider, Arc::new(observer));
        change
            .resolve(&Intent::new("stop process 4421", "stop process 4421"), &[])
            .expect("stopping a process is plannable")
    };
    let flushed = stopping.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::ApplicationPersistent)
        .expect("SIGTERM is catchable");
    assert!(
        !flushed.is_irreversible(),
        "an application that flushes on shutdown wrote what it held; the same effect under \
         SIGKILL is irreversible (§33.1)"
    );
}

// --- §30.5: requirement, recommendation, and an answer nobody has ---------------------------------

fn package_change(reboot: Option<RebootRequirement>) -> PlanFragment {
    let provider = Arc::new(FakeProvider::new("linux.packages", &["package"]));
    let mut observer = FakeObserver::new()
        .seeing("nginx", "identity", Value::string("nginx"))
        .seeing("nginx", "version", Value::string("1.22.0"));
    if let Some(answer) = reboot {
        observer = observer.rebooting("nginx", answer);
    }
    let change = change_provider(provider, Arc::new(observer));
    change
        .resolve(
            &Intent::new(
                "set package nginx --version 1.24.0",
                "plan set package nginx --version 1.24.0",
            ),
            &[],
        )
        .expect("a package transition is plannable")
}

fn reboot_effect(fragment: &PlanFragment) -> Option<EffectConfidence> {
    fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::KernelRuntime)
        .map(ono_change_core::ProposedEffect::confidence)
}

#[test]
fn should_produce_different_effects_for_a_reboot_requirement_and_a_recommendation() {
    let required = package_change(Some(RebootRequirement::Required));
    let recommended = package_change(Some(RebootRequirement::Recommended));
    assert_ne!(
        reboot_effect(&required),
        reboot_effect(&recommended),
        "§30.5: the provider MUST distinguish requirement from suggestion"
    );
}

#[test]
fn should_make_a_reboot_requirement_the_stronger_of_the_two() {
    assert_eq!(
        reboot_effect(&package_change(Some(RebootRequirement::Required))),
        Some(EffectConfidence::Expected)
    );
    assert_eq!(
        reboot_effect(&package_change(Some(RebootRequirement::Recommended))),
        Some(EffectConfidence::Possible)
    );
}

#[test]
fn should_answer_unknown_rather_than_null_when_the_provider_cannot_say() {
    assert_eq!(
        reboot_effect(&package_change(None)),
        Some(EffectConfidence::Unknown),
        "§30.5 and §2.4: a provider that cannot say has not said that no reboot is needed"
    );
}

#[test]
fn should_leave_the_operation_with_no_reboot_effect_when_the_provider_says_none() {
    assert_eq!(
        reboot_effect(&package_change(Some(RebootRequirement::NotNeeded))),
        None,
        "`not needed` is an answer about the world, and it is not an effect"
    );
}

#[test]
fn should_word_each_reboot_answer_so_a_person_can_tell_them_apart() {
    let text = |answer: RebootRequirement| {
        package_change(Some(answer)).actions()[0]
            .effects()
            .iter()
            .find(|effect| effect.domain() == EffectDomain::KernelRuntime)
            .map(|effect| effect.explanation().to_owned())
    };
    let required = text(RebootRequirement::Required).expect("a requirement is an effect");
    let recommended = text(RebootRequirement::Recommended).expect("so is a recommendation");
    assert!(required.contains("requires a reboot"));
    assert!(recommended.contains("recommends a reboot"));
    assert_ne!(required, recommended);
}

#[test]
fn should_keep_the_reboot_answer_out_of_the_effect_of_the_package_itself() {
    let fragment = package_change(Some(RebootRequirement::Required));
    let files = fragment.actions()[0]
        .effects()
        .iter()
        .find(|effect| effect.domain() == EffectDomain::FilesystemPersistent)
        .expect("a version transition replaces the package's files");
    assert_eq!(
        files.confidence(),
        EffectConfidence::Guaranteed,
        "§8.1: the effect on the object the operation names follows from the provider contract, \
         whatever the reboot answer is"
    );
}

// --- §51: `supports` and `resolve` mutate nothing --------------------------------------------------

/// A provider that fails loudly if anything reads it in a way it did not expect.
#[derive(Debug)]
struct CountingProvider(Arc<FakeProvider>);

#[async_trait::async_trait]
impl Provider for CountingProvider {
    fn id(&self) -> &str {
        self.0.id()
    }
    fn targets(&self) -> &[&str] {
        self.0.targets()
    }
    fn schemas(&self) -> Vec<Arc<ono_value::Schema>> {
        self.0.schemas()
    }
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("service.manage", Risk::Mutate)]
    }
    fn availability(&self) -> Availability {
        self.0.availability()
    }
    fn snapshot(&self, query: &Query) -> Result<ono_pipeline::ValueStream, ErrorValue> {
        self.0.snapshot(query)
    }
    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        self.0.resolve(selector).await
    }
    async fn act(
        &self,
        action: &ono_provider_api::Action,
    ) -> Result<ono_provider_api::ActionOutcome, ErrorValue> {
        self.0.act(action).await
    }
}

fn counting() -> (Arc<FakeProvider>, ProviderChangeProvider) {
    let inner = Arc::new(
        FakeProvider::new("linux.systemd", &["service"]).holding(vec![service("nginx", "running")]),
    );
    let change = ProviderChangeProvider::new(
        Arc::new(CountingProvider(Arc::clone(&inner))),
        Arc::new(FakeObserver::nginx()),
        Arc::new(support::TestBridge),
        "session-1",
        support::at(),
    )
    .expect("the registries typecheck");
    (inner, change)
}

#[test]
fn should_not_change_anything_while_answering_whether_it_supports_an_intent() {
    let (provider, change) = counting();
    for intent in [
        "restart service nginx",
        "stop service nginx",
        "remove package nginx",
        "sh -c 'rm -rf /somewhere'",
        WORKFLOW,
    ] {
        let _ = change.supports(&Intent::new(intent, intent));
    }
    assert_eq!(
        provider.act_count(),
        0,
        "§51: providers MUST NOT mutate state during `supports`"
    );
}

#[test]
fn should_not_change_anything_while_resolving_an_intent() {
    let (provider, change) = counting();
    let _ = change.resolve(&Intent::new(WORKFLOW, "plan { … }"), &[]);
    let _ = change.resolve(
        &Intent::new("restart service nginx", "restart service nginx"),
        &[FrozenTarget::new("ono.service/1", "nginx", "nginx")],
    );
    assert_eq!(
        provider.act_count(),
        0,
        "§51: providers MUST NOT mutate state during `resolve`, which is what makes §2.1 hold for \
         the whole planner"
    );
}

#[test]
fn should_not_change_anything_while_revalidating() {
    let (provider, change) = counting();
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("plannable");
    let _ = change.revalidate(&fragment.actions()[0]);
    assert_eq!(
        provider.act_count(),
        0,
        "§7.3: revalidation re-resolves targets and preconditions; it changes nothing"
    );
}

#[test]
fn should_change_something_only_when_asked_to_execute() {
    let (provider, change) = counting();
    let fragment = change
        .resolve(
            &Intent::new("restart service nginx", "restart service nginx"),
            &[],
        )
        .expect("plannable");
    assert_eq!(provider.act_count(), 0);
    change
        .execute(&fragment.actions()[0])
        .expect("the provider acts");
    assert_eq!(
        provider.act_count(),
        1,
        "§4.7: execution is the one phase that changes the system"
    );
}

#[test]
fn should_not_change_anything_when_an_intent_is_refused() {
    let (provider, change) = counting();
    let _ = change.resolve(
        &Intent::new("install plugin dev.example.thing", "plan install plugin …"),
        &[],
    );
    let _ = change.resolve(
        &Intent::new("validate config nginx", "plan validate …"),
        &[],
    );
    assert_eq!(
        provider.act_count(),
        0,
        "§6.2: a refusal means nothing was changed, and the prose in the error says so"
    );
}
