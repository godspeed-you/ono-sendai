//! The bulk strategies and the canary gate (spec v0.6 §28.4, §28.6, §55.10 case 43).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use std::cell::RefCell;
use std::sync::Arc;

use common::{
    PlanSpec, Script, empty_registry, instant, no_drift, observing, stored, store,
};
use ono_change_core::{ActionStatus, PlanState, Strategy, Verdict, VerificationStatus, Wave};
use ono_change_executor::execute::{ApplyRequest, FailurePoint, apply};
use ono_change_executor::strategy::{TargetResult, run_waves};

fn targets(count: usize) -> Vec<Arc<str>> {
    (1..=count)
        .map(|index| Arc::from(format!("svc-{index}").as_str()))
        .collect()
}

fn succeeding(_wave: &Wave, slice: &[Arc<str>]) -> Vec<TargetResult> {
    slice
        .iter()
        .map(|target| TargetResult::new(Arc::clone(target), ActionStatus::Succeeded, None))
        .collect()
}

fn passing(_wave: &Wave, _slice: &[Arc<str>]) -> Verdict {
    Verdict::Verified
}

// ---- §28.4: the four strategies ---------------------------------------------------------------

#[test]
fn should_submit_one_target_at_a_time_when_the_strategy_is_sequential() {
    let seen: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    let submit = |wave: &Wave, slice: &[Arc<str>]| {
        seen.borrow_mut().push(slice.len());
        succeeding(wave, slice)
    };

    let run = run_waves(Strategy::Sequential, &targets(5), &submit, &passing);

    assert_eq!(seen.into_inner(), vec![1, 1, 1, 1, 1]);
    assert_eq!(run.waves_run(), 5);
    assert_eq!(run.touched().len(), 5);
    assert!(run.completed());
}

#[test]
fn should_submit_a_whole_batch_together_when_the_strategy_is_batch() {
    let seen: RefCell<Vec<usize>> = RefCell::new(Vec::new());
    let submit = |wave: &Wave, slice: &[Arc<str>]| {
        seen.borrow_mut().push(slice.len());
        succeeding(wave, slice)
    };

    let run = run_waves(
        Strategy::batch(3).expect("a valid batch"),
        &targets(7),
        &submit,
        &passing,
    );

    assert_eq!(seen.into_inner(), vec![3, 3, 1]);
    assert_eq!(run.touched().len(), 7);
}

#[test]
fn should_never_submit_more_than_the_declared_width_at_once() {
    let submit = |wave: &Wave, slice: &[Arc<str>]| succeeding(wave, slice);

    let run = run_waves(
        Strategy::parallel(3).expect("a valid width"),
        &targets(10),
        &submit,
        &passing,
    );

    assert_eq!(
        run.widest_wave(),
        3,
        "§28.4: unlimited parallel mutation is not a strategy this shell offers"
    );
    assert_eq!(run.touched().len(), 10);
}

#[test]
fn should_reach_every_target_exactly_once_whatever_the_strategy() {
    for strategy in [
        Strategy::Sequential,
        Strategy::batch(4).expect("valid"),
        Strategy::canary(2, 5).expect("valid"),
        Strategy::parallel(3).expect("valid"),
    ] {
        let submit = |wave: &Wave, slice: &[Arc<str>]| succeeding(wave, slice);
        let run = run_waves(strategy, &targets(11), &submit, &passing);
        let mut touched: Vec<String> = run
            .touched()
            .iter()
            .map(|target| target.as_ref().to_owned())
            .collect();
        touched.sort();
        touched.dedup();
        assert_eq!(
            touched.len(),
            11,
            "{strategy} must schedule every target exactly once (§28.2)"
        );
    }
}

#[test]
fn should_do_nothing_at_all_for_a_plan_with_no_targets() {
    let submit = |wave: &Wave, slice: &[Arc<str>]| succeeding(wave, slice);

    let run = run_waves(Strategy::Sequential, &[], &submit, &passing);

    assert_eq!(run.waves_run(), 0);
    assert!(run.touched().is_empty());
    assert!(run.completed());
}

// ---- §28.6: the canary gate --------------------------------------------------------------------

#[test]
fn should_ask_the_gate_only_after_the_canary_wave() {
    let asked: RefCell<usize> = RefCell::new(0);
    let submit = |wave: &Wave, slice: &[Arc<str>]| succeeding(wave, slice);
    let gate = |_wave: &Wave, _slice: &[Arc<str>]| {
        *asked.borrow_mut() += 1;
        Verdict::Verified
    };

    let run = run_waves(
        Strategy::canary(1, 3).expect("a valid canary"),
        &targets(7),
        &submit,
        &gate,
    );

    assert_eq!(
        asked.into_inner(),
        1,
        "§28.6: only the canary batch is gated, and the rest are not re-gated"
    );
    assert_eq!(run.waves_run(), 3);
    assert_eq!(run.gate_verdict(), Some(Verdict::Verified));
}

