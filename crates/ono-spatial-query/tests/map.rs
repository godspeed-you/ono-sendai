//! The `SpatialMap` projection as a caller sees it — spec v0.4 §22, §8, §23.1, §23.6, §34.2,
//! §43.2.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::collections::BTreeSet;

use common::{NOW, bridge, draw, index, process};
use ono_spatial_core::{HierarchyKind, SpatialId, SpatialType};
use ono_spatial_index::SpatialIndex;
use ono_spatial_query::{
    HorizonPlace, MAP_NODE_BUDGET, MapHorizon, MapRequest, SpatialMap, TEXT_MAP_BUDGET,
};

/// The root, its COMPUTE domain, the processes collection and `count` processes inside it — the
/// shape of a horizon the shell hands the projection at the system root.
fn host(count: usize) -> (SpatialIndex, MapHorizon) {
    let records: Vec<ono_value::RecordValue> = (0..count)
        .map(|step| {
            let pid = 1000 + i64::try_from(step).expect("a small fixture");
            process(pid, &format!("worker-{step:03}"), "running")
        })
        .collect();
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
    for entry in index.of_type(SpatialType::Process) {
        horizon.place(HorizonPlace::new(
            entry.object().spatial_id().clone(),
            3,
            Some((processes.clone(), HierarchyKind::Grouping)),
        ));
    }
    (index, horizon)
}

fn node_ids(map: &SpatialMap) -> BTreeSet<String> {
    map.nodes.iter().map(|node| node.id.to_string()).collect()
}

#[test]
fn should_stay_inside_the_text_budget_and_cluster_the_rest_when_the_horizon_is_larger() {
    // §34.2: "unbounded graph rendering is prohibited"; §8.2: what does not fit is clustered
    // rather than truncated arbitrarily; §23.6: hidden counts are disclosed.
    let (index, horizon) = host(120);

    let map = draw(&index, &horizon, &MapRequest::new().depth(3));

    assert!(
        map.nodes.len() <= TEXT_MAP_BUDGET,
        "the default map drew {} nodes",
        map.nodes.len()
    );
    assert!(
        !map.clusters.is_empty(),
        "spec §8.2: the objects that did not fit are clustered"
    );
    let clustered: usize = map.clusters.iter().map(|cluster| cluster.members()).sum();
    assert_eq!(
        clustered + map.nodes.len(),
        124,
        "spec §23.6: every known place is either drawn or stood for by a cluster"
    );
    assert!(map.hidden.count > 0 && map.hidden.clustered == clustered);
}

#[test]
fn should_draw_more_when_the_caller_asks_for_all_and_still_honour_the_node_budget() {
    // §6.9/§53: `--all` is the explicit larger bound the default is not; §47 keeps it inside
    // `spatial.map.node_budget`.
    let (index, horizon) = host(400);

    let default = draw(&index, &horizon, &MapRequest::new().depth(3));
    let all = draw(&index, &horizon, &MapRequest::new().depth(3).all(true));

    assert!(all.nodes.len() > default.nodes.len());
    assert!(all.nodes.len() <= MAP_NODE_BUDGET);
}

#[test]
fn should_reach_only_the_requested_number_of_hops_when_a_depth_is_given() {
    // §6.9's `--depth <n>`: the horizon is the canonical children within n hops, and nothing
    // beyond them.
    let (index, horizon) = host(10);

    let one = draw(&index, &horizon, &MapRequest::new().depth(1));

    assert_eq!(
        one.nodes.len(),
        3,
        "the root and its two domains, and nothing behind them: {:?}",
        one.nodes.iter().map(|node| &node.label).collect::<Vec<_>>()
    );
}

#[test]
fn should_fold_entities_into_their_domain_when_the_zoom_level_is_coarse() {
    // §8.1: zoom "changes the level of conceptual aggregation while preserving drill-down
    // paths". L1 is the domain level, so a process is drawn as the domain it belongs to.
    let (index, horizon) = host(10);

    let domains = draw(&index, &horizon, &MapRequest::new().depth(3).zoom(1));
    let entities = draw(&index, &horizon, &MapRequest::new().depth(3).zoom(3));

    assert_eq!(domains.zoom_level, 1);
    let labels: Vec<String> = domains
        .nodes
        .iter()
        .map(|node| node.label.to_lowercase())
        .collect();
    assert!(labels.contains(&"compute".to_owned()) && labels.contains(&"network".to_owned()));
    assert!(
        domains.nodes.len() < entities.nodes.len(),
        "spec §8: L1 is coarser than L3"
    );
    assert_ne!(node_ids(&domains), node_ids(&entities));
}

