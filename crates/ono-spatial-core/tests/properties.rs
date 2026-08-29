//! The properties spec v0.4 §43.2 recommends, over generated navigation and generated graphs.
//!
//! Each one is a statement about the model that must hold for every input, not for a chosen
//! example, so each is checked against a reproducible pseudo-random stream (`ono_testkit::Rng`,
//! AGENTS.md §11: deterministic). A failure names the seed that produced it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::Timestamp;
use ono_spatial_core::{
    BackOutcome, BootIdentity, Confidence, Movement, NavigationStep, NavigationTrail,
    ProcessIdentity, RelationType, RelationshipEdge, SpatialId, SpatialIdentity, SpatialType,
    canonical_parent, hierarchy, relations, spaces,
};
use ono_testkit::Rng;
use ono_value::{Provenance, SchemaId};

const RUNS: usize = 512;

fn place(rng: &mut Rng) -> SpatialId {
    let places: Vec<SpatialId> = spaces()
        .iter()
        .map(ono_spatial_core::CanonicalSpace::spatial_id)
        .chain((0..8).map(|index| {
            SpatialIdentity::stable(SpatialType::Service, [("name", format!("unit-{index}"))])
                .spatial_id()
        }))
        .collect();
    places[rng.below(places.len())].clone()
}

fn edge(source: SpatialId, target: SpatialId, relation: &str) -> RelationshipEdge {
    RelationshipEdge::new(
        source,
        target,
        RelationType::new(relation).expect("a declared relation"),
        Confidence::Exact,
        Provenance::local("test", SchemaId::new("ono.process", 1)),
        Timestamp::UNIX_EPOCH,
    )
}

#[test]
fn should_return_to_the_prior_place_after_entering_one_whenever_both_still_exist() {
    // §43.2: "back(enter(x)) returns prior place when both remain valid", and §2.4 makes it a
    // MUST rather than a nicety.
    for seed in 0..RUNS as u64 {
        let mut rng = Rng::seeded(seed);
        let start = place(&mut rng);
        let destination = place(&mut rng);
        if start == destination {
            continue;
        }
        let mut trail = NavigationTrail::new(start.clone());
        trail.record(NavigationStep::new(
            Timestamp::UNIX_EPOCH,
            start.clone(),
            destination.clone(),
            Movement::Enter,
        ));
        assert_eq!(trail.current(), &destination, "seed {seed}");

        let outcome = trail.back(Timestamp::UNIX_EPOCH, |_| true);
        assert!(
            matches!(outcome, BackOutcome::Returned { .. }),
            "seed {seed}: back after enter returns, got {outcome:?}"
        );
        assert_eq!(trail.current(), &start, "seed {seed}");
    }
}

#[test]
fn should_return_through_the_whole_trail_however_deep_it_went() {
    // The same property, iterated: a session that entered n places reaches its start after n
    // `back`s, which is what "every movement is reversible" means for more than one movement.
    for seed in 0..RUNS as u64 {
        let mut rng = Rng::seeded(seed);
        let start = place(&mut rng);
        let mut trail = NavigationTrail::new(start.clone());
        let depth = 1 + rng.below(6);
        for _ in 0..depth {
            let next = place(&mut rng);
            trail.record(NavigationStep::new(
                Timestamp::UNIX_EPOCH,
                trail.current().clone(),
                next,
                Movement::Enter,
            ));
        }
        assert_eq!(trail.depth(), depth, "seed {seed}");
        for _ in 0..depth {
            trail.back(Timestamp::UNIX_EPOCH, |_| true);
        }
        assert_eq!(trail.current(), &start, "seed {seed}");
        assert_eq!(
            trail.steps().len(),
            depth * 2,
            "seed {seed}: §20.3 retains every record, including the backs"
        );
    }
}

