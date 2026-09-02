//! The process family the contract declares, plus the one
//! contract behaviour every mutation owes: a failed row makes the exit status non-zero.
//!
//! Contracts: `docs/spec/commands/process.yaml` (`ono.process.inspect`, `ono.job.get`,
//! `ono.process.enter`, `ono.process.set`, `ono.signal.send`, `ono.process.kill`,
//! `ono.process.stop`), schemas `ono.job/1`, `ono.action-result/1`, `ono.context/1`, and the
//! deferred `ono.process-detail/1` whose fields spec §33.1 names.
//!
//! Spec §9.1 (process commands), §14.3 (object context supplies the selector), §16.5 (partial
//! failure is never collapsed), §18.4 (`get job` returns structured job objects), §33.1 (the
//! detail view: parent, cgroup, open files, sockets). ADR-0006: the aggregate exit status of a
//! mutation is derived from its ActionResults. ADR-0023: a frame narrows, never redirects.
//! ADR-0024: a backgrounded live view is a job in the same table as an external command.
//!
//! Helper children are real `sleep` processes the test spawns and kills, so a signal or a
//! priority change lands on something the test owns and nothing outlives the test.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::Duration;

use ono_testkit::ono;
use serde_yaml_ng::Value;

mod support;
use support::SleepChild;

/// Parses one line of `to json` output — a JSON array of the stream's values (spec §33.5).
/// `context` is what the shell said on stderr, so a command that answered with a diagnostic
/// instead of rows fails with that diagnostic in view.
fn rows(json_line: &str, context: &str) -> Vec<Value> {
    let parsed: Value = serde_yaml_ng::from_str(json_line).unwrap_or_else(|error| {
        panic!("`to json` emits JSON on stdout, got {json_line:?} ({error}); stderr: {context:?}")
    });
    parsed
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("`to json` emits a JSON array on stdout, got {json_line:?}; stderr: {context:?}")
        })
        .clone()
}

/// The one JSON array a single `to json` stage printed.
fn json_rows(run: &ono_testkit::Run) -> Vec<Value> {
    rows(run.stdout().trim(), run.stderr())
}

/// The JSON arrays a script printed, one per `to json` stage, in order.
fn json_lines(run: &ono_testkit::Run) -> Vec<Vec<Value>> {
    run.stdout()
        .lines()
        .filter(|line| line.starts_with('['))
        .map(|line| rows(line, run.stderr()))
        .collect()
}

fn field<'a>(row: &'a Value, name: &str) -> &'a Value {
    row.get(name)
        .unwrap_or_else(|| panic!("the record carries a `{name}` field, got {row:?}"))
}

fn text(row: &Value, name: &str) -> String {
    match field(row, name) {
        Value::String(s) => s.clone(),
        other => serde_yaml_ng::to_string(other).expect("a value serialises"),
    }
}

/// Whether an ActionResult's structured error carries the given dotted error name or code,
/// whatever nesting the error value renders with.
fn error_names(row: &Value, dotted: &str, code: &str) -> bool {
    let rendered = serde_yaml_ng::to_string(field(row, "error")).expect("an error serialises");
    rendered.contains(dotted) || rendered.contains(code)
}

// --- inspect process -----------------------------------------------------------------------

#[test]
fn should_return_a_process_detail_record_when_inspecting_a_pid() {
    // Spec §9.1: `inspect process <pid>` returns a ProcessDetail; §33.1 shows what it carries
    // beyond the Process record — the parent, the cgroup, open files and sockets.
    let run = ono("inspect process 1 | to json");
    run.assert_success();
    let detail = json_rows(&run);
    assert_eq!(
        detail.len(),
        1,
        "spec §9.1: `inspect process` is a single `ono.process-detail/1` record, got {detail:?}"
    );
    let record = &detail[0];
    assert_eq!(
        field(record, "pid").as_i64(),
        Some(1),
        "spec §33.1: the detail carries the pid it was asked about, got {record:?}"
    );
    for name in ["parent", "cgroup", "open_files", "sockets"] {
        assert!(
            record.get(name).is_some(),
            "spec §33.1: the detail view carries `{name}` (null when unreadable, never absent), got {record:?}"
        );
    }
}

