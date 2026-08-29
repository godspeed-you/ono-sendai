//! The navigation trail and tombstones — spec v0.4 §2.4, §10.3, §20.1, §20.3, §53, and the unit
//! coverage §43.1 requires ("trail operations", "tombstone resolution").

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use jiff::{Span, Timestamp};
use ono_spatial_core::{
    BackOutcome, Liveness, Movement, NavigationStep, NavigationTrail, RelationType, SpatialId,
    SpatialIdentity, SpatialType, Tombstone, TombstoneRegistry,
};

fn at(seconds: i64) -> Timestamp {
    Timestamp::from_second(1_700_000_000 + seconds).expect("a timestamp")
}

fn object(kind: SpatialType, name: &str) -> SpatialId {
    SpatialIdentity::stable(kind, [("name", name)]).spatial_id()
}

fn trail() -> NavigationTrail {
    NavigationTrail::new(SpatialId::of_space("system"))
}

fn enter(trail: &mut NavigationTrail, to: &SpatialId, seconds: i64) {
    trail.record(NavigationStep::new(
        at(seconds),
        trail.current().clone(),
        to.clone(),
        Movement::Enter,
    ));
}

#[test]
fn should_start_a_session_at_the_root() {
    // §46.1: "the default v0.4 behavior is: start at local SYSTEM root".
    assert_eq!(trail().current(), &SpatialId::of_space("system"));
    assert_eq!(trail().depth(), 0);
}

#[test]
fn should_return_to_the_previous_place_when_going_back_from_a_place_it_entered() {
    // §2.4: "Every movement is reversible. `back` MUST return through the actual navigation
    // trail where the previous location still exists." This is §43.2's `back(enter(x))`.
    let mut trail = trail();
    let compute = SpatialId::of_space("compute");
    enter(&mut trail, &compute, 1);
    assert_eq!(trail.current(), &compute);

    let outcome = trail.back(at(2), |_| true);
    assert!(matches!(outcome, BackOutcome::Returned { .. }));
    assert_eq!(trail.current(), &SpatialId::of_space("system"));
}

#[test]
fn should_refuse_to_go_back_from_a_trail_that_holds_no_earlier_place() {
    // §40's `spatial.history_empty`. The trail of a session starts empty (§46.1).
    assert_eq!(trail().back(at(1), |_| true), BackOutcome::Empty);
}

#[test]
fn should_keep_the_record_of_a_movement_it_went_back_through() {
    // §20.3: "retain the original trail record". `trail` shows where a session has been, not
    // where it currently could return to.
    let mut trail = trail();
    enter(&mut trail, &SpatialId::of_space("compute"), 1);
    trail.back(at(2), |_| true);
    let movements: Vec<Movement> = trail
        .steps()
        .iter()
        .map(ono_spatial_core::NavigationStep::movement)
        .collect();
    assert_eq!(movements, vec![Movement::Enter, Movement::Back]);
}

#[test]
fn should_not_let_a_second_back_undo_the_first() {
    // `back` is an undo, not a toggle: two `back`s reach the place before the previous one.
    let mut trail = trail();
    let compute = SpatialId::of_space("compute");
    let services = SpatialId::of_space("compute.services");
    enter(&mut trail, &compute, 1);
    enter(&mut trail, &services, 2);

    trail.back(at(3), |_| true);
    assert_eq!(trail.current(), &compute);
    trail.back(at(4), |_| true);
    assert_eq!(trail.current(), &SpatialId::of_space("system"));
}

#[test]
fn should_skip_a_destination_that_no_longer_exists_and_say_which_ones_it_skipped() {
    // §20.3, step 2: "otherwise skip to the nearest valid previous place only after informing
    // the user". The skipped places come back in the outcome so the caller can say so.
    let mut trail = trail();
    let gone = object(SpatialType::Process, "1842");
    let services = SpatialId::of_space("compute.services");
    enter(&mut trail, &services, 1);
    enter(&mut trail, &gone, 2);
    enter(&mut trail, &object(SpatialType::Socket, ":443"), 3);

    let outcome = trail.back(at(4), |id| id != &gone);
    match outcome {
        BackOutcome::Skipped { to, skipped, .. } => {
            assert_eq!(to, services);
            assert_eq!(skipped, vec![gone]);
        }
        other => panic!("§20.3: a dead destination is skipped, got {other:?}"),
    }
    assert_eq!(trail.current(), &services);
}

