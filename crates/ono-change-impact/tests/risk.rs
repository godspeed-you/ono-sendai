//! The rule-based risk model — spec v0.6 §19, §28.3, §29, §30.5, §34.2, §35, §40.2, §43.3, §62.11.
//!
//! §19.2 says the classes are "rule-based, not AI-generated", so these tests assert two things
//! about every finding: the class it produced, and that a named rule produced it with a sentence
//! §40.2 could print in place of "Are you sure?".
//!
//! §19.3's three worked examples appear verbatim in outcome. Protection is a separate axis
//! (§19.1), so the examples' protection levels are deliberately not asserted here — this crate
//! does not compute them and must not appear to.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{
    EffectConfidence, EffectDomain, EffectKind, FrozenTarget, ImpactClass, ImpactGraph, ImpactNode,
    PlanAction, RequiredAcknowledgement, RestoreMethod, RiskClass, RiskDimension, UnknownBoundary,
};
use ono_change_impact::{
    ActiveLink, BulkThresholds, REBOOT_REQUIRED, REBOOT_SUGGESTED, RiskRequest, RiskRuleSpec,
    assess, rule, rules,
};
use ono_spatial_core::Confidence;
use ono_spatial_index::SpatialIndex;

mod common;

use common::{
    cgroup, container, edge, effect, file, id_of, listener, mutate, opaque, process, service,
    target_named, target_of, verify, world,
};

fn fired(request: &RiskRequest<'_>, id: &str) -> bool {
    assess(request)
        .findings()
        .iter()
        .any(|finding| finding.rule() == id)
}

fn class_of(request: &RiskRequest<'_>, id: &str) -> Option<RiskClass> {
    assess(request)
        .findings()
        .iter()
        .find(|finding| finding.rule() == id)
        .map(ono_change_core::RiskFinding::class)
}

// --- §19.3's three worked examples ------------------------------------------------------------

#[test]
fn should_rate_replacing_one_configuration_file_moderate() {
    let conf = file("/etc/nginx/nginx.conf", 4001);
    let action = mutate(0, "replace /etc/nginx/nginx.conf", id_of(&conf));
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        EffectConfidence::Guaranteed,
        "the file's contents are replaced",
        id_of(&conf),
    ));
    let actions = vec![action];
    let targets = vec![target_of(&conf, "ono.file/1")];
    let assessment = assess(&RiskRequest::new(&actions, &targets));
    assert_eq!(
        assessment.classify(),
        RiskClass::Moderate,
        "§19.3: `replace one config file with ZFS snapshot` is MODERATE risk; the protection \
         level is a separate axis (§19.1)"
    );
}

#[test]
fn should_rate_sigkilling_a_database_process_high() {
    let postgres = process(4711, "postgres");
    let action = mutate(0, "signal SIGKILL to postgres", id_of(&postgres));
    let action = action.clone().effecting(
        effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Remove,
            EffectConfidence::Guaranteed,
            "in-flight transactions and connection state are lost",
            id_of(&postgres),
        )
        .irreversible(),
    );
    let actions = vec![action];
    let targets = vec![target_of(&postgres, "ono.process/1")];
    let assessment = assess(&RiskRequest::new(&actions, &targets));
    assert_eq!(
        assessment.classify(),
        RiskClass::High,
        "§19.3: `SIGKILL database process` is HIGH risk, because §33.1 makes the runtime state it \
         destroys unrecoverable"
    );
}

#[test]
fn should_rate_restarting_all_forty_frontend_nodes_critical() {
    let fleet = Fleet::of(40);
    let assessment =
        assess(&RiskRequest::new(&fleet.actions, &fleet.targets).over_topology(&fleet.index));
    assert_eq!(
        assessment.classify(),
        RiskClass::Critical,
        "§19.3: `restart all 40 frontend nodes simultaneously` is CRITICAL risk"
    );
    assert!(
        assessment
            .leading()
            .iter()
            .any(|finding| finding.rule() == "risk.bulk.whole-role"),
        "§40.2: the gate shows the finding that made it critical, and that is the whole role"
    );
}

#[test]
fn should_leave_a_plan_that_changes_nothing_low() {
    let actions = vec![verify(0, "read the unit's state")];
    let assessment = assess(&RiskRequest::new(&actions, &[]));
    assert_eq!(
        assessment.classify(),
        RiskClass::Low,
        "§40.1: a gate on every plan is a gate nobody reads, so a plan that changes nothing is LOW"
    );
    assert!(assessment.findings().is_empty());
}