#[test]
fn should_inspect_the_process_arriving_through_the_pipeline() {
    // process.yaml: input `null | ono.process/1` — "omitted when one arrives through the
    // pipeline", as the contract example `... | take 1 | inspect process` does.
    let run = ono("get process 1 | inspect process | to json");
    run.assert_success();
    let detail = json_rows(&run);
    assert_eq!(
        detail.len(),
        1,
        "one process in, one detail out, got {detail:?}"
    );
    assert_eq!(
        field(&detail[0], "pid").as_i64(),
        Some(1),
        "the detail describes the process the pipeline delivered, got {:?}",
        detail[0]
    );
}

#[test]
fn should_fail_structured_when_inspecting_a_pid_that_does_not_exist() {
    let run = ono("inspect process 4000000 | to json");
    assert!(
        !run.status().is_success(),
        "inspecting nothing is a failure, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0301"),
        "errors.yaml: a process that does not exist is `io.not_found`, got {:?}",
        run.stderr()
    );
}

// --- get job ---------------------------------------------------------------------------------

#[test]
fn should_list_a_backgrounded_external_pipeline_as_a_job() {
    // Spec §18.4: "`get job` returns structured job objects". job.v1: an external job owns a
    // process group and the pids in it.
    let run = ono(
        "sleep 3 > /dev/null 2> /dev/null &; get job | to json; get process | where name == \"sleep\" | select pid | to json",
    );
    run.assert_success();
    let printed = json_lines(&run);
    assert_eq!(
        printed.len(),
        2,
        "spec §18.4: `get job` answers with rows like any other query, so two `to json` stages print two arrays; stdout {:?}, stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    let jobs = &printed[0];
    assert_eq!(jobs.len(), 1, "one job was backgrounded, got {jobs:?}");
    let job = &jobs[0];
    assert_eq!(
        field(job, "id").as_i64(),
        Some(1),
        "job.v1: the first job is number 1"
    );
    assert_eq!(
        text(job, "kind"),
        "external",
        "job.v1: `sleep` is an external process group"
    );
    assert_eq!(
        text(job, "state"),
        "running",
        "job.v1: a sleeping child is still running"
    );
    assert!(
        text(job, "command").contains("sleep 3"),
        "job.v1: `command` is the pipeline as typed, got {job:?}"
    );
    assert_eq!(
        field(job, "current").as_bool(),
        Some(true),
        "job.v1: the only job is the current one"
    );
    assert!(
        field(job, "process_group").is_i64(),
        "job.v1: an external job has a process group, got {job:?}"
    );
    assert!(
        field(job, "exit_status").is_null(),
        "job.v1: no exit status while running, got {job:?}"
    );
    assert!(
        field(job, "started").is_string(),
        "job.v1: `started` is a timestamp, got {job:?}"
    );

    let sleeping: Vec<i64> = printed[1]
        .iter()
        .filter_map(|row| field(row, "pid").as_i64())
        .collect();
    let pids = field(job, "pids")
        .as_sequence()
        .unwrap_or_else(|| panic!("job.v1: an external job lists its pids, got {job:?}"));
    assert!(
        !pids.is_empty(),
        "job.v1: the job has at least the `sleep` pid, got {job:?}"
    );
    for pid in pids {
        let pid = pid.as_i64().expect("a pid is an int");
        assert!(
            sleeping.contains(&pid),
            "job.v1: every pid in the job is a live `sleep` the shell started; {pid} is not among {sleeping:?}"
        );
    }
}

#[test]
fn should_list_a_detached_live_view_as_a_native_job() {
    // ADR-0024: a backgrounded watch is a job in the same table; job.v1: a native job owns no
    // process group and no pids.
    let run = ono("watch process --every 200ms &; get job | to json");
    run.assert_success();
    let jobs = json_rows(&run);
    assert_eq!(jobs.len(), 1, "one live view was detached, got {jobs:?}");
    let job = &jobs[0];
    assert_eq!(
        text(job, "kind"),
        "native",
        "job.v1: a detached watch is a native job, got {job:?}"
    );
    assert_eq!(
        text(job, "state"),
        "running",
        "ADR-0024: a live stream runs until cancelled"
    );
    assert!(
        field(job, "process_group").is_null(),
        "job.v1: a native job has no process group, got {job:?}"
    );
    assert!(
        field(job, "pids").is_null(),
        "job.v1: a native job has no pids, got {job:?}"
    );
    assert!(
        text(job, "command").contains("watch process"),
        "job.v1: `command` is the pipeline as typed, got {job:?}"
    );
}

#[test]
fn should_resolve_one_job_by_its_number() {
    // process.yaml: selector `id` — "Resolve one job by its job number."
    let run = ono(
        "sleep 3 > /dev/null 2> /dev/null &; watch process --every 200ms &; get job 2 | to json",
    );
    run.assert_success();
    let jobs = json_rows(&run);
    assert_eq!(
        jobs.len(),
        1,
        "a job number names exactly one job, got {jobs:?}"
    );
    assert_eq!(
        field(&jobs[0], "id").as_i64(),
        Some(2),
        "the selected job is number 2, got {jobs:?}"
    );
    assert_eq!(
        text(&jobs[0], "kind"),
        "native",
        "job 2 is the watch, got {jobs:?}"
    );
}

#[test]
fn should_report_a_finished_job_as_done_with_its_exit_status() {
    // job.v1: `exit_status` is null while running and the status once the job finished; the
    // contract example `get job | where state == running` shows `state` composes with `where`.
    let run = ono("get process | count &; sleep 0.4; get job | where state == \"done\" | to json");
    run.assert_success();
    let jobs = json_rows(&run);
    assert_eq!(
        jobs.len(),
        1,
        "the bounded pipeline finished and is the one done job, got {jobs:?}"
    );
    let job = &jobs[0];
    assert_eq!(
        text(job, "state"),
        "done",
        "job.v1: a finished job is `done`, got {job:?}"
    );
    assert_eq!(
        field(job, "exit_status").as_i64(),
        Some(0),
        "job.v1: a pipeline that succeeded reports exit status 0, got {job:?}"
    );
}

// --- enter process ---------------------------------------------------------------------------

#[test]
fn should_push_an_object_frame_when_entering_a_process() {
    // context.v1: an entered object is a frame of kind `object` whose target names what was
    // entered and whose identity is rendered the way the prompt shows it.
    let run = ono("enter process 1; get context | to json");
    run.assert_success();
    let stack = json_rows(&run);
    assert_eq!(
        stack.len(),
        2,
        "ground frame plus the entered process, got {stack:?}"
    );
    let frame = &stack[1];
    assert_eq!(
        text(frame, "kind"),
        "object",
        "context.v1: an entered process is an object frame, got {frame:?}"
    );
    assert_eq!(
        text(frame, "target"),
        "process",
        "context.v1: the frame narrows to `process`, got {frame:?}"
    );
    assert_eq!(
        text(frame, "identity").trim(),
        "1",
        "context.v1: the identity is the pid, got {frame:?}"
    );
}

#[test]
fn should_narrow_get_process_to_the_entered_process() {
    // Spec §14.3: the object context provides an implicit selector; ADR-0023: a frame narrows.
    let run = ono("enter process 1; get process | select pid | to json");
    run.assert_success();
    let processes = json_rows(&run);
    let pids: Vec<i64> = processes
        .iter()
        .filter_map(|row| field(row, "pid").as_i64())
        .collect();
    assert_eq!(
        pids,
        vec![1],
        "spec §14.3: inside `enter process 1`, `get process` is `get process 1`; got {} processes, stderr {:?}",
        pids.len(),
        run.stderr()
    );
}

#[test]
fn should_trace_the_entered_process_without_a_selector() {
    // Spec §14.3 again, for a command whose selector is otherwise mandatory. The entered process
    // is one the test owns, so a trace rooted anywhere else (say, at init) is visibly wrong.
    let child = SleepChild::spawn();
    let run = ono(&format!(
        "enter process {}; trace process | to json",
        child.pid()
    ));
    run.assert_success();
    let graphs = json_rows(&run);
    assert_eq!(
        graphs.len(),
        1,
        "one graph for the one entered process, got {graphs:?}"
    );
    let root = field(&graphs[0], "root");
    let rendered = serde_yaml_ng::to_string(root).expect("a node serialises");
    assert!(
        rendered.contains(&format!("pid: {}\n", child.pid())),
        "graph.v1: the root of the trace is the entered process {}, got {rendered}",
        child.pid()
    );
}

#[test]
fn should_pop_the_process_frame_when_leaving() {
    // Spec §14.1: `leave` pops the frame `enter` pushed — quietly, because there was one to pop.
    let run = ono("enter process 1; leave; get context | to json");
    run.assert_success();
    assert!(
        run.stderr().is_empty(),
        "entering and leaving a real process is a clean round trip, got {:?}",
        run.stderr()
    );
    let stack = json_rows(&run);
    assert_eq!(
        stack.len(),
        1,
        "only the ground frame remains after `leave`, got {stack:?}"
    );
}

#[test]
fn should_refuse_to_enter_a_process_that_does_not_exist() {
    let run = ono("enter process 4000000");
    assert!(
        !run.status().is_success(),
        "entering nothing must fail, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E1001"),
        "the refusal is the structured not-found of a failed navigation (errors.yaml \
         `spatial.not_found`, ADR-0191), got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("4000000"),
        "the refusal names the pid nothing answers to, got {:?}",
        run.stderr()
    );
}