#[test]
fn should_stay_where_it_is_when_nothing_on_the_trail_still_exists() {
    // §20.3 has nowhere to arrive, so the caller answers `spatial.destination_gone` (§40) and
    // the session does not move: §2.2 keeps location explicit, and a silent jump to the root
    // would not be.
    let mut trail = trail();
    let gone = object(SpatialType::Process, "1842");
    enter(&mut trail, &gone, 1);
    let here = trail.current().clone();

    let outcome = trail.back(at(2), |_| false);
    assert!(matches!(outcome, BackOutcome::AllGone { .. }));
    assert_eq!(trail.current(), &here, "a refused `back` does not move");
}

#[test]
fn should_record_the_relation_a_follow_traversed() {
    // §20.1's `relation: RelationType?`, which is what makes a trail explainable (§2.5).
    let mut trail = trail();
    let socket = object(SpatialType::Socket, ":443");
    let relation = RelationType::new("process.owns_socket").expect("a declared relation");
    trail.record(
        NavigationStep::new(
            at(1),
            trail.current().clone(),
            socket.clone(),
            Movement::Follow,
        )
        .along(relation),
    );
    assert_eq!(
        trail.last_step().and_then(NavigationStep::relation),
        Some(&relation)
    );
}

#[test]
fn should_rewrite_a_steps_origin_to_where_the_session_actually_is() {
    // A trail that could record a movement that did not happen would make §2.2 and §2.4 both
    // unenforceable.
    let mut trail = trail();
    let elsewhere = object(SpatialType::Service, "nginx.service");
    trail.record(NavigationStep::new(
        at(1),
        elsewhere.clone(),
        SpatialId::of_space("compute"),
        Movement::Enter,
    ));
    assert_eq!(
        trail.last_step().map(NavigationStep::from),
        Some(&SpatialId::of_space("system"))
    );
}

#[test]
fn should_resolve_a_removed_object_to_its_tombstone_while_one_is_held() {
    // §10.3: "Recently removed objects MAY remain as short-lived tombstones in navigation
    // history and live maps", with the replacement candidate §10.2 shows.
    let mut registry = TombstoneRegistry::new(Span::new().minutes(1));
    let old = object(SpatialType::Process, "1842");
    let new = object(SpatialType::Process, "2198");
    registry.record(
        Tombstone::new(old.clone(), SpatialType::Process, "nginx", at(0)).replaced_by(
            new.clone(),
            RelationType::new("service.controls_process").expect("a declared relation"),
        ),
    );

    let liveness = registry.resolve(&old, false, at(12));
    let tombstone = liveness
        .tombstone()
        .expect("§10.3: a tombstone is resolved");
    assert_eq!(tombstone.replacement(), Some(&new));
    assert!(
        !liveness.accepts_actions(),
        "§10.3: a tombstone MUST NOT accept actions that require a live object"
    );
    assert!(
        liveness.is_reachable(),
        "§20.3: navigation may still arrive and say what happened"
    );
}

#[test]
fn should_prefer_what_the_providers_see_over_what_the_registry_remembers() {
    // §33.2: "The index is a cache. Providers remain authoritative." A pid that came back is a
    // live object, whatever the registry holds about the one that had the number before.
    let mut registry = TombstoneRegistry::new(Span::new().minutes(1));
    let id = object(SpatialType::Process, "1842");
    registry.record(Tombstone::new(
        id.clone(),
        SpatialType::Process,
        "nginx",
        at(0),
    ));
    assert_eq!(registry.resolve(&id, true, at(1)), Liveness::Live);
}

#[test]
fn should_forget_a_tombstone_once_it_is_no_longer_short_lived() {
    // §10.3 says "short-lived", and §10.3's Intent says why: a place that came back from the
    // dead an hour later is exactly the disorientation tombstones exist to prevent.
    let mut registry = TombstoneRegistry::new(Span::new().seconds(30));
    let id = object(SpatialType::Process, "1842");
    registry.record(Tombstone::new(
        id.clone(),
        SpatialType::Process,
        "nginx",
        at(0),
    ));
    assert!(registry.get(&id, at(29)).is_some());
    assert_eq!(registry.resolve(&id, false, at(31)), Liveness::Gone);

    registry.prune(at(31));
    assert_eq!(registry.entries(at(31)).count(), 0);
}

