//! Execution strategies for plans with more than one target (spec v0.6 §28.4).
//!
//! §28.4 fixes four strategies and rules out a fifth: *"Unlimited parallel mutation is not a
//! default strategy."* The type therefore has no unbounded variant at all — [`Strategy::Parallel`]
//! carries its width, and a width of zero is refused at construction rather than treated as
//! "as many as possible".
//!
//! §28.5 makes the strategy part of the seal, because it changes operational risk and temporal
//! effects. [`Strategy::digest_text`] is what carries it there.

use crate::vocab::vocabulary;

vocabulary! {
    /// The name of a strategy, for contracts and rendered output.
    StrategyKind {
        Sequential => "sequential", "§28.4: one target at a time. The default (§53's `default_strategy`).";
        Batch => "batch", "§28.4: fixed-size batches, each completing before the next begins.";
        Canary => "canary", "§28.4: a first small batch whose required verification must pass before the rest continue (§28.6).";
        Parallel => "parallel", "§28.4: a bounded number of targets at once. The bound is not optional.";
    }
}

/// How a plan's mutating actions are scheduled across its targets (§28.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// One target at a time (§28.4). The default of §53's `default_strategy`.
    #[default]
    Sequential,
    /// Batches of `size`, each completing before the next begins (§28.4).
    Batch {
        /// How many targets one batch holds. Never zero.
        size: usize,
    },
    /// A canary batch of `canary`, then batches of `batch` (§28.4, §28.6).
    Canary {
        /// How many targets the canary batch holds. Never zero.
        canary: usize,
        /// How many targets each following batch holds. Never zero.
        batch: usize,
    },
    /// At most `width` targets at once (§28.4).
    Parallel {
        /// The concurrency bound. Never zero, and never unbounded.
        width: usize,
    },
}

impl Strategy {
    /// A batch strategy of `size`, or `None` for a size §28.4 does not permit.
    #[must_use]
    pub const fn batch(size: usize) -> Option<Self> {
        if size == 0 {
            return None;
        }
        Some(Self::Batch { size })
    }

    /// A canary strategy, or `None` when either width is zero.
    #[must_use]
    pub const fn canary(canary: usize, batch: usize) -> Option<Self> {
        if canary == 0 || batch == 0 {
            return None;
        }
        Some(Self::Canary { canary, batch })
    }

    /// A parallel strategy of `width`, or `None` for the unbounded case §28.4 rules out.
    #[must_use]
    pub const fn parallel(width: usize) -> Option<Self> {
        if width == 0 {
            return None;
        }
        Some(Self::Parallel { width })
    }

    /// The strategy's name.
    #[must_use]
    pub const fn kind(self) -> StrategyKind {
        match self {
            Strategy::Sequential => StrategyKind::Sequential,
            Strategy::Batch { .. } => StrategyKind::Batch,
            Strategy::Canary { .. } => StrategyKind::Canary,
            Strategy::Parallel { .. } => StrategyKind::Parallel,
        }
    }

    /// How `count` targets are grouped into waves, in order.
    ///
    /// Every strategy answers in waves, including the sequential one, so the executor has a
    /// single shape to schedule and §28.6's "required verification must pass before the
    /// remaining batches continue" is one rule rather than four. A bound of zero, which only a
    /// record can carry, is read as one, so every schedule advances and ends.
    #[must_use]
    pub fn waves(self, count: usize) -> Vec<Wave> {
        let mut waves = Vec::new();
        let mut start = 0usize;
        match self {
            Strategy::Sequential => {
                while start < count {
                    waves.push(Wave {
                        start,
                        len: 1,
                        gated: false,
                        concurrency: 1,
                    });
                    start += 1;
                }
            }
            Strategy::Batch { size } => {
                while start < count {
                    let len = size.max(1).min(count - start);
                    waves.push(Wave {
                        start,
                        len,
                        gated: false,
                        concurrency: 1,
                    });
                    start += len;
                }
            }
            Strategy::Canary { canary, batch } => {
                if count > 0 {
                    let len = canary.max(1).min(count);
                    waves.push(Wave {
                        start: 0,
                        len,
                        gated: true,
                        concurrency: 1,
                    });
                    start = len;
                }
                while start < count {
                    let len = batch.max(1).min(count - start);
                    waves.push(Wave {
                        start,
                        len,
                        gated: false,
                        concurrency: 1,
                    });
                    start += len;
                }
            }
            Strategy::Parallel { width } => {
                let width = width.max(1);
                while start < count {
                    let len = width.min(count - start);
                    waves.push(Wave {
                        start,
                        len,
                        gated: false,
                        concurrency: width,
                    });
                    start += len;
                }
            }
        }
        waves
    }