// --- set process -----------------------------------------------------------------------------

#[test]
fn should_set_the_niceness_of_a_process() {
    // process.yaml: `set process <pid> --priority N` — "The scheduling niceness to apply." An
    // unprivileged user may always lower priority, so the child's nice value afterwards is N.
    let child = SleepChild::spawn();
    let run = ono(&format!(
        "set process {} --priority 10 | to json",
        child.pid()
    ));
    run.assert_success();
    let results = json_rows(&run);
    assert_eq!(
        results.len(),
        1,
        "one target, one ActionResult (spec §11.5), got {results:?}"
    );
    let result = &results[0];
    assert_eq!(
        text(result, "status"),
        "success",
        "the priority change succeeded, got {result:?}"
    );
    assert_eq!(
        field(result, "changed").as_bool(),
        Some(true),
        "action-result.v1: the state changed"
    );
    assert_eq!(
        child.niceness(),
        10,
        "the kernel reports the niceness `set process --priority 10` asked for"
    );
}

#[test]
fn should_report_a_denied_priority_raise_as_a_failed_result() {
    // Raising priority (a negative niceness) needs CAP_SYS_NICE; the unprivileged answer is one
    // failed row with the permission error (errors.yaml E0302) and, per ADR-0006, exit status 1.
    let child = SleepChild::spawn();
    let before = child.niceness();
    let run = ono(&format!(
        "set process {} --priority -5 | to json",
        child.pid()
    ));
    run.assert_status(1);
    let results = json_rows(&run);
    assert_eq!(
        results.len(),
        1,
        "one target, one ActionResult, got {results:?}"
    );
    let result = &results[0];
    assert_eq!(
        text(result, "status"),
        "failed",
        "action-result.v1: a denied change is `failed`, got {result:?}"
    );
    assert!(
        error_names(result, "io.permission_denied", "E0302"),
        "errors.yaml: the failure is `io.permission_denied`, got {result:?}"
    );
    assert_eq!(
        child.niceness(),
        before,
        "a denied change leaves the process as it was"
    );
}