// --- the registry itself (§19.2, §62.11) ------------------------------------------------------

#[test]
fn should_expose_every_rule_with_the_dimension_it_speaks_to() {
    let ids: Vec<&str> = rules().iter().map(RiskRuleSpec::id).collect();
    for expected in [
        "risk.scope.single-object",
        "risk.scope.multi-object",
        "risk.bulk.warn-threshold",
        "risk.bulk.high-threshold",
        "risk.bulk.whole-role",
        "risk.privilege.elevation",
        "risk.irreversibility.effect",
        "risk.external.side-effect",
        "risk.downtime.service-restart",
        "risk.unknown.impact",
        "risk.reboot.required",
        "risk.remote.fanout",
        "risk.remote.link-loss",
        "risk.recovery.complexity",
    ] {
        assert!(
            ids.contains(&expected),
            "§19.2's classes come from declared rules, and `{expected}` is one of them"
        );
    }
}

#[test]
fn should_name_the_rule_and_the_reason_on_every_finding() {
    let fleet = Fleet::of(40);
    let assessment =
        assess(&RiskRequest::new(&fleet.actions, &fleet.targets).over_topology(&fleet.index));
    assert!(!assessment.findings().is_empty());
    for finding in assessment.findings() {
        assert!(
            rule(finding.rule()).is_some(),
            "§62.11: a finding comes from a declared rule, never from anywhere else — `{}`",
            finding.rule()
        );
        assert!(
            finding.reason().len() > 20,
            "§40.2: the confirmation summarises the actual risk reason rather than `Are you \
             sure?` — `{}`",
            finding.reason()
        );
    }
}

#[test]
fn should_classify_a_plan_as_the_strongest_class_any_rule_found() {
    let postgres = process(4711, "postgres");
    let action = mutate(0, "signal SIGKILL to postgres", id_of(&postgres));
    let action = action.clone().effecting(
        effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Remove,
            EffectConfidence::Guaranteed,
            "in-flight transactions are lost",
            id_of(&postgres),
        )
        .irreversible(),
    );
    let actions = vec![action];
    let targets = vec![target_of(&postgres, "ono.process/1")];
    let assessment = assess(&RiskRequest::new(&actions, &targets));
    assert!(
        assessment
            .findings()
            .iter()
            .any(|finding| finding.class() == RiskClass::Moderate),
        "the scope rule still fired"
    );
    assert_eq!(
        assessment.classify(),
        RiskClass::High,
        "§19.2: a plan with one serious finding among ordinary ones is a serious plan"
    );
}

#[test]
fn should_assess_the_same_plan_the_same_way_twice() {
    let fleet = Fleet::of(12);
    let request = RiskRequest::new(&fleet.actions, &fleet.targets).over_topology(&fleet.index);
    assert_eq!(
        assess(&request).digest_text(),
        assess(&request).digest_text(),
        "§4.4 seals the assessment into the plan digest, so it must be stable"
    );
}

// --- §19.1 scope --------------------------------------------------------------------------------

#[test]
fn should_name_the_single_object_a_plan_changes() {
    let conf = file("/etc/nginx/nginx.conf", 4001);
    let actions = vec![mutate(0, "replace nginx.conf", id_of(&conf))];
    let targets = vec![target_of(&conf, "ono.file/1")];
    let request = RiskRequest::new(&actions, &targets);
    assert_eq!(
        class_of(&request, "risk.scope.single-object"),
        Some(RiskClass::Moderate),
        "§19.3 rates a one-object change MODERATE"
    );
    assert!(!fired(&request, "risk.scope.multi-object"));
}

#[test]
fn should_raise_the_scope_dimension_when_a_plan_changes_several_objects() {
    let first = service("a.service");
    let second = service("b.service");
    let actions = vec![
        mutate(0, "restart a", id_of(&first)),
        mutate(1, "restart b", id_of(&second)),
    ];
    let targets = vec![
        target_of(&first, "ono.service/1"),
        target_of(&second, "ono.service/1"),
    ];
    let request = RiskRequest::new(&actions, &targets);
    assert!(fired(&request, "risk.scope.multi-object"));
    assert!(!fired(&request, "risk.scope.single-object"));
}

