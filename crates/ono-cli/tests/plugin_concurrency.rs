//! One KUANG/11 instance answers the shell, its jobs and the stages of one pipeline, up to the
//! package's concurrency ceiling — and an invocation beyond it waits for a slot instead of being
//! refused at once (spec §31.14, §31.15, §18.4; ADR-0586, ADR-0995, ADR-0998, ADR-1011,
//! ADR-1027; v0.6.5 review findings R-F2 and R-F3).
//!
//! Every package here declares `max_concurrent_invocations: 1`, the SDK's own default: one
//! invocation at a time. No test asserts a duration; the shell's watchdog bounds every run.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "kuang")]
#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_testkit::Scratch;
use support::{echo_plugin_home, ono_with_plugins};

const ECHO: &str = "dev.example.echo";

/// A bounded target, an endless one, and a slow bounded one that ends on its own.
const TARGETS: &str = r#"
targets:
  - name: echo-item
    schema: dev.example.echo.item/1
    summary: Items the example package provides.
    identity_doc: Two observations are the same item when their `seq` matches.
    options:
      - name: count
        type: int
        doc: How many items to answer with.
  - name: echo-tick
    schema: dev.example.echo.item/1
    summary: Items emitted until the query is cancelled.
    identity_doc: Two observations are the same tick when their `seq` matches.
    answer: unbounded
  - name: echo-slow
    schema: dev.example.echo.item/1
    summary: Counted items, paced, then the end.
    identity_doc: Two observations are the same item when their `seq` matches.
    options:
      - name: count
        type: int
        doc: How many items to answer with.
      - name: pace-ms
        type: int
        doc: Milliseconds between two items.
"#;

/// The example package, declared to run one invocation at a time.
fn one_at_a_time() -> Scratch {
    let home = echo_plugin_home(ECHO, TARGETS);
    let manifest = home.path().join("plugins").join(ECHO).join("manifest.yaml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest");
    std::fs::write(
        &manifest,
        text.replace(
            "  startup: lazy",
            "  startup: lazy\n  max_concurrent_invocations: 1",
        ),
    )
    .expect("the manifest is rewritten");
    home
}

/// Waits, in the script, until an invocation holds the instance's only slot.
const UNTIL_ACTIVE: &str = "while (get plugin | where id == \"dev.example.echo\" and state == \"active\" | count) == 0 { sleep 0.01 }";

#[test]
fn should_answer_the_shell_once_the_background_job_holding_the_only_slot_ends() {
    // R-F2: a job runs in the session's instance (ADR-0995), so it holds the only slot while it
    // answers. The shell's own invocation waits for it to end rather than being refused with
    // `runtime.concurrency_limit` while the job is still running.
    let home = one_at_a_time();
    let answer = home.path().join("job.json");
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}\n\
             get echo-slow --count 5 --pace-ms 400 | count | to json > '{}' &\n\
             {UNTIL_ACTIVE}\n\
             get echo-item | count | to json\n\
             while (get job | where state == \"running\" | count) > 0 {{ sleep 0.01 }}",
            answer.display()
        ),
    );

    run.assert_success();
    assert!(
        run.stdout().contains("[3]"),
        "the shell's invocation ran once the job's had ended: {:?}",
        run.output()
    );
    assert_eq!(
        std::fs::read_to_string(&answer).unwrap_or_default().trim(),
        "[5]",
        "the job's own answer is whole: {:?}",
        run.output()
    );
}

#[test]
fn should_answer_a_waiting_job_once_the_job_holding_the_only_slot_is_killed() {
    // R-F2: the slot is held by an invocation that never ends on its own. `kill %1` cancels it
    // (ADR-0998), and the invocation waiting behind it runs.
    let home = one_at_a_time();
    let answer = home.path().join("waiting.json");
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}\n\
             echo:count-forever | count &\n\
             {UNTIL_ACTIVE}\n\
             get echo-item | count | to json > '{}' &\n\
             sleep 0.3\n\
             kill %1\n\
             while (get job | where state == \"running\" | count) > 0 {{ sleep 0.01 }}",
            answer.display()
        ),
    );

    assert!(
        !run.stderr().contains("concurrency_limit"),
        "the waiting invocation was not refused: {:?}",
        run.output()
    );
    assert_eq!(
        std::fs::read_to_string(&answer).unwrap_or_default().trim(),
        "[3]",
        "the job that waited for the slot ran once the holder was killed: {:?}",
        run.output()
    );
}

#[test]
fn should_run_each_over_an_endless_upstream_that_take_has_ended() {
    // R-F3: `take 1` ends the upstream's invocation, which gives its slot back, and the stage
    // after it may then invoke the same package — not be refused while the cancellation is on
    // its way.
    let home = one_at_a_time();
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}; get echo-tick | take 1 | each {{ get echo-item --count 1 | count }} | to json"
        ),
    );

    run.assert_success();
    assert!(
        run.stdout().contains("[1]"),
        "the block ran in the slot the taken upstream gave back: {:?}",
        run.output()
    );
}

#[test]
fn should_run_each_over_a_slow_bounded_upstream_of_the_same_package() {
    // R-F3: since ADR-1011 a bounded answer streams, so it holds its slot while the stages after
    // it run. The block's invocation waits until the upstream has answered and ended.
    let home = one_at_a_time();
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}; get echo-slow --count 3 --pace-ms 50 | each {{ get echo-item --count 1 | count }} | to json"
        ),
    );

    run.assert_success();
    assert!(
        run.stdout().contains("[1,1,1]"),
        "the block ran once per upstream item: {:?}",
        run.output()
    );
}

#[test]
fn should_run_each_over_a_slow_bounded_upstream_that_take_has_ended() {
    let home = one_at_a_time();
    let run = ono_with_plugins(
        &home,
        &format!(
            "load plugin {ECHO}; get echo-slow --count 3 --pace-ms 50 | take 1 | each {{ get echo-item --count 1 | count }} | to json"
        ),
    );

    run.assert_success();
    assert!(
        run.stdout().contains("[1]"),
        "the block ran in the slot the taken upstream gave back: {:?}",
        run.output()
    );
}