#[test]
fn should_remember_that_a_place_went_away_even_after_its_tombstone_has_expired() {
    // §10.3 keeps tombstones short-lived, and §33.2 makes the providers authoritative. Between
    // those two rules sits the case a caller has to be able to ask about: the object went away,
    // the tombstone is past its lifetime, and nothing has answered for the place since. That is
    // `Gone` — not a live place, and not one that was never seen.
    let mut registry = TombstoneRegistry::new(Span::new().seconds(30));
    let id = object(SpatialType::Process, "1842");
    registry.record(Tombstone::new(
        id.clone(),
        SpatialType::Process,
        "nginx",
        at(0),
    ));

    assert!(registry.recorded(&id));
    assert_eq!(
        registry.resolve(&id, !registry.recorded(&id), at(120)),
        Liveness::Gone,
        "§10.3: a place whose tombstone has expired is gone, not alive"
    );
}

#[test]
fn should_stop_calling_a_place_gone_once_a_provider_answers_for_it_again() {
    // §33.2: "The index is a cache. Providers remain authoritative." A place the registry
    // recorded as gone and a provider then answered for is a live place, and the record of its
    // absence has to go with it — otherwise the next question about it is answered from memory.
    let mut registry = TombstoneRegistry::new(Span::new().minutes(1));
    let id = object(SpatialType::Process, "1842");
    registry.record(Tombstone::new(
        id.clone(),
        SpatialType::Process,
        "nginx",
        at(0),
    ));

    assert!(registry.forget(&id));
    assert!(!registry.recorded(&id));
    assert_eq!(
        registry.resolve(&id, !registry.recorded(&id), at(10)),
        Liveness::Live
    );
}

#[test]
fn should_keep_the_source_that_reached_a_place_so_a_candidate_can_be_asked_for_later() {
    // §10.3's `replacement:` cannot be answered when the object ends — the source of the relation
    // that reached it has not been observed since. What is known then is *which source to ask*,
    // and a tombstone that forgot it could never answer at all (ADR-0273).
    let dead = object(SpatialType::Process, "gone");
    let unit = object(SpatialType::Service, "fixture-web.service");
    let via = RelationType::new("service.controls_process").expect("a declared relation");
    let tombstone = Tombstone::new(dead.clone(), SpatialType::Process, "web", at(0))
        .reached_from([(unit.clone(), via)]);

    assert_eq!(tombstone.reached_by(), &[(unit, via)]);
    assert_eq!(
        tombstone.replacement(),
        None,
        "§2.17: nothing has been observed yet, so there is no candidate to name"
    );
}

#[test]
fn should_name_the_replacement_once_one_has_been_identified() {
    let dead = object(SpatialType::Process, "gone");
    let alive = object(SpatialType::Process, "new");
    let via = RelationType::new("service.controls_process").expect("a declared relation");
    let mut registry = TombstoneRegistry::new(Span::new().minutes(1));
    registry.record(Tombstone::new(
        dead.clone(),
        SpatialType::Process,
        "web",
        at(0),
    ));

    assert!(registry.fill_replacement(&dead, alive.clone(), via));
    let tombstone = registry.get(&dead, at(1)).expect("the tombstone is held");
    assert_eq!(tombstone.replacement(), Some(&alive));
    assert_eq!(tombstone.replacement_via(), Some(&via));
}

#[test]
fn should_keep_the_first_candidate_rather_than_revising_it() {
    // §53: the replacement is a candidate for continuity, never a claim that the two objects are
    // one. A candidate that changed under the reader would be worse than none at all.
    let dead = object(SpatialType::Process, "gone");
    let first = object(SpatialType::Process, "first");
    let second = object(SpatialType::Process, "second");
    let via = RelationType::new("service.controls_process").expect("a declared relation");
    let mut registry = TombstoneRegistry::new(Span::new().minutes(1));
    registry.record(Tombstone::new(
        dead.clone(),
        SpatialType::Process,
        "web",
        at(0),
    ));
    assert!(registry.fill_replacement(&dead, first.clone(), via));

    assert!(!registry.fill_replacement(&dead, second, via));
    assert_eq!(
        registry
            .get(&dead, at(1))
            .expect("the tombstone is held")
            .replacement(),
        Some(&first)
    );
}
