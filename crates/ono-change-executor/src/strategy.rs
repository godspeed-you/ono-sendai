//! Running §28.4's strategies and enforcing §28.6's canary gate.
//!
//! `Strategy::waves` already partitions the frozen target set (§28.2), and this module is what
//! runs the partition. Two rules of §28 live here rather than in the vocabulary:
//!
//! - **§28.6.** For a canary strategy the plan's *required* verification must pass before the
//!   remaining batches continue. [`run_waves`] therefore asks the gate after a gated wave and
//!   stops the whole run on anything other than [`Verdict::Verified`], leaving the untouched
//!   targets nameable — §55.10 case 43 is about what the operator is told afterwards as much as
//!   about what ran.
//! - **§28.4's bound.** "Unlimited parallel mutation is not a default strategy", so a wave is
//!   submitted whole and its results are collected whole, and the wave never holds more than the
//!   strategy's width. Concurrency is modelled rather than performed: the crate has no runtime,
//!   and a test can assert the bound without one.
//!
//! A failing or unestablished target result stops the run for the same reason Appendix F stops the
//! dependency chain — the next batch would be running against a system nobody has an account of.

use std::sync::Arc;

use ono_change_core::{ActionStatus, Strategy, Verdict, Wave};
use ono_value::ErrorValue;

/// What running one target's mutating actions established (§4.7).
#[derive(Debug, Clone, PartialEq)]
pub struct TargetResult {
    target: Arc<str>,
    status: ActionStatus,
    error: Option<ErrorValue>,
}

impl TargetResult {
    /// Records that `target` settled at `status`.
    #[must_use]
    pub const fn new(target: Arc<str>, status: ActionStatus, error: Option<ErrorValue>) -> Self {
        Self {
            target,
            status,
            error,
        }
    }

    /// The target this result is about.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// What is known about it.
    #[must_use]
    pub const fn status(&self) -> ActionStatus {
        self.status
    }

    /// The structured refusal, where there is one.
    #[must_use]
    pub const fn error(&self) -> Option<&ErrorValue> {
        self.error.as_ref()
    }

    /// Whether this result stops the run (§28.6, Appendix F).
    #[must_use]
    pub const fn stops_the_run(&self) -> bool {
        matches!(self.status, ActionStatus::Failed | ActionStatus::Unknown)
    }
}

/// What one strategy run did, and to which targets (§28.4, §55.10 case 43).
#[derive(Debug, Clone, PartialEq)]
pub struct StrategyRun {
    results: Vec<TargetResult>,
    touched: Vec<Arc<str>>,
    untouched: Vec<Arc<str>>,
    waves_run: usize,
    stopped_at: Option<usize>,
    gate_verdict: Option<Verdict>,
    widest_wave: usize,
}

impl StrategyRun {
    /// The per-target results, in the order the waves produced them.
    #[must_use]
    pub fn results(&self) -> &[TargetResult] {
        &self.results
    }

    /// The targets a mutating action actually ran against.
    #[must_use]
    pub fn touched(&self) -> &[Arc<str>] {
        &self.touched
    }

    /// The targets nothing ran against, which §55.10 case 43 requires to be nameable.
    #[must_use]
    pub fn untouched(&self) -> &[Arc<str>] {
        &self.untouched
    }

    /// How many waves ran.
    #[must_use]
    pub const fn waves_run(&self) -> usize {
        self.waves_run
    }

    /// The index of the wave the run stopped on, where it stopped.
    #[must_use]
    pub const fn stopped_at(&self) -> Option<usize> {
        self.stopped_at
    }

    /// What the canary gate answered, where a gated wave ran (§28.6).
    #[must_use]
    pub const fn gate_verdict(&self) -> Option<Verdict> {
        self.gate_verdict
    }

    /// The largest number of targets submitted together, which §28.4 bounds.
    #[must_use]
    pub const fn widest_wave(&self) -> usize {
        self.widest_wave
    }