#[test]
fn should_keep_the_place_and_yield_exactly_the_members_when_a_cluster_is_expanded() {
    // §8.3: "An interactive cluster MUST be expandable without changing the underlying current
    // place." Expansion is a view action, `enter` is navigation, and this is not `enter`.
    let (index, horizon) = host(120);
    let collapsed = draw(&index, &horizon, &MapRequest::new().depth(3));
    let cluster = collapsed
        .clusters
        .first()
        .expect("a horizon larger than the budget is clustered")
        .clone();

    let expanded = draw(
        &index,
        &horizon,
        &MapRequest::new().depth(3).expand(vec![cluster.id.clone()]),
    );

    let appeared: BTreeSet<String> = node_ids(&expanded)
        .difference(&node_ids(&collapsed))
        .cloned()
        .collect();
    assert_eq!(appeared.len(), cluster.members());
    assert_eq!(expanded.center, collapsed.center);
    assert!(
        !expanded.clusters.iter().any(|kept| kept.id == cluster.id),
        "spec §8.3: an expanded cluster no longer stands in for its members"
    );
}

#[test]
fn should_name_the_same_cluster_in_two_separate_projections() {
    // §8.3's expansion is spelled `map --expand <cluster-id>` in a *later* run, so a cluster id
    // that changed between two projections of the same horizon would be unusable.
    let (index, horizon) = host(120);

    let first = draw(&index, &horizon, &MapRequest::new().depth(3));
    let second = draw(&index, &horizon, &MapRequest::new().depth(3));

    let ids = |map: &SpatialMap| -> Vec<String> {
        map.clusters
            .iter()
            .map(|cluster| cluster.id.clone())
            .collect()
    };
    assert_eq!(ids(&first), ids(&second));
}

#[test]
fn should_resolve_every_edge_to_a_drawn_node_or_the_cluster_standing_for_it() {
    // §43.2: "all rendered edges reference existing rendered nodes or explicit off-map
    // endpoints", and §8.2 makes a cluster the explicit stand-in for what it hides.
    let (index, horizon) = host(120);

    let map = draw(&index, &horizon, &MapRequest::new().depth(3));

    let known: BTreeSet<String> = node_ids(&map)
        .into_iter()
        .chain(map.clusters.iter().map(|cluster| cluster.id.clone()))
        .collect();
    for edge in &map.edges {
        assert!(
            known.contains(&edge.source) && known.contains(&edge.target),
            "{edge:?} reaches something the map does not draw"
        );
    }
    assert!(!map.edges.is_empty());
}

#[test]
fn should_remove_edges_without_inventing_any_when_a_relation_filter_narrows_the_map() {
    // §43.2: "filtering cannot create unknown edges".
    let (index, horizon) = host(20);
    let complete = draw(&index, &horizon, &MapRequest::new().depth(3).all(true));
    let relation = complete.edges.first().expect("edges").relation.clone();

    let filtered = draw(
        &index,
        &horizon,
        &MapRequest::new()
            .depth(3)
            .all(true)
            .relations(vec![relation.clone()]),
    );

    let known: BTreeSet<String> = complete.edges.iter().map(|edge| edge.id.clone()).collect();
    for edge in &filtered.edges {
        assert_eq!(edge.relation, relation);
        assert!(known.contains(&edge.id));
    }
}

#[test]
fn should_leave_the_centre_where_it_is_when_a_node_is_focused() {
    // §23.4/§53: "Does focus move the shell? No. Only explicit navigation changes current place."
    let (index, horizon) = host(10);
    let unfocused = draw(&index, &horizon, &MapRequest::new().depth(3));
    let target = unfocused
        .nodes
        .iter()
        .find(|node| node.id != unfocused.center)
        .expect("a node to focus")
        .id
        .clone();

    let focused = draw(
        &index,
        &horizon,
        &MapRequest::new().depth(3).focus(target.to_string()),
    );

    assert_eq!(focused.center, unfocused.center);
    assert_eq!(focused.focus.as_ref(), Some(&target));
    assert_ne!(focused.center, target);
}

