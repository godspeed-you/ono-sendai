//! The runner: seeds first, then mutations, each execution isolated from the ones around it.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::mutate::Mutator;
use crate::targets::Target;

/// How much work one run of one target does. Fixed rather than timed, so the same command
/// answers the same thing on a loaded machine as on an idle one.
#[derive(Debug, Clone)]
pub struct Budget {
    /// How many mutated inputs to execute, beyond the seeds.
    pub iterations: usize,
    /// The seed that fixes the whole sequence.
    pub seed: u64,
    /// How long one input may take before the run reports it as a finding of its own.
    pub per_input: Duration,
    /// Where to write each input *before* executing it.
    ///
    /// Off by default, because it costs a file write per execution. It is how an input that
    /// aborts the process rather than unwinding is caught: a stack overflow cannot be caught by
    /// [`catch_unwind`], so the only way to see the input that caused one is to have written it
    /// down first. After an abort, the file holds the culprit.
    pub journal: Option<std::path::PathBuf>,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            iterations: 2_000,
            seed: 0x0035_0006,
            per_input: Duration::from_secs(2),
            journal: None,
        }
    }
}

/// Why one input is a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The target panicked. A decoder that panics on input from the kernel, a remote host or a
    /// plugin is the bug spec §35.6 exists to find.
    Panicked,
    /// The target returned, and took longer than the budget allowed. Not a hang — a hang cannot
    /// be observed from inside the thread that hangs (ADR-0313) — but the shape of one.
    TooSlow,
}

/// One input that must never be executed again without someone looking at it.
#[derive(Debug, Clone)]
pub struct Finding {
    /// What went wrong.
    pub fault: Fault,
    /// The exact bytes that caused it.
    pub input: Vec<u8>,
    /// The panic message, or how long the input took.
    pub detail: String,
}

/// What one run found, and how much it looked at.
#[derive(Debug, Clone)]
pub struct Report {
    /// The target that ran.
    pub target: &'static str,
    /// How many seeds the corpus held.
    pub seeds: usize,
    /// How many inputs were executed in total, seeds included.
    pub executions: usize,
    /// The longest a single input took.
    pub slowest: Duration,
    /// Everything that must be looked at. Empty is the ordinary answer.
    pub findings: Vec<Finding>,
}

/// The panic message of the execution in flight, filled by the hook installed for the run.
static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

fn take_panic() -> Option<String> {
    LAST_PANIC
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Executes `input` once, answering the fault it caused.
///
/// The panic hook is the caller's to install: it is process-global, and installing it per input
/// would cost more than the execution.
fn execute(target: &Target, input: &[u8], budget: &Budget) -> (Option<Finding>, Duration) {
    if let Some(path) = &budget.journal {
        let _ = std::fs::write(path, input);
    }
    let per_input = budget.per_input;
    let started = Instant::now();
    let outcome = catch_unwind(AssertUnwindSafe(|| (target.run)(input)));
    let elapsed = started.elapsed();
    let finding = match outcome {
        Err(_) => Some(Finding {
            fault: Fault::Panicked,
            input: input.to_vec(),
            detail: take_panic().unwrap_or_else(|| "panicked without a message".to_owned()),
        }),
        Ok(()) if elapsed > per_input => Some(Finding {
            fault: Fault::TooSlow,
            input: input.to_vec(),
            detail: format!("took {elapsed:?}, and the budget for one input is {per_input:?}"),
        }),
        Ok(()) => None,
    };
    (finding, elapsed)
}

/// Runs `target` over `corpus` and `budget.iterations` mutations of it.
///
/// Every seed is executed unmutated first, so a corpus that already fails is reported before a
/// single mutation is made.
#[must_use]
pub fn run(target: &Target, corpus: &[Vec<u8>], budget: &Budget) -> Report {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(|info| {
        let mut slot = LAST_PANIC
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = Some(info.to_string());
    }));

    let mut report = Report {
        target: target.name,
        seeds: corpus.len(),
        executions: 0,
        slowest: Duration::ZERO,
        findings: Vec::new(),
    };
    let record = |report: &mut Report, finding: Option<Finding>, elapsed: Duration| {
        report.executions += 1;
        report.slowest = report.slowest.max(elapsed);
        if let Some(finding) = finding {
            report.findings.push(finding);
        }
    };
    for seed in corpus {
        let (finding, elapsed) = execute(target, seed, budget);
        record(&mut report, finding, elapsed);
    }
    // An empty corpus still gets fuzzed, from the empty input: a target must survive one.
    let pool: Vec<&[u8]> = if corpus.is_empty() {
        vec![&[]]
    } else {
        corpus.iter().map(Vec::as_slice).collect()
    };
    let mut mutator = Mutator::new(budget.seed);
    let mut chooser = ono_testkit::Rng::seeded(budget.seed ^ 0x5eed);
    for _ in 0..budget.iterations {
        let base = chooser.pick(&pool).copied().unwrap_or(&[]);
        let other = chooser.pick(&pool).copied().unwrap_or(&[]);
        let input = mutator.mutate(base, other);
        let (finding, elapsed) = execute(target, &input, budget);
        record(&mut report, finding, elapsed);
    }

    std::panic::set_hook(previous);
    report
}
