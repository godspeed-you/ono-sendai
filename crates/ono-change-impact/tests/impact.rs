//! Impact derivation over the v0.4 topology — spec v0.6 §3.5, §9, §35, §52.2, §62.11.
//!
//! §9.1 fixes the question these tests hold the implementation to: *"What known parts of the
//! system could this plan touch directly or indirectly?"* — and not what will fail. So every
//! assertion here is about what is in the graph, how it got there, and where Ono stopped being
//! able to say.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{EffectConfidence, EffectDomain, EffectKind, ImpactClass, ImpactNode};
use ono_change_impact::{
    HistoricalRelevance, ImpactRequest, Prominence, derive, derive_in_detail, derive_within_budget,
    estimate,
};
use ono_spatial_core::{Confidence, PermissionState, SpatialId, ValidityWindow};
use ono_spatial_index::SpatialIndex;
use ono_spatial_query::INTERACTIVE_BUDGET;

mod common;

use common::{
    NOW, cgroup, edge, effect, file, host, id_of, listener, mutate, opaque, process, service,
    target_named, target_of, world,
};

/// §9.3's own drawing, as an index: a configuration file, the process that reads it, the sockets
/// that process owns, and the unit that controls it.
struct Nginx {
    index: SpatialIndex,
    conf: String,
    daemon: String,
    unit: String,
    http: String,
    https: String,
}

fn nginx_world() -> Nginx {
    let conf = file("/etc/nginx/nginx.conf", 4001);
    let daemon = process(1842, "nginx");
    let unit = service("nginx.service");
    let http = listener(9001, 80);
    let https = listener(9002, 443);
    let index = world(
        &[
            conf.clone(),
            daemon.clone(),
            unit.clone(),
            http.clone(),
            https.clone(),
        ],
        &[
            edge(&daemon, &conf, "process.opened_file", Confidence::Exact),
            edge(&daemon, &http, "process.owns_socket", Confidence::Exact),
            edge(&daemon, &https, "process.owns_socket", Confidence::Exact),
            edge(
                &unit,
                &daemon,
                "service.controls_process",
                Confidence::Exact,
            ),
        ],
    );
    Nginx {
        index,
        conf: id_of(&conf).to_owned(),
        daemon: id_of(&daemon).to_owned(),
        unit: id_of(&unit).to_owned(),
        http: id_of(&http).to_owned(),
        https: id_of(&https).to_owned(),
    }
}

fn node<'a>(graph: &'a ono_change_core::ImpactGraph, id: &str) -> &'a ImpactNode {
    graph
        .nodes()
        .iter()
        .find(|node| node.id() == id)
        .unwrap_or_else(|| panic!("`{id}` is in the impact graph"))
}

#[test]
fn should_class_a_plan_target_as_a_direct_target() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, &nginx.conf).class(),
        ImpactClass::DirectTarget,
        "§9.2: the plan names this object"
    );
}

#[test]
fn should_keep_a_target_the_index_has_never_seen_in_the_graph() {
    let index = world(&[], &[]);
    let targets = vec![target_named("ono.file/1", "/etc/app/config", "config")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        graph.blast_radius().direct_targets,
        1,
        "§9.5's summary must agree with the plan even where the index has seen nothing"
    );
    assert_eq!(node(&graph, "/etc/app/config").label(), "config");
}

#[test]
fn should_class_an_object_named_by_an_effect_as_a_direct_effect() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let action = mutate(0, "replace /etc/nginx/nginx.conf", &nginx.conf).effecting(effect(
        &mutate(0, "replace /etc/nginx/nginx.conf", &nginx.conf),
        EffectDomain::ProcessRuntime,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the worker set is replaced",
        &nginx.daemon,
    ));
    let actions = vec![action];
    let request = ImpactRequest::new(&nginx.index, &targets, &actions, NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, &nginx.daemon).class(),
        ImpactClass::DirectEffect,
        "§9.2: an action's declared effect lands on this object"
    );
}