// --- send signal -----------------------------------------------------------------------------

#[test]
fn should_deliver_a_signal_to_the_process_arriving_through_the_pipeline() {
    // process.yaml: `get process 4419 | send signal SIGHUP` — the signal is the selector, the
    // process comes through the pipeline.
    let mut child = SleepChild::spawn();
    let run = ono(&format!(
        "get process {} | send signal SIGTERM | to json",
        child.pid()
    ));
    run.assert_success();
    let results = json_rows(&run);
    assert_eq!(
        results.len(),
        1,
        "one process in, one ActionResult out, got {results:?}"
    );
    let result = &results[0];
    assert_eq!(
        text(result, "status"),
        "success",
        "the signal was delivered, got {result:?}"
    );
    assert_eq!(
        child.signal_within(Duration::from_secs(5)),
        Some(15),
        "the child died of SIGTERM, which is what `send signal SIGTERM` promised"
    );
}

// --- contract behaviour: a failed ActionResult sets the exit status ---------------------------

#[test]
fn should_report_a_failed_result_and_exit_status_one_when_killing_a_pid_that_does_not_exist() {
    // Spec §16.5 + ADR-0006: the outcome is one ActionResult per target and the exit status is
    // derived from them. A target that does not exist is a failed row, not an empty stream.
    let run = ono("kill process 4000000 | to json");
    run.assert_status(1);
    let results = json_rows(&run);
    assert_eq!(
        results.len(),
        1,
        "spec §16.5: the missing target gets its own failed row, got {results:?}"
    );
    let result = &results[0];
    assert_eq!(
        text(result, "status"),
        "failed",
        "action-result.v1: the row is `failed`, got {result:?}"
    );
    assert!(
        error_names(result, "io.not_found", "E0301"),
        "errors.yaml: a pid nothing answers to is `io.not_found`, got {result:?}"
    );
    assert!(
        text(result, "target").contains("4000000"),
        "action-result.v1: the target names what was asked for, got {result:?}"
    );
}

