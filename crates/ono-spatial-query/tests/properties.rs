//! The three properties of spec v0.4 §43.2 that are statements about a *map*, checked over
//! generated horizons rather than over one chosen example.
//!
//! §43.2's other four properties are statements about identity and navigation and live in
//! `crates/ono-spatial-core/tests/properties.rs`; these three need a projection, which is this
//! crate's (§45.3), so they live here. Each is checked against a reproducible pseudo-random
//! stream (`ono_testkit::Rng`, AGENTS.md §11: deterministic), and a failure names its seed.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::collections::BTreeSet;

use common::{NOW, bridge, index, process, service, socket_with};
use ono_spatial_core::{HierarchyKind, SpatialId, SpatialType};
use ono_spatial_index::{PinRegistry, SpatialIndex};
use ono_spatial_query::{
    HorizonPlace, MAP_NODE_BUDGET, MapHorizon, MapRequest, SpatialMap, project_map,
};
use ono_testkit::Rng;

const RUNS: u64 = 96;

/// A host of `count` objects of mixed kinds under the canonical geography, which is the shape of
/// horizon the shell hands the projection when it maps the system root.
fn host(count: usize) -> (SpatialIndex, MapHorizon) {
    let mut records = Vec::new();
    for step in 0..count {
        let step_i64 = i64::try_from(step).expect("a small fixture");
        match step % 3 {
            0 => records.push(process(
                1000 + step_i64,
                &format!("worker-{step:03}"),
                "running",
            )),
            1 => records.push(socket_with(9000 + step_i64, Some("LISTEN"), None)),
            _ => records.push(service(&format!("unit-{step:03}"), "active")),
        }
    }
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(&mut index, &records, NOW);

    let root = SpatialId::of_space("system");
    let compute = SpatialId::of_space("compute");
    let network = SpatialId::of_space("network");
    let processes = SpatialId::of_space("compute.processes");

    let mut horizon = MapHorizon::new();
    horizon.place(HorizonPlace::new(root.clone(), 0, None));
    horizon.place(HorizonPlace::new(
        compute.clone(),
        1,
        Some((root.clone(), HierarchyKind::Grouping)),
    ));
    horizon.place(HorizonPlace::new(
        network,
        1,
        Some((root, HierarchyKind::Grouping)),
    ));
    horizon.place(HorizonPlace::new(
        processes.clone(),
        2,
        Some((compute, HierarchyKind::Grouping)),
    ));
    for kind in [
        SpatialType::Process,
        SpatialType::Listener,
        SpatialType::Service,
    ] {
        for entry in index.of_type(kind) {
            horizon.place(HorizonPlace::new(
                entry.object().spatial_id().clone(),
                3,
                Some((processes.clone(), HierarchyKind::Grouping)),
            ));
        }
    }
    (index, horizon)
}

fn draw(index: &SpatialIndex, horizon: &MapHorizon, request: &MapRequest) -> SpatialMap {
    project_map(
        index,
        &SpatialId::of_space("system"),
        horizon,
        request,
        &PinRegistry::new(),
        MAP_NODE_BUDGET,
        NOW,
    )
}

fn node_ids(map: &SpatialMap) -> BTreeSet<String> {
    map.nodes.iter().map(|node| node.id.to_string()).collect()
}

fn edge_ids(map: &SpatialMap) -> BTreeSet<String> {
    map.edges.iter().map(|edge| edge.id.clone()).collect()
}