#[test]
fn should_speak_to_the_scope_dimension_in_a_scope_finding() {
    let conf = file("/etc/app/config", 4002);
    let actions = vec![mutate(0, "replace config", id_of(&conf))];
    let targets = vec![target_of(&conf, "ono.file/1")];
    let assessment = assess(&RiskRequest::new(&actions, &targets));
    let finding = assessment
        .findings()
        .iter()
        .find(|finding| finding.rule() == "risk.scope.single-object")
        .expect("the scope rule fired");
    assert_eq!(finding.dimension(), RiskDimension::Scope, "§19.1's scope");
}

// --- §28.3 bulk ---------------------------------------------------------------------------------

#[test]
fn should_warn_when_a_plan_reaches_the_configured_bulk_threshold() {
    let fleet = Fleet::of(10);
    let request = RiskRequest::new(&fleet.actions, &fleet.targets);
    assert_eq!(
        class_of(&request, "risk.bulk.warn-threshold"),
        Some(RiskClass::Moderate),
        "§53 sets `change.bulk.warn_targets = 10`"
    );
}

#[test]
fn should_not_warn_below_the_configured_bulk_threshold() {
    let fleet = Fleet::of(9);
    assert!(!fired(
        &RiskRequest::new(&fleet.actions, &fleet.targets),
        "risk.bulk.warn-threshold"
    ));
}

#[test]
fn should_raise_bulk_risk_high_at_the_configured_high_threshold() {
    let fleet = Fleet::of(50);
    let request = RiskRequest::new(&fleet.actions, &fleet.targets);
    assert_eq!(
        class_of(&request, "risk.bulk.high-threshold"),
        Some(RiskClass::High),
        "§53 sets `change.bulk.high_risk_targets = 50`"
    );
}

#[test]
fn should_take_the_configured_thresholds_in_place_of_the_defaults() {
    let fleet = Fleet::of(3);
    let request =
        RiskRequest::new(&fleet.actions, &fleet.targets).with_thresholds(BulkThresholds {
            warn_targets: 2,
            high_risk_targets: 3,
        });
    assert_eq!(
        class_of(&request, "risk.bulk.high-threshold"),
        Some(RiskClass::High),
        "§53's configuration decides the counts, and a stricter policy is a stricter answer"
    );
}

#[test]
fn should_treat_a_whole_role_as_critical_when_the_topology_proves_membership() {
    let fleet = Fleet::of(5);
    let request = RiskRequest::new(&fleet.actions, &fleet.targets).over_topology(&fleet.index);
    assert_eq!(
        class_of(&request, "risk.bulk.whole-role"),
        Some(RiskClass::Critical),
        "§28.3: if all members of a service group are targeted, availability risk is identified \
         where topology proves shared role membership"
    );
}

#[test]
fn should_not_treat_three_of_five_members_as_a_whole_role() {
    let fleet = Fleet::of(5);
    let partial: Vec<FrozenTarget> = fleet.targets.iter().take(3).cloned().collect();
    let actions: Vec<PlanAction> = fleet.actions.iter().take(3).cloned().collect();
    let request = RiskRequest::new(&actions, &partial).over_topology(&fleet.index);
    assert!(
        !fired(&request, "risk.bulk.whole-role"),
        "§28.3 is about *all* members; two members still serving is not an availability loss"
    );
    assert_eq!(
        assess(&request).classify(),
        RiskClass::Moderate,
        "the plan is still a multi-object change and nothing more"
    );
}

#[test]
fn should_not_claim_a_whole_role_without_topology_to_prove_it() {
    let fleet = Fleet::of(5);
    let request = RiskRequest::new(&fleet.actions, &fleet.targets);
    assert!(
        !fired(&request, "risk.bulk.whole-role"),
        "§28.3 identifies the risk *where topology proves* shared role membership, and a name is \
         not a proof"
    );
}

#[test]
fn should_say_which_role_is_wholly_targeted() {
    let fleet = Fleet::of(5);
    let assessment =
        assess(&RiskRequest::new(&fleet.actions, &fleet.targets).over_topology(&fleet.index));
    let finding = assessment
        .findings()
        .iter()
        .find(|finding| finding.rule() == "risk.bulk.whole-role")
        .expect("the whole-role rule fired");
    assert!(
        finding.reason().contains("frontend.slice")
            && finding.reason().contains("no serving member"),
        "§40.2's own example says which role and that no member is excluded — got `{}`",
        finding.reason()
    );
    assert_eq!(
        finding.dimension(),
        RiskDimension::Downtime,
        "§28.3 calls this availability risk"
    );
}