#[test]
fn should_never_let_a_graph_edge_change_where_up_arrives() {
    // §43.2: "up never traverses arbitrary graph edges", and §2.6: hierarchy and graph are
    // separate concepts. Whatever relationships an object participates in, only the ordered
    // parent rules of §11.3 can move its canonical parent.
    for seed in 0..RUNS as u64 {
        let mut rng = Rng::seeded(seed);
        let subject_type = SpatialType::ALL[rng.below(SpatialType::ALL.len())];
        let subject = SpatialIdentity::stable(subject_type, [("name", format!("subject-{seed}"))])
            .spatial_id();
        let rules: Vec<&str> = hierarchy::parent_rules(subject_type)
            .iter()
            .map(|rule| rule.relation)
            .collect();
        let baseline = canonical_parent(&subject, subject_type, &[]);

        let mut noise = Vec::new();
        for _ in 0..rng.below(8) {
            let relation = &relations()[rng.below(relations().len())];
            if rules.contains(&relation.id) {
                continue;
            }
            let Some(other) = relation.target_from(subject_type) else {
                continue;
            };
            let neighbour = SpatialIdentity::stable(other, [("name", format!("n{}", noise.len()))])
                .spatial_id();
            noise.push(if relation.source == subject_type {
                edge(subject.clone(), neighbour, relation.id)
            } else {
                edge(neighbour, subject.clone(), relation.id)
            });
        }

        assert_eq!(
            canonical_parent(&subject, subject_type, &noise),
            baseline,
            "seed {seed}: a relationship edge moved `up` for {subject_type}"
        );
    }
}

#[test]
fn should_resolve_the_same_provider_identity_to_the_same_spatial_id_every_time() {
    // §43.2: "same stable provider identity -> same SpatialId", and §42.1 makes it the
    // conformance test every provider must pass.
    for seed in 0..RUNS as u64 {
        let mut rng = Rng::seeded(seed);
        let kind = SpatialType::ALL[rng.below(SpatialType::ALL.len())];
        let key = format!("object-{}", rng.next_u64());
        let once = SpatialIdentity::stable(kind, [("name", key.clone())]).spatial_id();
        let again = SpatialIdentity::stable(kind, [("name", key.clone())]).spatial_id();
        assert_eq!(once, again, "seed {seed}");

        let different = SpatialIdentity::stable(kind, [("name", format!("{key}x"))]).spatial_id();
        assert_ne!(
            once, different,
            "seed {seed}: different objects, different id"
        );
    }
}

#[test]
fn should_give_a_reused_pid_a_different_lifetime_id_for_every_generated_case() {
    // §43.2: "PID reuse -> different lifetime SpatialId". §42.2 makes it a provider conformance
    // requirement, because the alternative is a tombstoned place resolving to a live stranger.
    for seed in 0..RUNS as u64 {
        let mut rng = Rng::seeded(seed);
        let boot = BootIdentity::new("testbox", "boot-a");
        let pid = i64::try_from(rng.below(65_536)).unwrap_or(1);
        let namespace = Some(4_026_531_836_u64);
        let first_start = rng.next_u64();
        let second_start = first_start.wrapping_add(1 + rng.next_u64() % 1_000);

        let first = ProcessIdentity::new(boot.clone(), pid, first_start, namespace);
        let reused = ProcessIdentity::new(boot, pid, second_start, namespace);
        assert_ne!(
            first.spatial_id(),
            reused.spatial_id(),
            "seed {seed}: pid {pid} reused with a different start time"
        );
    }
}

#[test]
fn should_never_produce_an_id_a_hand_written_string_could_collide_with() {
    // The opacity of §3.1 is only worth having if it holds for every id: a rendering that a user
    // could type would be a way around the identity rules.
    for seed in 0..RUNS as u64 {
        let mut rng = Rng::seeded(seed);
        let kind = SpatialType::ALL[rng.below(SpatialType::ALL.len())];
        let id = SpatialIdentity::stable(kind, [("name", rng.next_u64().to_string())]).spatial_id();
        assert_eq!(
            SpatialId::parse(id.as_str()),
            Some(id.clone()),
            "seed {seed}: an id reads back as itself"
        );
        assert!(
            !id.as_str().contains(kind.as_str()),
            "seed {seed}: the id does not spell the object out"
        );
    }
}

#[test]
fn should_keep_an_edge_the_same_assertion_however_often_it_is_inverted() {
    // Inverting is reading, not asserting. An edge inverted twice is the edge, for every
    // declared relation — which is what lets a neighborhood be built from either end.
    for relation in relations() {
        let source = SpatialIdentity::stable(relation.source, [("name", "a")]).spatial_id();
        let target = SpatialIdentity::stable(relation.target, [("name", "b")]).spatial_id();
        let edge = edge(source, target, relation.id);
        let twice = edge.inverted().inverted();
        assert_eq!(twice.source(), edge.source(), "{}", relation.id);
        assert_eq!(twice.target(), edge.target(), "{}", relation.id);
        assert_eq!(twice.direction(), edge.direction(), "{}", relation.id);
        assert_eq!(twice.edge_id(), edge.edge_id(), "{}", relation.id);
    }
}