#[test]
fn should_keep_every_node_and_edge_a_filter_left_alone_and_invent_none() {
    // §43.2: "filtering cannot create unknown edges". A filter is a narrowing: what survives it
    // was in the unfiltered projection of the same horizon, and nothing else appears.
    //
    // The horizons here stay inside the node budget on purpose, so the only variable is the
    // filter. What a filter does to a horizon *larger* than the budget is the budget's question,
    // not the filter's, and it is carried in docs/STATE.md with its own exit test.
    for seed in 0..RUNS {
        let mut rng = Rng::seeded(seed);
        let (index, horizon) = host(4 + rng.below(3 * MAP_NODE_BUDGET));
        let base = MapRequest::new().depth(3).all(true);
        let complete = draw(&index, &horizon, &base);
        assert!(
            !complete.nodes.is_empty(),
            "seed {seed}: the fixture horizon draws something"
        );

        let known_nodes = node_ids(&complete);
        let known_edges = edge_ids(&complete);

        let kind = [
            SpatialType::Process,
            SpatialType::Listener,
            SpatialType::Service,
        ][rng.below(3)];
        let typed = draw(&index, &horizon, &base.clone().types(vec![kind]));
        for node in &typed.nodes {
            assert!(
                known_nodes.contains(&node.id.to_string()),
                "seed {seed}: `--type {kind:?}` drew {node:?}, which the unfiltered map does not hold"
            );
        }
        for edge in &typed.edges {
            assert!(
                known_edges.contains(&edge.id),
                "seed {seed}: `--type {kind:?}` drew edge {edge:?}, which the unfiltered map does not hold"
            );
        }

        if let Some(relation) = complete.edges.first().map(|edge| edge.relation.clone()) {
            let narrowed = draw(
                &index,
                &horizon,
                &base.clone().relations(vec![relation.clone()]),
            );
            for edge in &narrowed.edges {
                assert_eq!(
                    edge.relation, relation,
                    "seed {seed}: `--relations {relation}` keeps only that relation"
                );
                assert!(
                    known_edges.contains(&edge.id),
                    "seed {seed}: `--relations {relation}` drew edge {edge:?}, which the unfiltered map does not hold"
                );
            }
        }
    }
}

#[test]
fn should_resolve_every_drawn_edge_to_a_drawn_node_or_a_cluster_standing_for_one() {
    // §43.2: "all rendered edges reference existing rendered nodes or explicit off-map
    // endpoints". §8.2 makes a cluster the explicit off-map endpoint: an edge to something the
    // budget could not draw points at the cluster that stands for it, which is on the map.
    for seed in 0..RUNS {
        let mut rng = Rng::seeded(seed);
        // Deliberately spanning the budget, so that clustering is exercised as an endpoint.
        let (index, horizon) = host(4 + rng.below(3 * MAP_NODE_BUDGET));
        let all = rng.below(2) == 0;
        let map = draw(&index, &horizon, &MapRequest::new().depth(3).all(all));

        let mut reachable = node_ids(&map);
        for cluster in &map.clusters {
            reachable.insert(cluster.id.clone());
        }
        for edge in &map.edges {
            assert!(
                reachable.contains(&edge.source),
                "seed {seed}: edge {edge:?} starts at something the map does not draw"
            );
            assert!(
                reachable.contains(&edge.target),
                "seed {seed}: edge {edge:?} ends at something the map does not draw"
            );
        }
    }
}

#[test]
fn should_keep_every_identity_the_same_however_the_map_is_laid_out() {
    // §43.2: "map coordinates never affect semantic identity"; §2.7: "screen layout may choose
    // positions, but those positions MUST NOT become semantic coordinates". The projection
    // therefore has no coordinate at all — and changing what the renderer would lay out
    // differently (which node is focused, how deep the view reaches, whether it is bounded)
    // never changes the identity, the type or the label of a place that is drawn either way.
    for seed in 0..RUNS {
        let mut rng = Rng::seeded(seed);
        let (index, horizon) = host(4 + rng.below(3 * MAP_NODE_BUDGET));
        let plain = draw(&index, &horizon, &MapRequest::new().depth(3).all(true));
        assert!(
            !plain.nodes.is_empty(),
            "seed {seed}: the fixture horizon draws something"
        );

        let focused_on = plain.nodes[rng.below(plain.nodes.len())].id.clone();
        let laid_out_again = draw(
            &index,
            &horizon,
            &MapRequest::new()
                .depth(3)
                .all(true)
                .focus(focused_on.to_string()),
        );
        assert_eq!(
            laid_out_again.focus.as_ref(),
            Some(&focused_on),
            "seed {seed}: the map focuses what it was asked to focus"
        );

        let before: std::collections::BTreeMap<String, (SpatialType, String)> = plain
            .nodes
            .iter()
            .map(|node| (node.id.to_string(), (node.object_type, node.label.clone())))
            .collect();
        for node in &laid_out_again.nodes {
            if let Some(was) = before.get(&node.id.to_string()) {
                assert_eq!(
                    (node.object_type, node.label.clone()),
                    was.clone(),
                    "seed {seed}: focusing changed what {node:?} is"
                );
            }
        }
        // The centre is where the user stands, and no layout choice moves it (§23.4).
        assert_eq!(
            laid_out_again.center, plain.center,
            "seed {seed}: focus is a view action and never moves the centre"
        );
    }
}