#[test]
fn should_class_one_relation_hop_as_a_dependent() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, &nginx.daemon).class(),
        ImpactClass::Dependent,
        "§9.2: one relation hop from the plan's target"
    );
}

#[test]
fn should_class_more_than_one_relation_hop_as_transitive_related() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    for far in [&nginx.http, &nginx.https, &nginx.unit] {
        assert_eq!(
            node(&graph, far).class(),
            ImpactClass::TransitiveRelated,
            "§9.2: reached through more than one relation"
        );
    }
}

#[test]
fn should_count_the_blast_radius_of_the_configuration_example() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let radius = derive(&request).blast_radius();
    assert_eq!(radius.direct_targets, 1, "§9.5: one direct target");
    assert_eq!(
        radius.dependents, 1,
        "§9.5: the process that reads the file"
    );
    assert_eq!(
        radius.transitive, 3,
        "§9.5: the two listeners and the unit, reached through the process"
    );
}

#[test]
fn should_class_an_external_side_effect_domain_as_an_external_side_effect() {
    let index = world(&[], &[]);
    let action = mutate(0, "call deployment webhook", "webhook");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Expected,
        "an outbound HTTPS request is made",
        "external API",
    ));
    let actions = vec![action];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, "external API").class(),
        ImpactClass::ExternalSideEffect,
        "§9.2 and §35.1: an effect that leaves the machine"
    );
}

#[test]
fn should_class_a_remote_system_effect_as_an_external_side_effect() {
    let index = world(&[], &[]);
    let action = mutate(0, "restart service on api-07", "api-07:nginx");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::RemoteSystem,
        EffectKind::Replace,
        EffectConfidence::Expected,
        "the remote unit is restarted",
        "api-07:nginx",
    ));
    let actions = vec![action];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, "api-07:nginx").class(),
        ImpactClass::ExternalSideEffect,
        "§9.2: state on another host is outside this machine's graph"
    );
}

#[test]
fn should_carry_the_relation_label_that_reached_a_node() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, &nginx.daemon).relation(),
        Some("opener"),
        "§3.5: the node says which v0.4 relation reached it, in the word `follow` takes at that \
         end — the file's `opener`"
    );
}

#[test]
fn should_carry_the_v04_confidence_of_the_edge_exactly_as_v04_spells_it() {
    let unit = service("nginx.service");
    let backend = service("backend.service");
    let index = world(
        &[unit.clone(), backend.clone()],
        &[edge(
            &unit,
            &backend,
            "service.depends_on",
            Confidence::Inferred,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, id_of(&backend)).confidence(),
        Confidence::Inferred.as_str(),
        "§3.5 and §1.3: an inferred v0.4 edge must not become anything stronger on the way into \
         the impact graph"
    );
}

#[test]
fn should_carry_a_user_declared_confidence_under_its_own_v04_name() {
    let unit = service("nginx.service");
    let backend = service("backend.service");
    let index = world(
        &[unit.clone(), backend.clone()],
        &[edge(
            &unit,
            &backend,
            "service.depends_on",
            Confidence::UserDeclared,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, id_of(&backend)).confidence(),
        "user_declared",
        "§3.5: the confidence travels in v0.4's own spelling, not in a re-worded one"
    );
}

#[test]
fn should_cite_the_edge_that_put_a_node_in_the_graph() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    let evidence: Vec<String> = node(&graph, &nginx.daemon)
        .evidence()
        .iter()
        .map(|citation| citation.to_string())
        .collect();
    assert!(
        evidence
            .iter()
            .any(|citation| citation.starts_with("edge:")),
        "§3.5 and v0.4 §2.5: the edge id is the evidence that explains why two objects are \
         related, and it must survive into impact — got {evidence:?}"
    );
}

#[test]
fn should_cite_the_canonical_object_ref_of_every_node_the_index_holds() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    for node in graph.nodes() {
        assert!(
            node.evidence()
                .iter()
                .any(|citation| citation.starts_with("ref:")),
            "§46.7: the impact graph preserves the canonical object refs of v0.4"
        );
    }
}