#[test]
fn should_exit_with_status_one_when_stopping_a_process_is_denied() {
    // `stop process 1` as an unprivileged user: the row already says `failed` with
    // `io.permission_denied`; ADR-0006 requires the exit status to say so too.
    let run = ono("stop process 1 | to json");
    run.assert_status(1);
    let results = json_rows(&run);
    assert_eq!(
        results.len(),
        1,
        "one target, one ActionResult, got {results:?}"
    );
    let result = &results[0];
    assert_eq!(
        text(result, "status"),
        "failed",
        "stopping init is denied, got {result:?}"
    );
    assert!(
        error_names(result, "io.permission_denied", "E0302"),
        "errors.yaml: the failure is `io.permission_denied`, got {result:?}"
    );
}

#[test]
fn should_report_partial_failure_row_by_row_when_a_bulk_kill_mixes_outcomes() {
    // Spec §16.5: "The shell should never collapse `97 succeeded, 3 failed` into a single
    // ambiguous boolean." One row per target, and the aggregate exit status is 1 because one
    // of them failed (ADR-0006).
    let mut child = SleepChild::spawn();
    let run = ono(&format!(
        "get process | where pid == {} or pid == 1 | kill process | to json",
        child.pid()
    ));
    run.assert_status(1);
    let results = json_rows(&run);
    assert_eq!(
        results.len(),
        2,
        "spec §16.5: two targets, two rows, got {results:?}"
    );
    let statuses: Vec<String> = results.iter().map(|row| text(row, "status")).collect();
    assert!(
        statuses.contains(&"success".to_owned()) && statuses.contains(&"failed".to_owned()),
        "spec §16.5: the owned child was killed and init was not, got {results:?}"
    );
    let failed = results
        .iter()
        .find(|row| text(row, "status") == "failed")
        .expect("one row failed");
    assert!(
        error_names(failed, "io.permission_denied", "E0302"),
        "errors.yaml: killing init is `io.permission_denied`, got {failed:?}"
    );
    assert_eq!(
        child.signal_within(Duration::from_secs(5)),
        Some(9),
        "process.yaml: `kill process` defaults to SIGKILL and the child died of it"
    );
}

// --- a selector that names nothing is a refusal, whatever follows it (ADR-0221) --------------

#[test]
fn should_fail_the_run_when_a_named_process_is_not_there_and_a_count_follows() {
    // `get process 999999 | count` reported the failure on stderr, wrote `0` to stdout and
    // exited 0 — three answers that contradict each other. Spec §35.3: what could not be read is
    // not zero.
    let run = ono("get process 999999 | count");
    assert_eq!(
        run.status().code(),
        1,
        "a selector that names nothing did not answer, and the status says so: {:?}",
        run.output()
    );
    assert!(
        !run.stdout().contains('0'),
        "spec §35.3: a count of what could not be read is not `0`: {:?}",
        run.stdout()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0301"),
        "the refusal is still reported: {:?}",
        run.stderr()
    );
    assert_eq!(
        run.stderr().matches("Ono-Sendai-E0301").count(),
        1,
        "one failure is reported once: {:?}",
        run.stderr()
    );
}

#[test]
fn should_report_a_named_process_that_is_not_there_exactly_once() {
    let run = ono("get process 999999");
    assert_eq!(
        run.status().code(),
        1,
        "the run did not get what it asked for: {:?}",
        run.output()
    );
    assert_eq!(
        run.stderr().matches("Ono-Sendai-E0301").count(),
        1,
        "one failure is reported once, not twice: {:?}",
        run.stderr()
    );
}

#[test]
fn should_still_count_what_arrived_when_only_some_of_the_stream_failed() {
    // Spec §16.5: a partial failure is not a total one. Everything that could be read is still
    // counted, and the count is a positive number.
    let run = ono("get process | count");
    let counted: i64 = run
        .stdout()
        .lines()
        .last()
        .and_then(|line| line.trim().parse().ok())
        .unwrap_or_else(|| panic!("`count` answers a number, got {:?}", run.output()));
    assert!(
        counted > 1,
        "the processes that could be read are still counted: {counted}"
    );
}