    /// The canonical text the strategy contributes to a plan digest (§28.5).
    #[must_use]
    pub fn digest_text(self) -> String {
        match self {
            Strategy::Sequential => "sequential".to_owned(),
            Strategy::Batch { size } => format!("batch:{size}"),
            Strategy::Canary { canary, batch } => format!("canary:{canary}:{batch}"),
            Strategy::Parallel { width } => format!("parallel:{width}"),
        }
    }
}

impl std::fmt::Display for Strategy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Strategy::Sequential => formatter.write_str("sequential"),
            Strategy::Batch { size } => write!(formatter, "batch {size}"),
            Strategy::Canary { canary, batch } => {
                write!(formatter, "canary {canary}, then batch {batch}")
            }
            Strategy::Parallel { width } => write!(formatter, "parallel {width}"),
        }
    }
}

/// One wave of targets the executor runs together (§28.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wave {
    /// The index of the first target of the wave.
    pub start: usize,
    /// How many targets the wave holds.
    pub len: usize,
    /// Whether the plan's required verification must pass before the next wave runs (§28.6).
    pub gated: bool,
    /// How many of the wave's targets may run at once.
    pub concurrency: usize,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    /// A bound of zero cannot be built through the constructors, but the variants' fields are
    /// public and a record can carry one. A wave of zero targets never advances, so such a
    /// schedule runs one target at a time — the most conservative — and never hangs.
    #[test]
    fn should_schedule_a_zero_bound_one_target_at_a_time_rather_than_forever() {
        for strategy in [
            Strategy::Batch { size: 0 },
            Strategy::Canary {
                canary: 0,
                batch: 0,
            },
            Strategy::Parallel { width: 0 },
        ] {
            let waves = strategy.waves(3);
            assert_eq!(
                waves.iter().map(|wave| wave.len).sum::<usize>(),
                3,
                "{strategy:?}: every target is scheduled exactly once"
            );
            assert!(
                waves
                    .iter()
                    .all(|wave| wave.len >= 1 && wave.concurrency >= 1)
            );
        }
    }

    #[test]
    fn should_refuse_an_unbounded_parallel_strategy() {
        assert!(
            Strategy::parallel(0).is_none(),
            "§28.4: unlimited parallel mutation is not a strategy this shell offers"
        );
        assert!(
            Strategy::batch(0).is_none(),
            "a batch of nothing is not a batch"
        );
        assert!(Strategy::canary(0, 3).is_none());
        assert!(Strategy::canary(1, 0).is_none());
    }

    #[test]
    fn should_run_one_target_per_wave_when_sequential() {
        let waves = Strategy::Sequential.waves(3);
        assert_eq!(waves.len(), 3);
        assert!(waves.iter().all(|wave| wave.len == 1 && !wave.gated));
    }

    #[test]
    fn should_gate_only_the_first_wave_of_a_canary() {
        let strategy = Strategy::canary(1, 3).expect("a valid canary");
        let waves = strategy.waves(7);
        assert_eq!(waves.len(), 3, "1 canary, then 3 and 3");
        assert!(
            waves[0].gated,
            "§28.6: required verification MUST pass before the remaining batches continue"
        );
        assert!(
            waves[1..].iter().all(|wave| !wave.gated),
            "only the canary gate is a canary gate"
        );
        assert_eq!((waves[0].len, waves[1].len, waves[2].len), (1, 3, 3));
    }

    #[test]
    fn should_cover_every_target_exactly_once_whatever_the_strategy() {
        for strategy in [
            Strategy::Sequential,
            Strategy::batch(4).expect("valid"),
            Strategy::canary(2, 5).expect("valid"),
            Strategy::parallel(3).expect("valid"),
        ] {
            for count in [0usize, 1, 7, 18, 40] {
                let waves = strategy.waves(count);
                let covered: usize = waves.iter().map(|wave| wave.len).sum();
                assert_eq!(
                    covered, count,
                    "{strategy} must schedule every one of {count} targets exactly once"
                );
                let mut cursor = 0;
                for wave in &waves {
                    assert_eq!(wave.start, cursor, "waves must be contiguous and in order");
                    cursor += wave.len;
                }
            }
        }
    }

    #[test]
    fn should_bound_concurrency_to_the_declared_width() {
        let waves = Strategy::parallel(3).expect("valid").waves(10);
        assert!(
            waves
                .iter()
                .all(|wave| wave.concurrency == 3 && wave.len <= 3),
            "§28.4: the bound is the whole point of the strategy"
        );
    }

    #[test]
    fn should_change_the_digest_when_the_strategy_changes() {
        assert_ne!(
            Strategy::Sequential.digest_text(),
            Strategy::batch(1).expect("valid").digest_text(),
            "§28.5: changing strategy creates a new plan revision"
        );
        assert_ne!(
            Strategy::batch(3).expect("valid").digest_text(),
            Strategy::batch(4).expect("valid").digest_text(),
            "a wider batch is a different operational risk (§28.5)"
        );
    }
}