#[test]
fn should_state_the_plans_own_claim_on_a_direct_target_as_exact() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, &nginx.conf).confidence(),
        Confidence::Exact.as_str(),
        "§7.1 froze this target against a resolved object, so the plan's claim on it is exact"
    );
    assert_eq!(
        node(&graph, &nginx.conf).relation(),
        None,
        "no relation reached a direct target, and inventing one would be provenance nobody \
         asserted (§3.5)"
    );
}

#[test]
fn should_keep_the_effect_confidence_on_a_direct_effect() {
    let index = world(&[], &[]);
    let action = mutate(0, "call deployment webhook", "webhook");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::FilesystemPersistent,
        EffectKind::Modify,
        EffectConfidence::Possible,
        "the cache directory may be rewritten",
        "/var/cache/app",
    ));
    let actions = vec![action];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, "/var/cache/app").confidence(),
        EffectConfidence::Possible.as_str(),
        "§8.1: what Ono can say about the effect is what the node says about the object"
    );
}

#[test]
fn should_keep_the_direct_target_reading_of_an_object_a_relation_also_reaches() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
        target_named("ono.process/1", nginx.daemon.as_str(), "nginx")
            .at_place(nginx.daemon.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, &nginx.daemon).class(),
        ImpactClass::DirectTarget,
        "§9.2's classes describe how close the plan comes, so the closest reading wins"
    );
}

#[test]
fn should_visit_an_object_once_even_when_two_relations_reach_it() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
        target_named("ono.service/1", nginx.unit.as_str(), "nginx.service")
            .at_place(nginx.unit.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    let daemons = graph
        .nodes()
        .iter()
        .filter(|node| node.id() == nginx.daemon)
        .count();
    assert_eq!(daemons, 1, "one object is one node in the graph");
}

#[test]
fn should_not_walk_past_an_object_the_index_does_not_hold() {
    let unit = service("nginx.service");
    let daemon = process(1842, "nginx");
    // The process is never registered: the provider asserted the edge, and the index holds no
    // place for the far end.
    let index = world(
        std::slice::from_ref(&unit),
        &[edge(
            &unit,
            &daemon,
            "service.controls_process",
            Confidence::Exact,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        graph.nodes().len(),
        1,
        "§2.16 of v0.4: the impact graph shows objects a provider described, never one composed \
         out of an edge"
    );
}

#[test]
fn should_not_follow_a_relationship_whose_validity_window_has_closed() {
    let unit = service("nginx.service");
    let daemon = process(1842, "nginx");
    let closed = edge(
        &unit,
        &daemon,
        "service.controls_process",
        Confidence::Exact,
    )
    .valid(ValidityWindow::new(None, Some(NOW)));
    let index = world(&[unit.clone(), daemon.clone()], &[closed]);
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        graph.nodes().len(),
        1,
        "§3.5: a relationship the provider says has ended is not a path this plan travels along"
    );
}

#[test]
fn should_follow_a_relationship_whose_end_nobody_knows() {
    let unit = service("nginx.service");
    let daemon = process(1842, "nginx");
    let open = edge(
        &unit,
        &daemon,
        "service.controls_process",
        Confidence::Exact,
    )
    .valid(ValidityWindow::since(NOW));
    let index = world(&[unit.clone(), daemon.clone()], &[open]);
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        graph.nodes().len(),
        2,
        "v0.4 §2.17: an absent end is not `now`, and not knowing when something ended is not \
         knowing that it did"
    );
}