    /// Whether the run reached the last wave.
    #[must_use]
    pub const fn completed(&self) -> bool {
        self.stopped_at.is_none()
    }
}

/// Runs `targets` in the waves `strategy` partitions them into, gating where §28.6 says to.
///
/// `submit` receives one wave's targets together, which is the crate's model of concurrency: the
/// wave is submitted together and its results are collected together, so `Parallel { width }`
/// never offers more than `width` at once and a test can prove the bound without a runtime.
///
/// `gate` is asked after a wave `Strategy::waves` marked gated, and only then. Its answer is the
/// plan's required verification (§28.6), and anything other than [`Verdict::Verified`] stops the
/// run with the remaining targets untouched.
#[must_use]
pub fn run_waves(
    strategy: Strategy,
    targets: &[Arc<str>],
    submit: &dyn Fn(&Wave, &[Arc<str>]) -> Vec<TargetResult>,
    gate: &dyn Fn(&Wave, &[Arc<str>]) -> Verdict,
) -> StrategyRun {
    let waves = strategy.waves(targets.len());
    let mut run = StrategyRun {
        results: Vec::new(),
        touched: Vec::new(),
        untouched: Vec::new(),
        waves_run: 0,
        stopped_at: None,
        gate_verdict: None,
        widest_wave: 0,
    };
    for (index, wave) in waves.iter().enumerate() {
        let end = wave.start.saturating_add(wave.len).min(targets.len());
        let slice: Vec<Arc<str>> = targets
            .get(wave.start..end)
            .unwrap_or_default()
            .iter()
            .map(Arc::clone)
            .collect();
        run.widest_wave = run.widest_wave.max(slice.len());
        let results = submit(wave, &slice);
        run.waves_run = run.waves_run.saturating_add(1);
        for result in &results {
            if result.status == ActionStatus::Skipped {
                run.untouched.push(Arc::clone(&result.target));
            } else {
                run.touched.push(Arc::clone(&result.target));
            }
        }
        let stopped = results.iter().any(TargetResult::stops_the_run);
        // A target the submission never reached is untouched, whatever the wave held.
        let reported: Vec<Arc<str>> = results
            .iter()
            .map(|result| Arc::clone(&result.target))
            .collect();
        for target in &slice {
            if !reported.contains(target) {
                run.untouched.push(Arc::clone(target));
            }
        }
        run.results.extend(results);
        if stopped {
            run.stopped_at = Some(index);
            run.untouched.extend(
                targets
                    .get(end..)
                    .unwrap_or_default()
                    .iter()
                    .map(Arc::clone),
            );
            return run;
        }
        if wave.gated {
            let verdict = gate(wave, &slice);
            run.gate_verdict = Some(verdict);
            if verdict != Verdict::Verified {
                run.stopped_at = Some(index);
                run.untouched.extend(
                    targets
                        .get(end..)
                        .unwrap_or_default()
                        .iter()
                        .map(Arc::clone),
                );
                return run;
            }
        }
    }
    run
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_stop_a_run_on_a_result_nobody_could_establish() {
        let unknown = TargetResult::new(Arc::from("svc-1"), ActionStatus::Unknown, None);
        assert!(
            unknown.stops_the_run(),
            "Appendix F.2: the next wave would run against a system nobody has an account of"
        );
        let failed = TargetResult::new(Arc::from("svc-1"), ActionStatus::Failed, None);
        assert!(failed.stops_the_run());
        let done = TargetResult::new(Arc::from("svc-1"), ActionStatus::Succeeded, None);
        assert!(!done.stops_the_run());
    }

    #[test]
    fn should_run_no_waves_at_all_over_no_targets() {
        let submit = |_wave: &Wave, _slice: &[Arc<str>]| Vec::new();
        let gate = |_wave: &Wave, _slice: &[Arc<str>]| Verdict::Verified;
        let run = run_waves(Strategy::Sequential, &[], &submit, &gate);
        assert_eq!(run.waves_run(), 0);
        assert!(run.completed());
        assert_eq!(run.gate_verdict(), None);
    }
}