#[test]
fn should_treat_every_process_in_one_container_as_a_whole_role_too() {
    let runtime = container("web-7f3a");
    let workers: Vec<_> = (0..3).map(|n| process(3000 + n, "gunicorn")).collect();
    let mut objects = vec![runtime.clone()];
    objects.extend(workers.iter().cloned());
    let edges: Vec<_> = workers
        .iter()
        .map(|worker| {
            edge(
                &runtime,
                worker,
                "container.contains_process",
                Confidence::Strong,
            )
        })
        .collect();
    let index = world(&objects, &edges);
    let targets: Vec<FrozenTarget> = workers
        .iter()
        .map(|worker| target_of(worker, "ono.process/1"))
        .collect();
    let actions: Vec<PlanAction> = workers
        .iter()
        .enumerate()
        .map(|(ordinal, worker)| mutate(ordinal, "signal TERM", id_of(worker)))
        .collect();
    assert_eq!(
        class_of(
            &RiskRequest::new(&actions, &targets).over_topology(&index),
            "risk.bulk.whole-role"
        ),
        Some(RiskClass::Critical),
        "§28.3: the topology proves the shared role whichever end of the membership relation the \
         group sits at"
    );
}

// --- §43.3 privilege ----------------------------------------------------------------------------

#[test]
fn should_raise_the_privilege_dimension_when_an_action_needs_elevation() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit)).privileged()];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = RiskRequest::new(&actions, &targets);
    assert_eq!(
        class_of(&request, "risk.privilege.elevation"),
        Some(RiskClass::Moderate),
        "§43.3: the plan must show which actions require privilege"
    );
}

#[test]
fn should_not_raise_privilege_for_a_plan_that_needs_none() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit))];
    let targets = vec![target_of(&unit, "ono.service/1")];
    assert!(!fired(
        &RiskRequest::new(&actions, &targets),
        "risk.privilege.elevation"
    ));
}

// --- §19.1 irreversibility ----------------------------------------------------------------------

#[test]
fn should_raise_irreversibility_for_an_effect_nothing_can_undo() {
    let postgres = process(4711, "postgres");
    let action = mutate(0, "signal SIGKILL", id_of(&postgres));
    let action = action.clone().effecting(
        effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Remove,
            EffectConfidence::Guaranteed,
            "the process and its in-memory state end",
            id_of(&postgres),
        )
        .irreversible(),
    );
    let actions = vec![action];
    let targets = vec![target_of(&postgres, "ono.process/1")];
    let request = RiskRequest::new(&actions, &targets);
    assert_eq!(
        class_of(&request, "risk.irreversibility.effect"),
        Some(RiskClass::High)
    );
}

#[test]
fn should_require_an_irreversibility_acknowledgement_of_its_own() {
    let postgres = process(4711, "postgres");
    let action = mutate(0, "signal SIGKILL", id_of(&postgres));
    let action = action.clone().effecting(
        effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Remove,
            EffectConfidence::Guaranteed,
            "the process and its in-memory state end",
            id_of(&postgres),
        )
        .irreversible(),
    );
    let actions = vec![action];
    let targets = vec![target_of(&postgres, "ono.process/1")];
    let assessment = assess(&RiskRequest::new(&actions, &targets));
    assert!(
        assessment
            .outstanding_acknowledgements()
            .contains(&RequiredAcknowledgement::Irreversible),
        "§19.4: plans containing known irreversible actions require an explicit \
         `accept_irreversible` acknowledgement, independently of the class"
    );
}

#[test]
fn should_not_raise_irreversibility_for_an_ordinary_file_replacement() {
    let conf = file("/etc/nginx/nginx.conf", 4001);
    let action = mutate(0, "replace nginx.conf", id_of(&conf));
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        EffectConfidence::Guaranteed,
        "the file's contents are replaced",
        id_of(&conf),
    ));
    let actions = vec![action];
    let targets = vec![target_of(&conf, "ono.file/1")];
    assert!(!fired(
        &RiskRequest::new(&actions, &targets),
        "risk.irreversibility.effect"
    ));
}

// --- §35 external side effects ------------------------------------------------------------------