#[test]
fn should_record_an_unknown_boundary_when_an_exit_was_withheld_by_permission() {
    let daemon = process(1842, "nginx");
    let mut index = world(std::slice::from_ref(&daemon), &[]);
    index.record_withheld(
        daemon.spatial_id(),
        "files",
        PermissionState::PermissionDenied,
        "permission denied for 14 process FDs",
    );
    let targets = vec![target_of(&daemon, "ono.process/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    let boundary = graph
        .boundaries()
        .iter()
        .find(|boundary| boundary.beyond().contains("files"))
        .expect("§9.6: a withheld exit is a boundary, not an empty collection");
    assert!(
        boundary.reason().contains("permission_denied"),
        "v0.4 §42.4: denied information produces `permission_denied`, never a false empty \
         collection — got `{}`",
        boundary.reason()
    );
}

#[test]
fn should_not_record_a_boundary_for_an_exit_that_was_read_and_is_empty() {
    let daemon = process(1842, "nginx");
    let mut index = world(std::slice::from_ref(&daemon), &[]);
    index.record_withheld(daemon.spatial_id(), "files", PermissionState::Empty, "");
    let targets = vec![target_of(&daemon, "ono.process/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert!(
        graph.boundaries().is_empty(),
        "§9.6 marks where Ono cannot see, and an exit that was read and held nothing is not one"
    );
}

#[test]
fn should_record_an_unknown_boundary_for_a_relation_acquired_at_external_cost() {
    let far = host("prod-router");
    let index = world(std::slice::from_ref(&far), &[]);
    let targets = vec![target_of(&far, "ono.host/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW);
    let graph = derive(&request);
    assert!(
        graph
            .boundaries()
            .iter()
            .any(|boundary| boundary.reason().contains("external")),
        "§9.6 and v0.4 §34.2: an exit that would need another host is where impact stops, and it \
         must be visible"
    );
}

#[test]
fn should_record_the_ninety_six_boundary_for_an_effect_that_leaves_the_machine() {
    let index = world(&[], &[]);
    let action = mutate(0, "call deployment webhook", "nginx");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ExternalSideEffect,
        EffectKind::Emit,
        EffectConfidence::Expected,
        "an outbound HTTPS request is made",
        "external API",
    ));
    let actions = vec![action];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    let boundary = graph
        .boundaries()
        .first()
        .expect("§9.6: the graph ends at an opaque boundary and the boundary must be visible");
    assert_eq!(boundary.at(), "nginx", "§9.6's `nginx -> …` shape");
    assert_eq!(
        boundary.beyond(),
        "external API",
        "§9.6's `… -> external API`"
    );
    assert!(
        boundary.reason().contains("beyond Ono visibility"),
        "§9.6 spells it `? beyond Ono visibility` — got `{}`",
        boundary.reason()
    );
    assert_eq!(
        graph.blast_radius().boundaries,
        1,
        "§9.5 counts boundaries beside objects so the summary cannot hide them"
    );
}

#[test]
fn should_mark_the_graph_truncated_when_the_node_budget_is_reached() {
    let unit = service("nginx.service");
    let workers: Vec<_> = (0..5).map(|n| process(2000 + n, "nginx")).collect();
    let mut objects = vec![unit.clone()];
    objects.extend(workers.iter().cloned());
    let edges: Vec<_> = workers
        .iter()
        .map(|worker| edge(&unit, worker, "service.controls_process", Confidence::Exact))
        .collect();
    let index = world(&objects, &edges);
    let targets = vec![target_of(&unit, "ono.service/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW).within_nodes(3);
    let graph = derive(&request);
    assert!(
        !graph.is_complete(),
        "§9.5: a bounded summary must not read as a whole graph"
    );
    assert!(
        graph
            .truncation()
            .is_some_and(|reason| reason.contains("node budget")),
        "the reason names the budget that stopped the walk — got {:?}",
        graph.truncation()
    );
}

#[test]
fn should_mark_the_graph_truncated_when_the_depth_budget_stops_the_walk() {
    let first = process(100, "init");
    let second = process(200, "sshd");
    let third = process(300, "bash");
    let index = world(
        &[first.clone(), second.clone(), third.clone()],
        &[
            edge(&first, &second, "process.parent_of", Confidence::Exact),
            edge(&second, &third, "process.parent_of", Confidence::Exact),
        ],
    );
    let targets = vec![target_of(&first, "ono.process/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW).to_depth(1);
    let graph = derive(&request);
    assert!(
        graph
            .truncation()
            .is_some_and(|reason| reason.contains("depth budget")),
        "§52.2 permits bounding the walk, and §9.5 forbids a bounded graph that looks complete — \
         got {:?}",
        graph.truncation()
    );
}

#[test]
fn should_leave_a_graph_that_reached_the_end_of_the_world_complete() {
    let first = process(100, "init");
    let second = process(200, "sshd");
    let third = process(300, "bash");
    let index = world(
        &[first.clone(), second.clone(), third.clone()],
        &[
            edge(&first, &second, "process.parent_of", Confidence::Exact),
            edge(&second, &third, "process.parent_of", Confidence::Exact),
        ],
    );
    let targets = vec![target_of(&first, "ono.process/1")];
    let request = ImpactRequest::new(&index, &targets, &[], NOW).to_depth(3);
    let graph = derive(&request);
    assert!(
        graph.is_complete(),
        "a walk that ran out of world rather than out of budget is complete (§9.5)"
    );
    assert_eq!(graph.nodes().len(), 3);
}

#[test]
fn should_refuse_a_traversal_whose_estimate_is_beyond_the_interactive_budget() {
    let fleet = fleet_world(40);
    let targets = vec![target_of(&fleet.members[0], "ono.service/1")];
    let request = ImpactRequest::new(&fleet.index, &targets, &[], NOW).to_depth(400);
    let estimate = estimate(&request);
    assert!(
        estimate.exceeds(INTERACTIVE_BUDGET),
        "a walk of {} units over {} candidates is beyond v0.4.1 §33.3's budget",
        estimate.units(),
        estimate.candidates()
    );
    let refusal = derive_within_budget(&request).expect_err("§33.3 refuses rather than hanging");
    assert!(
        refusal.message().contains("interactive budget"),
        "the refusal names the budget rather than saying `too expensive` — got `{}`",
        refusal.message()
    );
}

#[test]
fn should_answer_a_traversal_the_operator_accepted_the_cost_of() {
    let fleet = fleet_world(40);
    let targets = vec![target_of(&fleet.members[0], "ono.service/1")];
    let request = ImpactRequest::new(&fleet.index, &targets, &[], NOW)
        .to_depth(400)
        .accepting_cost();
    assert!(
        derive_within_budget(&request).is_ok(),
        "v0.4.1 §34.3: the refusal exists to stop work nobody asked for, and this was asked for"
    );
}

#[test]
fn should_answer_an_ordinary_traversal_without_asking_about_cost() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let derivation =
        derive_within_budget(&request).expect("an indexed single-service walk is interactive");
    assert_eq!(
        derivation.graph().blast_radius().direct_targets,
        1,
        "§52.2: impact over an already indexed world is the cheap case"
    );
}

#[test]
fn should_not_raise_a_nodes_confidence_when_the_ledger_has_seen_the_relationship_before() {
    let unit = service("nginx.service");
    let backend = service("backend.service");
    let index = world(
        &[unit.clone(), backend.clone()],
        &[edge(
            &unit,
            &backend,
            "service.depends_on",
            Confidence::Inferred,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let without = derive(&ImpactRequest::new(&index, &targets, &[], NOW));
    let history = |_: &SpatialId| {
        Some(HistoricalRelevance::new(
            9,
            "restart service",
            "this relationship changed with every restart of nginx",
        ))
    };
    let with = derive(&ImpactRequest::new(&index, &targets, &[], NOW).with_history(&history));
    assert_eq!(
        node(&with, id_of(&backend)).confidence(),
        node(&without, id_of(&backend)).confidence(),
        "§9.4: evidence may show history, and MUST NOT upgrade correlation to causation"
    );
    assert_eq!(
        node(&with, id_of(&backend)).confidence(),
        Confidence::Inferred.as_str(),
        "§62.11: nothing may upgrade unknown to guaranteed, and nothing may upgrade inferred \
         either"
    );
}

#[test]
fn should_raise_the_prominence_of_a_relationship_the_ledger_has_seen_repeatedly() {
    let unit = service("nginx.service");
    let backend = service("backend.service");
    let index = world(
        &[unit.clone(), backend.clone()],
        &[edge(
            &unit,
            &backend,
            "service.depends_on",
            Confidence::Inferred,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let history = |_: &SpatialId| {
        Some(HistoricalRelevance::new(
            9,
            "restart service",
            "this relationship changed with every restart of nginx",
        ))
    };
    let derivation =
        derive_in_detail(&ImpactRequest::new(&index, &targets, &[], NOW).with_history(&history));
    assert_eq!(
        derivation.prominence(id_of(&backend)),
        Prominence::Raised,
        "§9.4: v0.5 evidence MAY strengthen impact relevance"
    );
    assert!(derivation.raised().contains(&id_of(&backend)));
}

#[test]
fn should_cite_the_ledger_beside_the_node_it_raised() {
    let unit = service("nginx.service");
    let backend = service("backend.service");
    let index = world(
        &[unit.clone(), backend.clone()],
        &[edge(
            &unit,
            &backend,
            "service.depends_on",
            Confidence::Exact,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let history = |_: &SpatialId| Some(HistoricalRelevance::new(4, "restart service", "seen"));
    let graph = derive(&ImpactRequest::new(&index, &targets, &[], NOW).with_history(&history));
    assert!(
        node(&graph, id_of(&backend))
            .evidence()
            .iter()
            .any(|citation| citation.starts_with("ledger:")),
        "§9.4: Ono MAY show that history as evidence"
    );
}

#[test]
fn should_leave_a_node_ordinary_when_the_ledger_saw_it_once() {
    let unit = service("nginx.service");
    let backend = service("backend.service");
    let index = world(
        &[unit.clone(), backend.clone()],
        &[edge(
            &unit,
            &backend,
            "service.depends_on",
            Confidence::Exact,
        )],
    );
    let targets = vec![target_of(&unit, "ono.service/1")];
    let history = |_: &SpatialId| Some(HistoricalRelevance::new(1, "restart service", "seen once"));
    let derivation =
        derive_in_detail(&ImpactRequest::new(&index, &targets, &[], NOW).with_history(&history));
    assert_eq!(
        derivation.prominence(id_of(&backend)),
        Prominence::Ordinary,
        "§9.4 speaks of relationships the ledger has repeatedly observed"
    );
}

#[test]
fn should_order_the_graph_closest_to_the_plan_first() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    let classes: Vec<u8> = graph
        .nodes()
        .iter()
        .map(|node| node.class().distance())
        .collect();
    let mut sorted = classes.clone();
    sorted.sort_unstable();
    assert_eq!(
        classes, sorted,
        "§20.1 reads the plan outward from what it names, so the graph is ordered that way"
    );
}

#[test]
fn should_derive_the_same_graph_from_the_same_world_twice() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    assert_eq!(
        derive(&request),
        derive(&request),
        "v0.4 §29.3: the same index answers the same question the same way, which is what makes \
         a plan digest stable (§4.4)"
    );
}

#[test]
fn should_record_the_host_every_node_lives_on() {
    let nginx = nginx_world();
    let targets = vec![
        target_named("ono.file/1", nginx.conf.as_str(), "nginx.conf").at_place(nginx.conf.as_str()),
    ];
    let request = ImpactRequest::new(&nginx.index, &targets, &[], NOW);
    let graph = derive(&request);
    assert_eq!(
        graph.blast_radius().hosts,
        1,
        "§29.1: per-host truth begins with knowing which host each object is on"
    );
}

#[test]
fn should_take_an_opaque_action_without_inventing_what_it_touches() {
    let index = world(&[], &[]);
    let actions = vec![opaque(
        0,
        "run vendor installer",
        "an installer nobody can read",
    )];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    assert!(
        graph.nodes().is_empty(),
        "§6.3: an opaque action's scope is not something impact may guess at"
    );
}

/// A fleet of `count` services proven to share one role by the control group they are in.
struct Fleet {
    index: SpatialIndex,
    members: Vec<ono_spatial_core::SpatialObject>,
    group: ono_spatial_core::SpatialObject,
}

fn fleet_world(count: usize) -> Fleet {
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
    Fleet {
        index: world(&objects, &edges),
        members,
        group,
    }
}

#[test]
fn should_reach_the_whole_role_from_one_member_through_the_group() {
    let fleet = fleet_world(5);
    let targets = vec![target_of(&fleet.members[0], "ono.service/1")];
    let request = ImpactRequest::new(&fleet.index, &targets, &[], NOW).to_depth(2);
    let graph = derive(&request);
    assert_eq!(
        node(&graph, id_of(&fleet.group)).class(),
        ImpactClass::Dependent,
        "§9.3: the control group is one relation from the service"
    );
    assert_eq!(
        graph.blast_radius().transitive,
        4,
        "§9.5: the other four members of the role, reached through the group"
    );
}

#[test]
fn should_end_the_graph_at_an_unknown_boundary_where_an_effects_domain_is_unknown() {
    let index = world(&[], &[]);
    let action = opaque(0, "opaque action: touch work/opq", "touch work/opq");
    // §6.3's escape, as `plan --opaque` builds it: an effect in a domain Ono cannot classify,
    // naming no object, because what the command touches is exactly what nobody knows.
    let unknown = ono_change_core::ProposedEffect::new(
        action.id().clone(),
        EffectDomain::Unknown,
        EffectKind::Unknown,
        EffectConfidence::Unknown,
        "Ono cannot reason about what this command touches",
    );
    let actions = vec![action.effecting(unknown)];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    let boundary = graph.boundaries().first().expect(
        "§6.3 classifies an opaque action's impact as unknown, and §9.6 requires the place the \
         graph ends to be visible rather than absent",
    );
    assert!(
        boundary.beyond().contains("touch work/opq") || boundary.at().contains("touch work/opq"),
        "the boundary names the action whose effects Ono cannot follow — got at `{}`, beyond `{}`",
        boundary.at(),
        boundary.beyond()
    );
    assert!(
        !graph.is_complete(),
        "§2.4: an impact Ono cannot establish MUST NOT be reported as a complete graph"
    );
    assert!(
        graph.nodes().is_empty(),
        "§1.3: the effect names no object, so impact may not invent one"
    );
}

#[test]
fn should_end_the_graph_at_an_unknown_boundary_where_an_effects_confidence_is_unknown() {
    let index = world(&[], &[]);
    let action = mutate(0, "reload vendor agent", "vendor-agent");
    let action = action.clone().effecting(effect(
        &action,
        EffectDomain::ProcessRuntime,
        EffectKind::Modify,
        EffectConfidence::Unknown,
        "the agent may re-read configuration nobody declared",
        "vendor-agent",
    ));
    let actions = vec![action];
    let request = ImpactRequest::new(&index, &[], &actions, NOW);
    let graph = derive(&request);
    assert_eq!(
        graph.blast_radius().boundaries,
        1,
        "§8.1: an UNKNOWN effect remains visible, and in the graph that is a boundary (§9.6)"
    );
    assert!(
        !graph.is_complete(),
        "§2.4: an UNKNOWN effect is not silently promoted to a complete impact"
    );
}
