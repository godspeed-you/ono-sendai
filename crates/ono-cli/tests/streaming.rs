//! `each` and functions as pipeline stages that produce incrementally (v0.4.1 §25, §26, §28).
//!
//! v0.4.1 §2.5 fixes what the word means: *"a command is 'streaming' only if it can consume and
//! produce incrementally without first waiting for the complete upstream stream"*, and names
//! `each` among the commands that MUST be. §25.2 says how that is not to be done — no
//! complete-input `Vec<Value>` — and §25.6 draws the consequence a test can see from outside:
//! an unbounded source is a legal input.
//!
//! **No test here measures a duration.** ADR-0431 settled the shape and ADR-0480 keeps it: the
//! source is `tail file <path> --follow`, the shell's own file provider, and the barrier is the
//! file not growing. "Before the source closes" is written as a fact read off the disk after the
//! run — the file still holds the lines it held — rather than as a stopwatch reading, because
//! issue #21 and ADR-0252 are this repository's record of what a wall-clock threshold on shared
//! hardware costs.

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
/// short enough that a test which never terminates does not hold the suite. Nothing is asserted
/// about it: a streaming implementation answers in milliseconds and a capturing one never
/// answers at all (ADR-0431).
const BUDGET: Duration = Duration::from_secs(60);

/// A followed file: a source that has produced what it holds and will produce no more until
/// something writes to it.
struct Following {
    home: Scratch,
    path: PathBuf,
}

impl Following {
    /// A followed file holding `lines`, and a configuration home the run cannot escape.
    fn holding(lines: &[&str]) -> Self {
        let home = scratch();
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        let path = home.write("streaming/source.log", text);
        Self { home, path }
    }

    /// The pipeline `script` reads, with this source at its head, following `lines` of it.
    fn run(&self, lines: usize, script: &str) -> Bounded {
        let script = format!(
            "tail file {} --lines {lines} --follow | {script}",
            self.path.display()
        );
        run_bounded(&self.home, &script, BUDGET)
    }