/// A directory and `files` + `directories` entries inside it, as the shell hands the projection
/// the horizon of a directory place: the entries one containment hop from the centre.
fn directory_of(files: usize, directories: usize) -> (SpatialIndex, SpatialId, MapHorizon) {
    let entry = |path: String, kind: &str, inode: usize| {
        common::record(
            "ono.file/1",
            &[
                (
                    "path",
                    ono_value::Value::Path(std::sync::Arc::from(std::path::Path::new(&path))),
                ),
                (
                    "name",
                    ono_value::Value::string(path.rsplit('/').next().unwrap_or(&path)),
                ),
                ("kind", ono_value::Value::string(kind)),
                ("device", ono_value::Value::string("8:2")),
                (
                    "inode",
                    ono_value::Value::Int(i128::try_from(inode).expect("a small fixture")),
                ),
            ],
        )
    };
    let mut records = vec![entry("/srv".to_owned(), "dir", 1)];
    for step in 0..files {
        records.push(entry(format!("/srv/file-{step:03}"), "file", 100 + step));
    }
    for step in 0..directories {
        records.push(entry(format!("/srv/dir-{step:03}"), "dir", 1000 + step));
    }
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(&mut index, &records, NOW);
    let center = index
        .of_type(SpatialType::Directory)
        .into_iter()
        .find(|entry| entry.object().display_name().ends_with("srv"))
        .expect("the directory was absorbed")
        .object()
        .spatial_id()
        .clone();

    let mut horizon = MapHorizon::new();
    horizon.place(HorizonPlace::new(center.clone(), 0, None));
    for kind in [SpatialType::File, SpatialType::Directory] {
        for entry in index.of_type(kind) {
            let id = entry.object().spatial_id().clone();
            if id != center {
                horizon.place(HorizonPlace::new(
                    id,
                    1,
                    Some((center.clone(), HierarchyKind::Containment)),
                ));
            }
        }
    }
    (index, center, horizon)
}

#[test]
fn should_cluster_the_entries_of_a_directory_by_kind_when_they_exceed_the_budget() {
    // §15.4: a huge directory is clustered or summarised, never enumerated; §8.2 allows the
    // directory subtree as a clustering dimension and requires each cluster to report what it
    // hides. Counting "83 more in /srv" says less than the directory can: the entries that did
    // not fit are grouped by what they are — files, directories — under the directory that
    // holds them (ADR-0976).
    let (index, center, horizon) = directory_of(70, 40);

    let map = ono_spatial_query::project_map(
        &index,
        &center,
        &horizon,
        &MapRequest::new(),
        &ono_spatial_index::PinRegistry::new(),
        MAP_NODE_BUDGET,
        NOW,
    );

    assert!(map.nodes.len() <= TEXT_MAP_BUDGET);
    let mut clusters: Vec<(String, &str, usize)> = map
        .clusters
        .iter()
        .map(|cluster| (cluster.label.clone(), cluster.grouping, cluster.members()))
        .collect();
    clusters.sort();
    let labels: Vec<&str> = clusters
        .iter()
        .map(|(label, _, _)| label.as_str())
        .collect();
    assert_eq!(
        labels,
        ["/srv (directories)", "/srv (files)"],
        "one cluster per kind of entry, named after the directory and the kind"
    );
    assert!(
        clusters
            .iter()
            .all(|(_, grouping, _)| *grouping == "directory_subtree"),
        "spec §8.2: the dimension is the directory subtree; got {clusters:?}"
    );
    let clustered: usize = clusters.iter().map(|(_, _, members)| members).sum();
    assert_eq!(
        clustered + map.nodes.len(),
        111,
        "spec §8.2/§23.6: every entry is either drawn or counted by a cluster"
    );
}

