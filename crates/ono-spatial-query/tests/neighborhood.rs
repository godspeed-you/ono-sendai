//! Neighborhood ranking as a caller sees it — spec v0.4 §3.6, §6.2, §26.4, §32.2, §35.2, §42.4.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use common::{NOW, bridge, index, process, socket_with, with};
use jiff::Span;
use ono_spatial_core::{Completeness, PermissionState, SpatialId, SpatialType};
use ono_spatial_index::{Pin, PinRegistry, SpatialIndex};
use ono_spatial_query::{NeighborhoodRequest, neighborhood_of};
use ono_value::Value;

/// One process holding `sockets` listening sockets, and the index that holds them all.
fn process_with_sockets(sockets: usize) -> (SpatialIndex, SpatialId) {
    let mut records = vec![process(1842, "nginx", "running")];
    for step in 0..sockets {
        let inode = 5000 + i64::try_from(step).expect("a small fixture");
        records.push(with(
            socket_with(inode, Some("listen"), None),
            "process",
            Value::Int(1842),
        ));
    }
    let mut index = index();
    let mut bridge = bridge();
    let absorbed = bridge.absorb(&mut index, &records, NOW);
    assert!(absorbed.refused().is_empty(), "{:?}", absorbed.refused());
    let center = index
        .by_alias("nginx")
        .first()
        .expect("the process is indexed")
        .object()
        .spatial_id()
        .clone();
    (index, center)
}

/// One process with `children` child processes, joined by `process.parent_of` — the relation
/// whose two ends have different words: `children`/`child` from the parent, `parent` from a
/// child.
fn process_with_children(children: usize) -> (SpatialIndex, SpatialId) {
    let mut index = index();
    let mut bridge = bridge();
    let mut records = vec![process(1842, "nginx", "running")];
    for step in 0..children {
        let pid = 1900 + i64::try_from(step).expect("a small fixture");
        records.push(process(pid, &format!("worker-{step}"), "running"));
    }
    let absorbed = bridge.absorb(&mut index, &records, NOW);
    assert!(absorbed.refused().is_empty(), "{:?}", absorbed.refused());
    let center = index
        .by_alias("nginx")
        .first()
        .expect("the process is indexed")
        .object()
        .spatial_id()
        .clone();
    for step in 0..children {
        let child = index
            .by_alias(&format!("worker-{step}"))
            .first()
            .expect("the child is indexed")
            .object()
            .spatial_id()
            .clone();
        index.record_edge(ono_spatial_core::RelationshipEdge::new(
            center.clone(),
            child,
            ono_spatial_core::RelationType::new("process.parent_of").expect("a declared relation"),
            ono_spatial_core::Confidence::Exact,
            ono_value::Provenance::local(
                "linux.process-tree",
                ono_value::SchemaId::new("ono.process", 1),
            ),
            NOW,
        ));
    }
    (index, center)
}

fn group<'a>(
    neighborhood: &'a ono_spatial_core::Neighborhood,
    label: &str,
) -> &'a ono_spatial_core::NeighborhoodGroup {
    neighborhood
        .groups()
        .iter()
        .find(|group| group.label() == label)
        .unwrap_or_else(|| {
            panic!(
                "no `{label}` group among {:?}",
                neighborhood
                    .groups()
                    .iter()
                    .map(ono_spatial_core::NeighborhoodGroup::label)
                    .collect::<Vec<_>>()
            )
        })
}

#[test]
fn should_bound_the_projection_and_count_what_it_hid_when_a_place_has_many_neighbors() {
    // §3.6: a neighborhood is "bounded, ranked ... not simply all adjacent nodes", and what the
    // bound left out is counted rather than dropped (`hidden_count`, `completeness`).
    let (index, center) = process_with_sockets(30);
    let bounded = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new(),
        &PinRegistry::new(),
        NOW,
    );
    assert!(
        bounded.hidden_count() > 0,
        "thirty sockets do not fit a bounded view, got hidden_count {}",
        bounded.hidden_count()
    );
    assert_eq!(
        bounded.completeness(),
        Completeness::Partial,
        "a process's `file` exit is expensive and stays unloaded until it is asked for, so the \
         projection is not complete either (§3.6, §32.2)"
    );
    assert_eq!(group(&bounded, "sockets").total(), Some(30));
    assert!(group(&bounded, "sockets").members().len() < 30);
    assert_eq!(bounded.center(), &center);
    assert_eq!(bounded.generated_at(), NOW);
}

#[test]
fn should_show_every_neighbor_and_hide_nothing_when_all_is_asked_for() {
    // §6.2: "`--all` requests the complete currently known one-hop neighborhood."
    let (index, center) = process_with_sockets(30);
    let complete = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().all(true),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(
        complete.hidden_count(),
        0,
        "`--all` hides no known neighbour"
    );
    assert_eq!(group(&complete, "sockets").members().len(), 30);
}

#[test]
fn should_answer_exactly_the_requested_number_of_neighbors_when_a_limit_is_given() {
    // §6.2's `near --limit <n>`: the user's number is the bound, whatever the view budget is.
    let (index, center) = process_with_sockets(30);
    let limited = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().limit(3),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(group(&limited, "sockets").members().len(), 3);
    assert_eq!(group(&limited, "sockets").total(), Some(30));
    assert_eq!(limited.hidden_count(), 27);
}