#[test]
fn should_raise_external_side_effects_for_an_effect_that_leaves_the_machine() {
    let action = mutate(0, "call deployment webhook", "webhook");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Expected,
        "a deployment webhook is posted to the release service",
        "https://example.invalid/deployments",
    ));
    let actions = vec![action];
    let request = RiskRequest::new(&actions, &[]);
    assert_eq!(
        class_of(&request, "risk.external.side-effect"),
        Some(RiskClass::High),
        "§35.2: such effects are not recoverable through local snapshots and must stay visible"
    );
    let assessment = assess(&request);
    let finding = assessment
        .findings()
        .iter()
        .find(|finding| finding.rule() == "risk.external.side-effect")
        .expect("the external rule fired");
    assert_eq!(finding.dimension(), RiskDimension::ExternalSideEffects);
}

// --- §33 downtime -------------------------------------------------------------------------------

#[test]
fn should_raise_downtime_when_a_restart_interrupts_known_listeners() {
    let unit = service("nginx.service");
    let http = listener(9001, 80);
    let https = listener(9002, 443);
    let index = world(
        &[unit.clone(), http.clone(), https.clone()],
        &[
            edge(&unit, &http, "service.listens_on", Confidence::Exact),
            edge(&unit, &https, "service.listens_on", Confidence::Exact),
        ],
    );
    let action = mutate(0, "restart nginx", id_of(&unit));
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ProcessRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the worker set is replaced",
        id_of(&unit),
    ));
    let actions = vec![action];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = RiskRequest::new(&actions, &targets).over_topology(&index);
    assert_eq!(
        class_of(&request, "risk.downtime.service-restart"),
        Some(RiskClass::Moderate),
        "§33.1: what the listeners were serving ends, whatever the filesystem protection says"
    );
    let assessment = assess(&request);
    let finding = assessment
        .findings()
        .iter()
        .find(|finding| finding.rule() == "risk.downtime.service-restart")
        .expect("the downtime rule fired");
    assert!(
        finding.reason().contains('2'),
        "§40.2: the reason says how many listeners — got `{}`",
        finding.reason()
    );
}

#[test]
fn should_raise_downtime_when_the_provider_declares_the_interruption_itself() {
    let unit = service("nginx.service");
    let action = mutate(0, "restart nginx", id_of(&unit));
    let action = action
        .clone()
        .effecting(effect(
            &action,
            EffectDomain::ProcessRuntime,
            EffectKind::Replace,
            EffectConfidence::Expected,
            "the worker set is replaced",
            id_of(&unit),
        ))
        .effecting(effect(
            &action,
            EffectDomain::NetworkRuntime,
            EffectKind::Interrupt,
            EffectConfidence::Possible,
            "requests being served may be cut",
            id_of(&unit),
        ));
    let actions = vec![action];
    let targets = vec![target_of(&unit, "ono.service/1")];
    assert!(fired(
        &RiskRequest::new(&actions, &targets),
        "risk.downtime.service-restart"
    ));
}

#[test]
fn should_not_raise_downtime_for_a_restart_that_interrupts_nothing_known() {
    let unit = service("batch.service");
    let index = world(std::slice::from_ref(&unit), &[]);
    let action = mutate(0, "restart batch", id_of(&unit));
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ProcessRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the worker set is replaced",
        id_of(&unit),
    ));
    let actions = vec![action];
    let targets = vec![target_of(&unit, "ono.service/1")];
    assert!(
        !fired(
            &RiskRequest::new(&actions, &targets).over_topology(&index),
            "risk.downtime.service-restart"
        ),
        "§8.3 forbids theatre: a service nothing is known to be connected to has no known \
         downtime to declare"
    );
}

// --- §9.6 unknown impact ------------------------------------------------------------------------

#[test]
fn should_raise_unknown_impact_when_the_graph_ends_at_a_boundary() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit))];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let mut graph = ImpactGraph::empty();
    graph.add(ImpactNode::new(
        id_of(&unit),
        "nginx.service",
        "Service",
        ImpactClass::DirectTarget,
        0,
    ));
    graph.add_boundary(UnknownBoundary::new(
        "nginx",
        "external API",
        "beyond Ono visibility",
    ));
    let request = RiskRequest::new(&actions, &targets).over_impact(&graph);
    assert_eq!(
        class_of(&request, "risk.unknown.impact"),
        Some(RiskClass::Unknown),
        "§9.6 and §19.2: a graph that ends where Ono stops seeing is unknown impact"
    );
}