    /// The lines the file holds now.
    fn lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

// --- `each` consumes and emits incrementally (§25.1–§25.4, §60.1, issue #75) -------------------

#[test]
fn should_emit_the_first_value_before_the_source_closes() {
    // v0.4.1 §25.1: "Ono MUST be able to begin executing `transform` for the first value before
    // `source` has completed, provided `source` has produced that value." The source here never
    // completes — a followed file has no end — so the only way to answer is to answer from the
    // value it has already produced.
    let source = Following::holding(&["first"]);

    let run = source.run(1, "each { @ } | take 1 | to json");

    assert_eq!(
        source.lines(),
        ["first"],
        "the source must still be waiting for this to say anything about streaming"
    );
    assert!(
        run.finished,
        "v0.4.1 §25.1 and §60.1: the pipeline answers from the value the source has already \
         produced, without waiting for the source to end. {}",
        run.report()
    );
    assert_eq!(
        run.stdout.trim(),
        "[\"first\"]",
        "v0.4.1 §60.1: `each` returns the one value the source emitted. {}",
        run.report()
    );
}

#[test]
fn should_run_the_block_for_one_item_before_the_next_item_is_required() {
    // v0.4.1 §25.4: values a block emits for one input item "MUST be forwarded before the next
    // input item is required". The proof makes the block itself produce the next input item: it
    // appends a second line to the very file the source is following. A `each` that captured its
    // input first would be waiting for a source that cannot end, and the second line — which only
    // the block writes — would never exist.
    let source = Following::holding(&["first"]);
    let path = source.path.display().to_string();

    let run = source.run(
        1,
        &format!(
            "each {{ if @ == \"first\" {{ let next = \"second\"; $next | to text >> {path} }}; @ }} \
             | take 2 | to json"
        ),
    );

    assert!(
        run.finished,
        "the block runs for the first item, which is what produces the second. {}",
        run.report()
    );
    assert_eq!(
        source.lines(),
        ["first", "second"],
        "the block's side effect for the first item happened while the source was still open. {}",
        run.report()
    );
    assert_eq!(
        run.stdout.trim(),
        "[\"first\",\"second\"]",
        "both items reach the end of the pipeline, in the order the source produced them. {}",
        run.report()
    );
}

#[test]
fn should_keep_the_input_order_and_the_serial_execution_of_the_block() {
    // v0.4.1 §25.3: "the default `each` behavior remains serial and preserves input order", and
    // "v0.4.1 does not introduce parallel `each` as a feature". Both halves are asserted: the
    // values leave in the order they arrived, and the block's side effects — one appended line
    // per item — are in that same order, which a parallel or reordered implementation could not
    // manage.
    let scratch = scratch();
    let trace = scratch.path().join("trace.log");
    let script = format!(
        "echo \"[1,2,3,4,5]\" | from json | each {{ @ | to text >> {}; @ }} | to json",
        trace.display()
    );

    let run = run_bounded(&scratch, &script, BUDGET);

    assert!(run.finished, "{}", run.report());
    assert_eq!(
        run.stdout.trim(),
        "[1,2,3,4,5]",
        "the output keeps the input order. {}",
        run.report()
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap_or_default(),
        "1\n2\n3\n4\n5\n",
        "the block ran once per item, in order, with no invocation overlapping another. {}",
        run.report()
    );
}

// --- `each` accepts an unbounded source (§25.6, §25.7, §60.1, issue #76) -----------------------

#[test]
fn should_accept_a_source_declared_unbounded_without_refusing_it() {
    // v0.4.1 §25.6: "`each` MUST accept an unbounded stream because its semantics are
    // incremental." A followed file is such a stream — it declares itself unbounded because it
    // never closes — and the capture-based implementation refused it with
    // `stream.unbounded_operation` rather than reading one value from it.
    let source = Following::holding(&["first"]);

    let run = source.run(1, "each { @ } | take 1 | to json");

    assert!(
        !run.stderr.contains("E0801") && !run.stderr.contains("unbounded_operation"),
        "an unbounded source is a legal input to `each`, not an operation to refuse. {}",
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
fn should_keep_memory_flat_while_an_unbounded_source_is_consumed() {
    // The `Done` line of the work package in v0.4.1 §58.2: "memory stays within bounded channel
    // plus per-item frame overhead". What a test can count from outside the process is how much
    // of the source the shell had to put through the block before it could answer — and the
    // answer must not depend on how much the source holds.
    //
    // Each run's block records one line per invocation. The pipeline asks for one value, so a
    // streaming `each` runs the block for the values already in the bounded channels and no
    // more, whether the source holds two hundred values or two thousand. An implementation that
    // grew with its input would show it here as a count that grew with it.
    let small = worked_through(200);
    let large = worked_through(2000);

    assert!(
        small >= 1 && large >= 1,
        "the block ran at least once in each run, got {small} and {large}"
    );
    assert!(
        small < 200 && large < 200,
        "answering `take 1` cost a bounded prefix of the source in both runs, and the cost did \
         not grow with the source: 200 available values cost {small} invocations, 2000 available \
         values cost {large}"
    );
}

/// How many times the block ran while a `take 1` pipeline answered from a source holding `lines`
/// values.
fn worked_through(lines: usize) -> usize {
    let held: Vec<String> = (0..lines).map(|line| format!("line-{line}")).collect();
    let source = Following::holding(&held.iter().map(String::as_str).collect::<Vec<_>>());
    let trace = source.home.path().join("worked.log");
    let run = source.run(
        lines,
        &format!(
            "each {{ @ | to text >> {}; @ }} | take 1 | to json",
            trace.display()
        ),
    );

    assert!(run.finished, "{}", run.report());
    assert_eq!(
        run.stdout.trim(),
        "[\"line-0\"]",
        "the answer is the first value the source produced. {}",
        run.report()
    );
    std::fs::read_to_string(&trace)
        .unwrap_or_default()
        .lines()
        .count()
}

// --- control flow survives the rewrite (§25.5, §30.3, §60.3, issue #77) ------------------------

#[test]
fn should_stop_upstream_consumption_promptly_when_the_block_breaks() {
    // v0.4.1 §25.5: "`break` stops consuming upstream and cancels the remaining source where
    // possible", and §60.3 is the scenario — an unbounded source, a block that breaks. Written
    // without a clock: the source here has no end, so "promptly" is the difference between a run
    // that terminates and one that does not (ADR-0459).
    let source = Following::holding(&["first", "second", "third"]);
    let trace = source.home.path().join("broke.log");

    let run = source.run(
        3,
        &format!(
            "each {{ @ | to text >> {}; break }} | to json",
            trace.display()
        ),
    );

    assert!(
        run.finished,
        "`break` stops reading a source that would never have ended on its own. {}",
        run.report()
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap_or_default(),
        "first\n",
        "the block ran for the item it broke on and for no item after it. {}",
        run.report()
    );
    assert_eq!(
        source.lines(),
        ["first", "second", "third"],
        "and the source was left as it was: nothing here waited for it to end"
    );
}

#[test]
fn should_skip_exactly_one_item_when_the_block_continues() {
    // v0.4.1 §25.5: "`continue` skips the remainder of the current item". The remainder — the
    // trace line and the value — is what the second item does not produce; the third item is
    // unaffected.
    let scratch = scratch();
    let trace = scratch.path().join("continued.log");
    let script = format!(
        "echo \"[1,2,3]\" | from json | each {{ if @ == 2 {{ continue }}; @ | to text >> {}; @ }} \
         | to json",
        trace.display()
    );

    let run = run_bounded(&scratch, &script, BUDGET);

    assert!(run.finished, "{}", run.report());
    assert_eq!(
        run.stdout.trim(),
        "[1,3]",
        "the continued item contributes nothing and the stream carries on. {}",
        run.report()
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap_or_default(),
        "1\n3\n",
        "the statements after `continue` did not run for the skipped item, and did run for the \
         one after it. {}",
        run.report()
    );
}

#[test]
fn should_return_from_the_enclosing_function_when_the_block_returns() {
    // v0.4.1 §25.5: "`return` exits the containing function according to existing language
    // semantics" — the function's value is the returned one, the statements after the pipeline
    // do not run, and the items after the returning one are never read.
    let scratch = scratch();
    let trace = scratch.path().join("returned.log");
    let script = format!(
        "fn pick() {{\n  \
           echo \"[1,2,3]\" | from json | each {{ @ | to text >> {}; if @ == 2 {{ return @ }} }}\n  \
           echo \"the statement after the pipeline\"\n\
         }}\n\
         pick | to json",
        trace.display()
    );

    let run = run_bounded(&scratch, &script, BUDGET);

    assert!(run.finished, "{}", run.report());
    assert_eq!(
        run.stdout.trim(),
        "[2]",
        "the function's value is what the block returned, and nothing after the pipeline ran. {}",
        run.report()
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap_or_default(),
        "1\n2\n",
        "the item after the returning one was never read. {}",
        run.report()
    );
}

#[test]
fn should_propagate_a_block_error_with_the_status_it_had_before_the_rewrite() {
    // v0.4.1 §25.5: "an unhandled error follows existing error propagation and MUST stop or
    // continue exactly as the language contract defines". ADR-0008 fixes the statuses: a command
    // that cannot be found is 127, and a command that ran and failed is 1. Neither is a property
    // of `each`, which is the point — the rewrite changed how the block is reached, not what
    // happens when it fails.
    let scratch = scratch();
    let trace = scratch.path().join("failed.log");

    let missing = run_bounded(
        &scratch,
        &format!(
            "echo \"[1,2,3]\" | from json | each {{ @ | to text >> {}; a-command-no-host-has }} \
             | to json",
            trace.display()
        ),
        BUDGET,
    );

    assert_eq!(
        missing.code,
        Some(127),
        "a command the block cannot resolve is 127, as it is anywhere else (ADR-0008). {}",
        missing.report()
    );
    assert!(
        missing.stderr.contains("resolve.command_not_found"),
        "the block's own error is what is reported. {}",
        missing.report()
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap_or_default(),
        "1\n",
        "an unhandled error stops the run: no item after the failing one was read. {}",
        missing.report()
    );

    let failing = run_bounded(
        &scratch,
        "echo \"[1,2,3]\" | from json | each { get file /no/such/path/at/all } | to json",
        BUDGET,
    );
    assert_eq!(
        failing.code,
        Some(1),
        "a command that ran and failed is 1. {}",
        failing.report()
    );
}

#[test]
fn should_leave_the_shell_with_the_status_the_block_exited_with() {
    // §25.5 lists four jumps and the box in `docs/ACCEPTANCE.md` §4.8.7 adds the fifth the
    // language has: `exit` unwinds to the top and the shell leaves with the status it was given
    // (ADR-0008), from inside a streamed block exactly as from anywhere else.
    let scratch = scratch();
    let trace = scratch.path().join("exited.log");
    let script = format!(
        "echo \"[1,2,3]\" | from json | each {{ @ | to text >> {}; if @ == 2 {{ exit 3 }} }} \
         | to json",
        trace.display()
    );

    let run = run_bounded(&scratch, &script, BUDGET);

    assert_eq!(
        run.code,
        Some(3),
        "the shell leaves with the status `exit` was given. {}",
        run.report()
    );
    assert_eq!(
        std::fs::read_to_string(&trace).unwrap_or_default(),
        "1\n2\n",
        "and it leaves at once: the third item was never read. {}",
        run.report()
    );
}

// --- a function is a pipeline stage (§26.2, §26.3, issue #79) ---------------------------------

#[test]
fn should_forward_values_from_a_function_as_it_produces_them() {
    // v0.4.1 §26.2: "a function used as a pipeline stage SHOULD be able to stream values to
    // downstream stages when the function body itself streams", and "the preferred v0.4.1 outcome
    // is streaming continuation rather than preservation of an accidental capture architecture".
    //
    // The body here streams and never ends, so a capture architecture has nothing to hand on: it
    // refuses the body with `stream.unbounded_operation` rather than reading one value from it.
    let source = Following::holding(&["first"]);
    let script = format!(
        "fn watched() {{ tail file {} --lines 1 --follow }}\nwatched | take 1 | to json",
        source.path.display()
    );

    let run = run_bounded(&source.home, &script, BUDGET);

    assert!(
        run.finished,
        "the caller's `take 1` answers from the value the function's body has already produced. \
         {}",
        run.report()
    );
    assert_eq!(
        run.stdout.trim(),
        "[\"first\"]",
        "the function's values reach the stage after it. {}",
        run.report()
    );
    assert_eq!(
        source.lines(),
        ["first"],
        "and the body's own source never ended: nothing waited for it to"
    );
}

#[test]
fn should_keep_a_pipeline_streaming_when_a_function_sits_in_the_middle_of_it() {
    // The same continuation with stages on both sides of the call: the body has a producer and a
    // transform, the caller has a prefix and a serializer, and no stage between them collects. A
    // function that turned its pipeline into two phases would have to see the end of the body
    // before `take 1` could be answered, and the body has no end.
    //
    // What the language does *not* have is a call between two stages — `get process | mine |
    // take 1` reads `mine` as a program, and giving a function an input stream is a language
    // feature rather than a streaming repair (ADR-0481, reported for the backlog).
    let source = Following::holding(&["first", "second", "third", "fourth"]);
    let script = format!(
        "fn watched() {{ tail file {} --lines 4 --follow | where @ != \"a line nothing writes\" }}\n\
         watched | take 1 | to json",
        source.path.display()
    );

    let run = run_bounded(&source.home, &script, BUDGET);

    assert!(
        run.finished,
        "the caller's `take 1` is answered without the body's transform seeing the end of its \
         input. {}",
        run.report()
    );
    assert_eq!(
        run.stdout.trim(),
        "[\"first\"]",
        "the first value crossed the call, the transform and the prefix. {}",
        run.report()
    );
    assert_eq!(
        source.lines(),
        ["first", "second", "third", "fourth"],
        "and the body's source is still open"
    );
}

#[test]
fn should_drop_the_invocation_scope_when_the_function_call_ends() {
    // v0.4.1 §26.3: "streaming a block/function MUST NOT let lexical scope references outlive
    // their owning scope unsafely" and "the refactor MUST preserve deterministic variable binding
    // and mutation semantics".
    //
    // Three things are asserted together because they are one property: the parameter is visible
    // inside the body while it streams, a caller's binding of the same name is untouched by the
    // call, and the parameter is gone once the call has returned.
    let scratch = scratch();
    let script = "let held = \"the caller's\"\n\
                  fn tagged(held) { echo \"[1,2]\" | from json | each { $held } }\n\
                  tagged \"the callee's\" | to json\n\
                  echo $held\n\
                  echo (\"[\" + $held + \"]\")";

    let run = run_bounded(&scratch, script, BUDGET);

    assert!(run.finished, "{}", run.report());
    assert!(
        run.stdout.contains("[\"the callee's\",\"the callee's\"]"),
        "the parameter is what the streamed body reads, for every value. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("the caller's"),
        "the caller's binding of the same name survived the call unchanged. {}",
        run.report()
    );
    assert!(
        !run.stdout.contains("[the callee's]"),
        "the invocation's binding did not outlive the invocation. {}",
        run.report()
    );
}

#[test]
fn should_say_in_explain_which_calls_stream_and_which_collect() {
    // v0.4.1 §26.2: "if function semantics currently require a complete function result before
    // continuation, that limitation MUST be explicit in `explain`". A body that is one native
    // pipeline is continued as a stage; anything else is collected first, and the plan says which
    // of the two a call is before it is run rather than leaving a user to meet the difference as
    // a refusal over an unbounded source.
    let scratch = scratch();

    let streams = run_bounded(
        &scratch,
        "fn watched() { get process | take 4 }\nexplain watched | where pid > 0",
        BUDGET,
    );
    assert!(
        streams
            .stdout
            .contains("its body streams into the stages after the call"),
        "a continuable body is named as one. {}",
        streams.report()
    );

    let collects = run_bounded(
        &scratch,
        "fn counted() {\n  let wanted = 4\n  get process | take 4\n}\n\
         explain counted | where pid > 0",
        BUDGET,
    );
    assert!(
        collects
            .stdout
            .contains("its result is collected before the stages after the call run"),
        "and a body that cannot be continued says so, with the finite-input requirement that \
         follows from it. {}",
        collects.report()
    );
}

#[test]
fn should_refuse_an_unbounded_body_the_call_would_have_to_collect() {
    // The other half of §26.2's requirement: a call that collects "MUST have a finite-input/budget
    // guard". §65.8 is the rule behind it — "waiting forever for an explicitly unbounded stream to
    // finish is forbidden. Refuse early." — so the refusal arrives instead of a shell that never
    // answers.
    let source = Following::holding(&["first"]);
    let script = format!(
        "fn watched() {{\n  let wanted = 1\n  tail file {} --lines 1 --follow\n}}\n\
         watched | take 1 | to json",
        source.path.display()
    );

    let run = run_bounded(&source.home, &script, BUDGET);

    assert!(
        run.finished,
        "a call that has to collect an unbounded body refuses rather than waiting for an end \
         that never comes. {}",
        run.report()
    );
    assert!(
        run.stderr.contains("unbounded_operation"),
        "and the refusal is the structured one, naming the stream it would not read. {}",
        run.report()
    );
}

// --- backpressure and cancellation survive the rewrite (§28.3, §28.4, issue #81) ---------------

#[test]
fn should_reap_the_child_process_of_a_cancelled_stage() {
    // v0.4.1 §28.4: "where a streaming pipeline stage owns an external process, cancellation MUST
    // close or signal it using the existing process/job-control policy rather than leaving an
    // orphan merely because downstream stopped reading."
    //
    // `adapt find` is such a stage: its decoder is line-oriented, so records reach the pipeline
    // while the child is still walking the tree. The child is given the scratch directory first
    // and the whole filesystem after it, so it produces its first record at once and then has
    // work left for as long as the assertion needs — and the scratch path in its command line is
    // what makes it this test's child rather than somebody else's.
    let scratch = scratch();
    scratch.write("found/first.txt", "");
    let marker = scratch.path().join("found");
    let script = format!(
        "adapt find {} / -type f | take 1 | to json",
        marker.display()
    );

    let run = run_bounded(&scratch, &script, BUDGET);

    assert!(
        run.finished,
        "`take 1` answers from the first record the child produced. {}",
        run.report()
    );
    assert!(
        run.stdout.contains("first.txt"),
        "the stage really owned a child and really streamed a record out of it — without that \
         this test would prove nothing. {}",
        run.report()
    );
    assert_eq!(
        processes_naming(&marker.display().to_string()),
        Vec::<u32>::new(),
        "the shell has been reaped and the child it stopped reading from is still running. {}",
        run.report()
    );
}

/// The pids of every process whose command line names `needle`.
///
/// Read out of `/proc` rather than from `ps`, so the test depends on the kernel interface the
/// shell's own process provider depends on and on no other program.
fn processes_naming(needle: &str) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        if String::from_utf8_lossy(&cmdline).contains(needle) {
            found.push(pid);
        }
    }
    found
}