#[test]
fn should_stop_after_the_canary_when_its_required_verification_fails() {
    let submit = |wave: &Wave, slice: &[Arc<str>]| succeeding(wave, slice);
    let gate = |_wave: &Wave, _slice: &[Arc<str>]| Verdict::Failed;

    let run = run_waves(
        Strategy::canary(2, 4).expect("a valid canary"),
        &targets(10),
        &submit,
        &gate,
    );

    assert_eq!(
        run.waves_run(),
        1,
        "§28.6: required verification MUST pass before remaining batches continue"
    );
    assert_eq!(run.touched().len(), 2);
    assert_eq!(
        run.untouched().len(),
        8,
        "§55.10 case 43: the outcome says exactly which targets were touched"
    );
    assert_eq!(run.stopped_at(), Some(0));
}

#[test]
fn should_stop_after_the_canary_when_its_verification_could_not_be_answered() {
    let submit = |wave: &Wave, slice: &[Arc<str>]| succeeding(wave, slice);
    let gate = |_wave: &Wave, _slice: &[Arc<str>]| Verdict::Degraded;

    let run = run_waves(
        Strategy::canary(1, 4).expect("a valid canary"),
        &targets(9),
        &submit,
        &gate,
    );

    assert_eq!(
        run.touched().len(),
        1,
        "§23.5 and §28.6: a verdict that is not VERIFIED does not let the rest continue"
    );
    assert_eq!(run.untouched().len(), 8);
}

#[test]
fn should_stop_a_batch_run_when_one_of_its_targets_fails() {
    let submit = |_wave: &Wave, slice: &[Arc<str>]| {
        slice
            .iter()
            .map(|target| {
                let status = if target.as_ref() == "svc-4" {
                    ActionStatus::Failed
                } else {
                    ActionStatus::Succeeded
                };
                TargetResult::new(Arc::clone(target), status, None)
            })
            .collect()
    };

    let run = run_waves(
        Strategy::batch(3).expect("valid"),
        &targets(9),
        &submit,
        &passing,
    );

    assert_eq!(run.stopped_at(), Some(1));
    assert!(!run.completed());
    assert_eq!(
        run.untouched().len(),
        3,
        "the third batch never ran, and the outcome names its targets"
    );
}

#[test]
fn should_stop_a_run_when_a_targets_outcome_could_not_be_established() {
    let submit = |_wave: &Wave, slice: &[Arc<str>]| {
        slice
            .iter()
            .map(|target| {
                let status = if target.as_ref() == "svc-1" {
                    ActionStatus::Unknown
                } else {
                    ActionStatus::Succeeded
                };
                TargetResult::new(Arc::clone(target), status, None)
            })
            .collect()
    };

    let run = run_waves(Strategy::Sequential, &targets(4), &submit, &passing);

    assert_eq!(
        run.stopped_at(),
        Some(0),
        "Appendix F.2: the next batch would run against a system nobody has an account of"
    );
    assert_eq!(run.untouched().len(), 3);
}

// ---- §55.10 case 43, through the whole executor -------------------------------------------------

#[test]
fn should_stop_the_canary_plan_after_the_first_batch_verification_failure() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(9)
        .with_strategy(Strategy::canary(1, 4).expect("a valid canary"))
        .only_required("svc-1");
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Failed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        script.calls(),
        vec!["svc-1".to_owned()],
        "§55.10 case 43: the canary stops after the first batch verification failure"
    );
    assert_eq!(
        outcome.touched_targets(),
        &[Arc::<str>::from("svc-1")],
        "the outcome says exactly which targets were touched"
    );
    assert_eq!(
        outcome.untouched_targets().len(),
        8,
        "and exactly which were not"
    );
    assert_eq!(outcome.state(), PlanState::Failed);
    assert_eq!(
        outcome.failure_point(),
        Some(FailurePoint::RequiredVerification)
    );
    assert!(outcome.has_mutated());
}

#[test]
fn should_continue_past_a_canary_whose_verification_passed() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(5)
        .with_strategy(Strategy::canary(1, 2).expect("a valid canary"))
        .only_required("svc-1");
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(
        script.calls().len(),
        5,
        "§28.6: a canary that verified lets the remaining batches continue"
    );
    assert_eq!(outcome.state(), PlanState::Verified);
    assert!(outcome.untouched_targets().is_empty());
}

#[test]
fn should_run_a_bounded_parallel_plan_in_waves_of_the_declared_width() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(7).with_strategy(Strategy::parallel(2).expect("valid"));
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy();
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(script.calls().len(), 7);
    assert_eq!(outcome.state(), PlanState::Verified);
}

#[test]
fn should_name_the_untouched_targets_when_a_batch_plan_fails_midway() {
    let now = instant(1_000);
    let (_directory, store) = store();
    let spec = PlanSpec::over(6).with_strategy(Strategy::batch(2).expect("valid"));
    let plan = stored(&spec, &store, now);
    let providers = empty_registry();
    let protection = Vec::new();
    let drift = no_drift();
    let script = Script::healthy().failing("svc-3");
    let execute = script.execute();
    let observe = observing(VerificationStatus::Passed);
    let mut request = ApplyRequest::new(
        &plan,
        &store,
        "session-a",
        now,
        &protection,
        &providers,
        &drift,
        &execute,
        &observe,
    );

    let outcome = apply(&mut request);

    assert_eq!(outcome.state(), PlanState::ApplyFailed);
    assert_eq!(
        outcome.touched_targets().len(),
        3,
        "svc-1 and svc-2 ran, and svc-3 failed"
    );
    assert_eq!(
        outcome.untouched_targets().len(),
        3,
        "§29.1's spirit locally: the outcome never implies the whole plan ran"
    );
}