#[test]
fn should_raise_unknown_impact_when_the_graph_was_truncated() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit))];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let graph = ImpactGraph::empty().truncated("the node budget of 3 was reached");
    assert!(
        fired(
            &RiskRequest::new(&actions, &targets).over_impact(&graph),
            "risk.unknown.impact"
        ),
        "§9.5: the parts a bounded graph did not reach are parts nobody has seen"
    );
}

#[test]
fn should_raise_unknown_impact_for_an_effect_the_provider_could_not_classify() {
    let unit = service("app.service");
    let action = mutate(0, "migrate schema", id_of(&unit));
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ApplicationPersistent,
        EffectKind::Modify,
        EffectConfidence::Unknown,
        "the migration's effect on application state could not be established",
        id_of(&unit),
    ));
    let actions = vec![action];
    let targets = vec![target_of(&unit, "ono.service/1")];
    assert_eq!(
        class_of(&RiskRequest::new(&actions, &targets), "risk.unknown.impact"),
        Some(RiskClass::Unknown),
        "§8.1: UNKNOWN must remain visible, and §2.4 forbids promoting it"
    );
}

#[test]
fn should_raise_unknown_impact_for_an_opaque_action() {
    let actions =
        vec![opaque(0, "run vendor installer", "an installer nobody reads").on("vendor-installer")];
    let request = RiskRequest::new(&actions, &[]);
    assert!(fired(&request, "risk.unknown.impact"), "§6.3");
    assert_eq!(
        assess(&request).classify(),
        RiskClass::Unknown,
        "§19.2: UNKNOWN outranks MODERATE, so a plan nobody can bound is not an ordinary one"
    );
}

#[test]
fn should_let_a_serious_finding_outrank_an_unknown_one() {
    let action =
        opaque(0, "run vendor installer", "an installer nobody reads").on("vendor-installer");
    let action = action.clone().effecting(
        effect(
            &action,
            EffectDomain::ExternalSideEffect,
            EffectKind::Emit,
            EffectConfidence::Expected,
            "a licence activation request is sent",
            "https://example.invalid/activate",
        )
        .irreversible(),
    );
    let actions = vec![action];
    assert_eq!(
        assess(&RiskRequest::new(&actions, &[])).classify(),
        RiskClass::High,
        "§19.2: UNKNOWN is outranked by HIGH"
    );
}

#[test]
fn should_not_raise_unknown_impact_for_a_complete_graph_and_a_declared_plan() {
    let unit = service("nginx.service");
    let action = mutate(0, "restart nginx", id_of(&unit));
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ProcessRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the worker set is replaced",
        id_of(&unit),
    ));
    let actions = vec![action];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let graph = ImpactGraph::empty();
    assert!(!fired(
        &RiskRequest::new(&actions, &targets).over_impact(&graph),
        "risk.unknown.impact"
    ));
}

// --- §30.5 reboot -------------------------------------------------------------------------------

#[test]
fn should_raise_the_reboot_dimension_for_a_declared_requirement() {
    let action = mutate(0, "update package openssl", "openssl");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::KernelRuntime,
        EffectKind::Modify,
        EffectConfidence::Expected,
        "the running kernel keeps the old library until the host is rebooted",
        REBOOT_REQUIRED,
    ));
    let actions = vec![action];
    let request = RiskRequest::new(&actions, &[]);
    assert_eq!(
        class_of(&request, "risk.reboot.required"),
        Some(RiskClass::High),
        "§30.5: a reboot requirement ends every process on the host"
    );
}

#[test]
fn should_not_raise_the_reboot_dimension_for_a_recommendation() {
    let action = mutate(0, "update package openssl", "openssl");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::KernelRuntime,
        EffectKind::Modify,
        EffectConfidence::Possible,
        "a reboot is recommended so every process picks up the new library",
        REBOOT_SUGGESTED,
    ));
    let actions = vec![action];
    assert!(
        !fired(&RiskRequest::new(&actions, &[]), "risk.reboot.required"),
        "§30.5: the provider MUST distinguish requirement from suggestion, and so must the risk \
         model"
    );
}

// --- §29 remote ---------------------------------------------------------------------------------