#[test]
fn should_keep_one_exit_only_when_a_relation_is_named() {
    // §6.2's `near <relation>`.
    let (index, center) = process_with_sockets(3);
    let one = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().along("socket"),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(
        one.groups()
            .iter()
            .map(ono_spatial_core::NeighborhoodGroup::label)
            .collect::<Vec<_>>(),
        vec!["sockets"]
    );
}

#[test]
fn should_keep_only_the_requested_type_among_the_members_when_a_type_is_given() {
    // §6.2's `near --type <type>`: a user filter narrows the members before the bound applies.
    let (index, center) = process_with_sockets(3);
    let typed = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().of_type(SpatialType::Process),
        &PinRegistry::new(),
        NOW,
    );
    assert!(
        typed
            .groups()
            .iter()
            .flat_map(ono_spatial_core::NeighborhoodGroup::members)
            .all(|id| index
                .get(id)
                .is_some_and(|entry| entry.object().object_type() == SpatialType::Process)),
        "a `--type process` view holds no sockets"
    );
}

#[test]
fn should_report_a_refused_exit_as_its_state_rather_than_as_a_count() {
    // §42.4 with §35.2: "Denied information must produce `permission_denied` or `unknown`, never
    // false empty collections." A bound must not turn a refusal into a number, and a filter must
    // not make it disappear.
    let (mut index, center) = process_with_sockets(2);
    assert!(index.record_withheld(
        &center,
        "files",
        PermissionState::PermissionDenied,
        "permission denied for 14 process FDs",
    ));
    let view = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().limit(1),
        &PinRegistry::new(),
        NOW,
    );
    let files = group(&view, "files");
    assert_eq!(files.state(), PermissionState::PermissionDenied);
    assert_eq!(files.total(), None, "a refusal has no count (§2.17)");
    assert_eq!(files.detail(), Some("permission denied for 14 process FDs"));
    assert_eq!(
        view.completeness(),
        Completeness::Partial,
        "a source that could not be read makes the projection partial (§3.6)"
    );
}

#[test]
fn should_rank_a_pinned_neighbor_first_and_name_it_as_a_landmark() {
    // §26.4: "User pins are landmarks", and a pin outranks every heuristic. §3.6 puts the
    // landmarks on the neighborhood itself.
    let (index, center) = process_with_sockets(5);
    let last = group(
        &neighborhood_of(
            &index,
            &center,
            &NeighborhoodRequest::new().all(true),
            &PinRegistry::new(),
            NOW,
        ),
        "sockets",
    )
    .members()
    .last()
    .expect("five sockets")
    .clone();

    let mut pins = PinRegistry::new();
    pins.insert(Pin::new(
        "edge",
        last.clone(),
        ":80",
        SpatialType::Listener,
        "host:testbox",
        NOW,
    ));
    let pinned = neighborhood_of(&index, &center, &NeighborhoodRequest::new(), &pins, NOW);
    assert_eq!(
        group(&pinned, "sockets").members().first(),
        Some(&last),
        "the pinned socket is ranked first"
    );
    assert!(
        pinned
            .landmarks()
            .iter()
            .any(|landmark| landmark.subject() == &last
                && landmark.reason() == ono_spatial_core::LandmarkReason::UserPinned),
        "the pin is a landmark of the projection, got {:?}",
        pinned.landmarks()
    );
}

#[test]
fn should_answer_the_same_order_twice_when_nothing_changed() {
    // §29.3: a script must see a deterministic answer, or `find place x | take 1 | enter` is a
    // coin flip.
    let (index, center) = process_with_sockets(12);
    let once = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new(),
        &PinRegistry::new(),
        NOW,
    );
    let twice = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new(),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(once, twice);
}

#[test]
fn should_drop_a_neighbor_older_than_the_change_window_when_changes_are_asked_for() {
    // §6.2's `near --changed [duration]` with §24.3: the window is a filter over what was
    // observed recently, not a second source of truth.
    let (index, center) = process_with_sockets(4);
    let later = NOW
        .checked_add(Span::new().hours(1))
        .expect("an hour after the fixture");
    let changed = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().changed_within(Span::new().minutes(5)),
        &PinRegistry::new(),
        later,
    );
    assert_eq!(
        group(&changed, "sockets").members().len(),
        0,
        "nothing was observed inside the window"
    );
    assert_eq!(group(&changed, "sockets").total(), Some(0));
}

#[test]
fn should_keep_the_exit_at_this_end_of_a_relation_rather_than_the_one_at_the_other() {
    // §6.2's `near <relation>` names an exit of *this* place. `process.parent_of` is `children`
    // from the parent and `parent` from the child, and matching either end's word from either
    // end answered `near parent` at a process with that process's children — the opposite of
    // what was asked (ADR-0275). `relation::resolve_label` has always resolved by source type;
    // this is the same rule, applied to the filter.
    let (index, center) = process_with_children(3);
    let asked = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().along("parent"),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(
        asked
            .groups()
            .iter()
            .map(ono_spatial_core::NeighborhoodGroup::label)
            .collect::<Vec<_>>(),
        vec!["parent"],
        "the exit named `parent` is the one kept"
    );
    assert!(
        group(&asked, "parent").members().is_empty(),
        "§2.17: the fixture's centre has no parent, and an empty exit is not the other exit"
    );

    let children = neighborhood_of(
        &index,
        &center,
        &NeighborhoodRequest::new().along("children"),
        &PinRegistry::new(),
        NOW,
    );
    assert_eq!(group(&children, "children").members().len(), 3);
}