/// `per_user` processes for each of the named users, under the processes collection, with the
/// users themselves observed — the horizon of `compute/processes` on a shared host.
fn processes_of(users: &[(i64, &str)], per_user: usize) -> (SpatialIndex, MapHorizon) {
    let user = |uid: i64, name: &str| {
        common::record(
            "ono.user/1",
            &[
                ("uid", ono_value::Value::Int(i128::from(uid))),
                ("name", ono_value::Value::string(name)),
            ],
        )
    };
    let mut records = Vec::new();
    for (uid, name) in users {
        records.push(user(*uid, name));
    }
    let mut pid = 2000;
    for (uid, name) in users {
        for step in 0..per_user {
            pid += 1;
            records.push(common::with(
                process(pid, &format!("{name}-{step:03}"), "running"),
                "user",
                ono_value::Value::Record(std::sync::Arc::new(user(*uid, name))),
            ));
        }
    }
    let mut index = index();
    let mut bridge = bridge();
    bridge.absorb(&mut index, &records, NOW);

    let processes = SpatialId::of_space("compute.processes");
    let mut horizon = MapHorizon::new();
    horizon.place(HorizonPlace::new(processes.clone(), 0, None));
    for entry in index.of_type(SpatialType::Process) {
        horizon.place(HorizonPlace::new(
            entry.object().spatial_id().clone(),
            1,
            Some((processes.clone(), HierarchyKind::Grouping)),
        ));
    }
    (index, horizon)
}

#[test]
fn should_cluster_by_the_user_when_the_map_is_asked_to() {
    // §8.2 lists the user among the allowed clustering dimensions. Asked for it, the processes
    // the budget did not draw are grouped by who runs them — a fact each process record states
    // — and each cluster says how many it stands for (ADR-0981).
    let (index, horizon) = processes_of(&[(0, "root"), (1000, "ada"), (33, "www-data")], 40);
    let center = SpatialId::of_space("compute.processes");
    let project = |request: &MapRequest| {
        ono_spatial_query::project_map(
            &index,
            &center,
            &horizon,
            request,
            &ono_spatial_index::PinRegistry::new(),
            MAP_NODE_BUDGET,
            NOW,
        )
    };

    let by_user = project(&MapRequest::new().cluster_by(ono_spatial_query::ClusterDimension::User));
    let mut clusters: Vec<(String, &str)> = by_user
        .clusters
        .iter()
        .map(|cluster| (cluster.label.clone(), cluster.grouping))
        .collect();
    clusters.sort();
    assert_eq!(
        clusters,
        [
            ("processes (user ada)".to_owned(), "user"),
            ("processes (user root)".to_owned(), "user"),
            ("processes (user www-data)".to_owned(), "user"),
        ],
        "one cluster per user, named after the collection and the user"
    );
    let clustered: usize = by_user
        .clusters
        .iter()
        .map(|cluster| cluster.members())
        .sum();
    assert_eq!(
        clustered + by_user.nodes.len(),
        121,
        "spec §8.2/§23.6: every process is drawn or counted by a cluster"
    );

    let default = project(&MapRequest::new());
    assert!(
        default
            .clusters
            .iter()
            .all(|cluster| cluster.grouping == "canonical_collection"),
        "without the request the dimension stays the canonical collection"
    );
}

#[test]
fn should_keep_the_largest_groups_and_count_the_rest_with_the_collection() {
    // §34.2: a dimension with a group per object would trade one long list for another. The
    // largest groups get a cluster each; what is left is counted with its collection, so
    // nothing is dropped and the list of clusters stays short (ADR-0981).
    let users: Vec<(i64, String)> = (0..20).map(|n| (3000 + n, format!("user{n:02}"))).collect();
    let named: Vec<(i64, &str)> = users
        .iter()
        .map(|(uid, name)| (*uid, name.as_str()))
        .collect();
    let (index, horizon) = processes_of(&named, 6);
    let center = SpatialId::of_space("compute.processes");

    let map = ono_spatial_query::project_map(
        &index,
        &center,
        &horizon,
        &MapRequest::new().cluster_by(ono_spatial_query::ClusterDimension::User),
        &ono_spatial_index::PinRegistry::new(),
        MAP_NODE_BUDGET,
        NOW,
    );

    let along = map
        .clusters
        .iter()
        .filter(|cluster| cluster.grouping == "user")
        .count();
    assert_eq!(along, 8, "the eight largest groups, got {:?}", map.clusters);
    let clustered: usize = map.clusters.iter().map(|cluster| cluster.members()).sum();
    assert_eq!(
        clustered + map.nodes.len(),
        121,
        "every process is drawn or counted"
    );
}