#[test]
fn should_raise_remote_fanout_for_a_plan_that_reaches_two_hosts() {
    let targets = vec![
        target_named("ono.service/1", "api-01:nginx", "nginx").on_host("api-01"),
        target_named("ono.service/1", "api-02:nginx", "nginx").on_host("api-02"),
    ];
    let actions = vec![
        mutate(0, "restart nginx on api-01", "api-01:nginx"),
        mutate(1, "restart nginx on api-02", "api-02:nginx"),
    ];
    let request = RiskRequest::new(&actions, &targets);
    assert_eq!(
        class_of(&request, "risk.remote.fanout"),
        Some(RiskClass::Moderate),
        "§29.1: a remote plan is decomposed per host and has no global atomicity"
    );
}

#[test]
fn should_raise_remote_fanout_high_for_a_large_fleet() {
    let targets: Vec<FrozenTarget> = (0..20)
        .map(|host| {
            target_named("ono.service/1", &format!("api-{host:02}:nginx"), "nginx")
                .on_host(format!("api-{host:02}"))
        })
        .collect();
    let actions: Vec<PlanAction> = (0..20)
        .map(|host| {
            mutate(
                host,
                &format!("restart nginx on api-{host:02}"),
                &format!("api-{host:02}:nginx"),
            )
        })
        .collect();
    let request = RiskRequest::new(&actions, &targets);
    assert_eq!(
        class_of(&request, "risk.remote.fanout"),
        Some(RiskClass::High),
        "§29.2's own example is a 20-host plan, and the reach itself is the risk"
    );
}

#[test]
fn should_not_raise_remote_fanout_for_a_single_host_plan() {
    let targets = vec![target_named("ono.service/1", "nginx", "nginx").on_host("testbox")];
    let actions = vec![mutate(0, "restart nginx", "nginx")];
    assert!(!fired(
        &RiskRequest::new(&actions, &targets),
        "risk.remote.fanout"
    ));
}

#[test]
fn should_treat_a_plan_that_may_remove_the_active_links_path_as_critical() {
    let link = ActiveLink::new("prod-router")
        .via_route("route:default")
        .via_interface("interface:eth0");
    let action = mutate(0, "set route default via 10.0.0.254", "route:default");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::NetworkRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the default route is replaced",
        "route:default",
    ));
    let actions = vec![action];
    let request = RiskRequest::new(&actions, &[]).over_link(&link);
    assert_eq!(
        class_of(&request, "risk.remote.link-loss"),
        Some(RiskClass::Critical),
        "§34.2: a plan that may remove the path used by the active remote Ono link MUST be a \
         CRITICAL risk landmark"
    );
    assert_eq!(assess(&request).classify(), RiskClass::Critical);
    let assessment = assess(&request);
    let finding = assessment
        .findings()
        .iter()
        .find(|finding| finding.rule() == "risk.remote.link-loss")
        .expect("§34.2's rule fired");
    assert!(
        finding.reason().contains("transport path"),
        "Appendix I.3's reason reads `proposed route may remove transport path used by this \
         active Ono link` — got `{}`",
        finding.reason()
    );
}

#[test]
fn should_not_treat_an_unrelated_route_change_as_link_loss() {
    let link = ActiveLink::new("prod-router").via_route("route:default");
    let action = mutate(0, "set route 10.9.0.0/16 via 10.0.0.9", "route:10.9.0.0-16");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::NetworkRuntime,
        EffectKind::Create,
        EffectConfidence::Expected,
        "a route to the lab network is added",
        "route:10.9.0.0-16",
    ));
    let actions = vec![action];
    assert!(
        !fired(
            &RiskRequest::new(&actions, &[]).over_link(&link),
            "risk.remote.link-loss"
        ),
        "§34.2 is about the path this link uses; every other route change is ordinary"
    );
}

#[test]
fn should_not_claim_link_loss_without_a_link() {
    let action = mutate(0, "set route default via 10.0.0.254", "route:default");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::NetworkRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the default route is replaced",
        "route:default",
    ));
    let actions = vec![action];
    assert!(
        !fired(&RiskRequest::new(&actions, &[]), "risk.remote.link-loss"),
        "a local session has no link to lose (§34.2)"
    );
}

#[test]
fn should_treat_removing_the_links_interface_as_link_loss_too() {
    let link = ActiveLink::new("prod-router").via_interface("interface:eth0");
    let action = mutate(0, "set interface eth0 down", "interface:eth0");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::NetworkRuntime,
        EffectKind::Modify,
        EffectConfidence::Guaranteed,
        "the interface goes down",
        "interface:eth0",
    ));
    let actions = vec![action];
    assert_eq!(
        class_of(
            &RiskRequest::new(&actions, &[]).over_link(&link),
            "risk.remote.link-loss"
        ),
        Some(RiskClass::Critical),
        "§34.2 is about the path, and the interface is part of it"
    );
}

