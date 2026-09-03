//! `each` as an item transform rather than a collector (v0.4.1 §0.5.5, §25.1–§25.7, §60.1).
//!
//! v0.4.1 §25.1 is the contract: given `source | each { transform $it } | downstream`, *"Ono MUST
//! be able to begin executing `transform` for the first value before `source` has completed"*.
//! §25.2 says how that is not to be done — *"the normal `each` implementation MUST NOT capture
//! the complete upstream stream into a `Vec<Value>` before block execution"* — and §25.6 draws
//! the consequence that separates the two implementations from the outside: *"`each` MUST accept
//! an unbounded stream because its semantics are incremental."*
//!
//! The shape of the proof is written out in §58.2, work package H6-WP1: *"source emits one value,
//! waits on a barrier, then would emit forever; `each { $it } | take 1` must complete before
//! barrier release."*
//!
//! **The source and the barrier.** `tail file <path> --lines 1 --follow` is that source, built
//! out of the shell's own production provider rather than a test double: it emits the line the
//! file already holds, then waits for the file to grow, and it is marked unbounded because a
//! followed file never closes. The barrier is the file not growing, and every test here holds it
//! shut for the whole run — the file still has exactly one line when the shell exits, which is
//! what "before the barrier releases" means when it is written as something a test can observe
//! rather than as a duration.
//!
//! **What these were.** `should_answer_take_one_before_the_source_closes_when_each_transforms_a_waiting_stream`
//! and `should_accept_an_unbounded_source_when_each_transforms_it` are the §57 phase H0 failure
//! proofs required before the streaming repair of phase H6 landed (issue #31). They were
//! `#[ignore]`d and red until issues #75 and #76 turned them green; ADR-0431 records why they
//! landed ignored, and ADR-0480 is the repair. Neither assertion changed. The third test was
//! green throughout and is the differential that named the defect: the same source and the same
//! `take 1`, with `where` in place of `each`, answered at once while `each` did not.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::path::PathBuf;
use std::time::Duration;

use ono_testkit::{Scratch, scratch};

use support::{Bounded, run_bounded};

/// Long enough that a loaded machine cannot turn a working implementation into a failure, and
/// short enough that a test which never terminates does not hold the suite.
///
/// Nothing here asserts a duration. A streaming `each` answers this in milliseconds and a
/// capturing one never answers at all, so the budget only decides how long a red run takes to
/// report itself (issue #21 and ADR-0252: a wall-clock threshold on shared hardware is a trap).
const BUDGET: Duration = Duration::from_secs(60);

/// The single value the source has produced when a test starts.
const FIRST: &str = "first";

/// A source that has produced one value and will produce no more until its barrier releases.
///
/// The barrier is the file not growing. Nothing in this suite writes to it after `open`, so the
/// shell is asked the question §58.2 asks: can it answer from the value it already has, without
/// the source ever ending?
struct WaitingSource {
    home: Scratch,
    path: PathBuf,
}

impl WaitingSource {
    /// A file holding exactly one line, and a configuration home the run cannot escape.
    fn open() -> Self {
        let home = scratch();
        let path = home.write("waiting/source.log", format!("{FIRST}\n"));
        Self { home, path }
    }

    /// The pipeline `script` reads, with this source at its head.
    fn run(&self, script: &str) -> Bounded {
        let script = format!("tail file {} --lines 1 --follow | {script}", self.display());
        run_bounded(&self.home, &script, BUDGET)
    }

    /// The source path as a script writes it.
    fn display(&self) -> String {
        self.path.display().to_string()
    }

    /// Whether the barrier is still shut: the source has neither closed nor produced a second
    /// value while the shell was being asked.
    fn still_waiting(&self) -> bool {
        std::fs::read_to_string(&self.path).is_ok_and(|text| text == format!("{FIRST}\n"))
    }
}

// --- the failure proofs of v0.4.1 §57 phase H0 ------------------------------------------------

#[test]
fn should_answer_take_one_before_the_source_closes_when_each_transforms_a_waiting_stream() {
    let source = WaitingSource::open();

    let run = source.run("each { @ } | take 1 | to json");

    assert!(
        source.still_waiting(),
        "the proof is only a proof while the source is still waiting: it must not have closed or \
         produced a second value, got {:?}",
        std::fs::read_to_string(&source.path)
    );
    assert!(
        run.finished,
        "v0.4.1 §25.1 and §58.2: `each {{ @ }} | take 1` answers from the value the source has \
         already produced, without waiting for the source to end. {}",
        run.report()
    );
    assert_eq!(
        run.stdout.trim(),
        format!("[{FIRST:?}]"),
        "v0.4.1 §60.1: the pipeline returns the one value the source emitted. {}",
        run.report()
    );
    assert_eq!(
        run.code,
        Some(0),
        "an answered pipeline succeeds. {}",
        run.report()
    );
}

#[test]
fn should_accept_an_unbounded_source_when_each_transforms_it() {
    let source = WaitingSource::open();

    let run = source.run("each { @ } | take 1 | to json");

    assert!(
        !run.stderr.contains("E0801"),
        "v0.4.1 §25.6: `each` accepts an unbounded stream because its semantics are incremental, \
         so a followed file is not an unbounded operation to refuse. {}",
        run.report()
    );
}

// --- the same source, without `each` ----------------------------------------------------------

#[test]
fn should_answer_take_one_before_the_source_closes_when_a_predicate_filters_a_waiting_stream() {
    // The differential that names the defect. `where` is a predicate and `each` is an item
    // transform, and v0.4.1 Appendix E classifies both as stages that never require finite input;
    // this one already behaves that way over the very same source, with the very same `take 1`.
    let source = WaitingSource::open();

    let run = source.run("where @ != \"a line this fixture never writes\" | take 1 | to json");

    assert!(
        source.still_waiting(),
        "the source must still be waiting for this to say anything about streaming, got {:?}",
        std::fs::read_to_string(&source.path)
    );
    assert!(
        run.finished,
        "a streaming stage answers `take 1` from the value the source has already produced. {}",
        run.report()
    );
    assert_eq!(
        run.stdout.trim(),
        format!("[{FIRST:?}]"),
        "the one value the source emitted reaches the end of the pipeline. {}",
        run.report()
    );
}