// --- §19.1 recovery complexity ------------------------------------------------------------------

#[test]
fn should_raise_recovery_complexity_for_an_offline_method() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit))];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let methods = [RestoreMethod::OfflineRootRecovery];
    let request = RiskRequest::new(&actions, &targets).with_recovery(&methods);
    assert_eq!(
        class_of(&request, "risk.recovery.complexity"),
        Some(RiskClass::High),
        "§13.7 and §14.6: recovery that cannot run from the running system is a different \
         proposition under pressure"
    );
}

#[test]
fn should_raise_recovery_complexity_more_gently_for_a_next_boot_method() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit))];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let methods = [RestoreMethod::SubvolumeReplacement];
    let request = RiskRequest::new(&actions, &targets).with_recovery(&methods);
    assert_eq!(
        class_of(&request, "risk.recovery.complexity"),
        Some(RiskClass::Moderate),
        "§14.4: a subvolume replacement is prepared while the system runs and completes at the \
         next boot"
    );
}

#[test]
fn should_not_raise_recovery_complexity_for_a_selective_restore() {
    let unit = service("nginx.service");
    let actions = vec![mutate(0, "restart nginx", id_of(&unit))];
    let targets = vec![target_of(&unit, "ono.service/1")];
    let methods = [RestoreMethod::SelectiveFileRestore];
    assert!(
        !fired(
            &RiskRequest::new(&actions, &targets).with_recovery(&methods),
            "risk.recovery.complexity"
        ),
        "Appendix C.1 puts selective restore among the least destructive methods"
    );
}

// --- §19.4 gates --------------------------------------------------------------------------------

#[test]
fn should_require_an_acknowledgement_for_a_critical_plan() {
    let fleet = Fleet::of(40);
    let assessment =
        assess(&RiskRequest::new(&fleet.actions, &fleet.targets).over_topology(&fleet.index));
    assert!(
        assessment
            .outstanding_acknowledgements()
            .contains(&RequiredAcknowledgement::Risk(RiskClass::Critical)),
        "§19.4: HIGH and CRITICAL plans require explicit interactive acknowledgement or a \
         non-interactive policy flag"
    );
}

#[test]
fn should_require_no_acknowledgement_for_a_moderate_plan() {
    let conf = file("/etc/nginx/nginx.conf", 4001);
    let actions = vec![mutate(0, "replace nginx.conf", id_of(&conf))];
    let targets = vec![target_of(&conf, "ono.file/1")];
    assert!(
        assess(&RiskRequest::new(&actions, &targets))
            .outstanding_acknowledgements()
            .is_empty(),
        "§40.1: `apply` is the intentional commitment, and an ordinary change needs nothing more"
    );
}

/// A fleet of services that one control group proves to share a role (§28.3).
struct Fleet {
    index: SpatialIndex,
    targets: Vec<FrozenTarget>,
    actions: Vec<PlanAction>,
}

impl Fleet {
    fn of(count: usize) -> Self {
        let group = cgroup("/system.slice/frontend.slice");
        let members: Vec<_> = (0..count)
            .map(|index| service(&format!("frontend-{index:02}.service")))
            .collect();
        let mut objects = vec![group.clone()];
        objects.extend(members.iter().cloned());
        let edges: Vec<_> = members
            .iter()
            .map(|member| edge(member, &group, "service.in_cgroup", Confidence::Exact))
            .collect();
        let targets = members
            .iter()
            .map(|member| target_of(member, "ono.service/1"))
            .collect();
        let actions = members
            .iter()
            .enumerate()
            .map(|(ordinal, member)| {
                let action = mutate(
                    ordinal,
                    &format!("restart {}", member.display_name()),
                    id_of(member),
                );
                action.clone().effecting(effect(
                    &action,
                    EffectDomain::ProcessRuntime,
                    EffectKind::Replace,
                    EffectConfidence::Expected,
                    "the worker set is replaced",
                    id_of(member),
                ))
            })
            .collect();
        Self {
            index: world(&objects, &edges),
            targets,
            actions,
        }
    }
}
