//! The identity family the contract declares and this build does not yet deliver: sessions
//! (spec §9.1, `docs/spec/commands/identity.yaml` `ono.session.get`), account management
//! (`add`/`remove`/`set` on `user` and `group`, spec §52), the live streams `watch user` and
//! `watch group` (spec §18.2, ADR-0024, ADR-0034), `trace user` (spec §22.3) and the context
//! frames `enter user` and `enter group` (spec §14.3, ADR-0023).
//!
//! Every test is unprivileged and offline. Mutations therefore cannot succeed; what they must
//! do is *try* and report one `ono.action-result/1` row with a structured error, exiting 1
//! (spec §16.5, ADR-0006) — never answer "declared but not implemented".
//!
//! `set env` (`ono.env.set`) is not tested here: `get command | where id == "ono.env.set"` and
//! `explain set env X 1` already name the registry entry, so nothing is missing at that level.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use ono_testkit::Shell;
use serde_yaml_ng::Value;

fn ono(script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .timeout(Duration::from_secs(30))
        .run()
}

/// Parses one line of `to json` output. JSON is YAML, so the workspace's YAML parser reads it.
fn json(text: &str) -> Value {
    serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!("`to json` emits a JSON document (spec §33.5): {error}\n{text}")
    })
}

/// The last non-empty stdout line as a JSON document — a script's final `| to json`.
fn last_json(run: &ono_testkit::Run) -> Value {
    let line = run
        .stdout()
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .unwrap_or_else(|| panic!("a `to json` document on stdout, got {:?}", run.output()));
    json(line)
}

fn items(value: &Value) -> &[Value] {
    value
        .as_sequence()
        .unwrap_or_else(|| {
            panic!("`to json` emits an array of the stream's values (spec §33.5), got {value:?}")
        })
        .as_slice()
}

/// The uid this test runs as, from the kernel rather than an environment variable.
fn my_uid() -> u32 {
    std::fs::metadata("/proc/self")
        .expect("/proc/self is readable")
        .uid()
}

/// These tests describe what an unprivileged user observes; as root they would really mutate
/// the account database, which no test may do.
fn require_unprivileged() {
    assert_ne!(
        my_uid(),
        0,
        "identity_missing tests must run unprivileged (BRIEF: unprivileged, offline)"
    );
}

/// What "not implemented" looks like today, so a test can insist on the opposite.
fn assert_not_unimplemented(run: &ono_testkit::Run) {
    let stderr = run.stderr();
    assert!(
        !stderr.contains("Ono-Sendai-E0101")
            && !stderr.contains("implements nothing")
            && !stderr.contains("not delivered")
            && !stderr.contains("has no target")
            && !stderr.contains("cannot be a pipeline stage"),
        "the command is delivered as the provider command the contract declares, not merely declared or swallowed by a builtin (spec §52), got {stderr:?}"
    );
}

/// A mutation's row for an unprivileged attempt: `failed`, unchanged, with a permission or
/// policy error (`io.permission_denied` E0302 or `safety.policy_denied` E0702) — spec §16.5,
/// `docs/spec/schemas/action-result.v1.yaml`.
fn assert_failed_for_lack_of_privilege(run: &ono_testkit::Run, operation: &str) {
    assert_not_unimplemented(run);
    let rows = last_json(run);
    let rows = items(&rows);
    assert_eq!(
        rows.len(),
        1,
        "one ono.action-result/1 row per target (spec §16.5), got {rows:?}"
    );
    let row = &rows[0];
    assert_eq!(
        row["status"].as_str(),
        Some("failed"),
        "the unprivileged attempt is reported as `failed`, got {row:?}"
    );
    assert_eq!(
        row["changed"].as_bool(),
        Some(false),
        "a failed mutation changed nothing, got {row:?}"
    );
    let op = row["operation"].as_str().unwrap_or_default();
    assert!(
        op.ends_with(operation),
        "`operation` names the command (`{operation}`), got {op:?}"
    );
    let error = format!("{:?}", row["error"]);
    assert!(
        error.contains("permission_denied")
            || error.contains("E0302")
            || error.contains("policy_denied")
            || error.contains("E0702"),
        "the error is the structured permission (E0302) or policy (E0702) refusal, not a bare message — got {error}"
    );
    // Any failed row makes the exit status 1 (ADR-0006).
    run.assert_status(1);
}

// --- get session ------------------------------------------------------------------------------

#[test]
fn should_enumerate_sessions_as_a_list_when_asked() {
    // Spec §9.1: `get session` enumerates local/login/session objects as `Stream<Session>`. An
    // unprivileged container may have none — the shape is asserted only for what is there, but
    // the command itself answers, it does not report a missing provider.
    let run = ono("get session | to json");
    assert!(
        !run.stderr().contains("no provider answers"),
        "a session provider exists (identity.yaml `ono.session.get`), got {:?}",
        run.stderr()
    );
    run.assert_success();
    let sessions = last_json(&run);
    for session in items(&sessions) {
        assert!(
            session.is_mapping(),
            "each session is a record (spec §9.1), got {session:?}"
        );
        assert!(
            !session["id"].is_null(),
            "a session carries its identity so it can be named again, got {session:?}"
        );
        assert!(
            session["user"].is_mapping() && session["user"]["uid"].as_i64().is_some(),
            "a session belongs to a user, referenced by uid (ref<ono.user/1>, spec §23.6), got {session:?}"
        );
    }
}

#[test]
fn should_restrict_sessions_to_one_user_when_the_user_option_is_given() {
    // identity.yaml `ono.session.get` option `--user ref<ono.user/1>`: only that user's sessions.
    let run = ono("get session --user root | to json");
    assert!(
        !run.stderr().contains("no provider answers") && !run.stderr().contains("no option"),
        "`get session --user` is contracted, got {:?}",
        run.stderr()
    );
    run.assert_success();
    let sessions = last_json(&run);
    for session in items(&sessions) {
        assert_eq!(
            session["user"]["uid"].as_i64(),
            Some(0),
            "`--user root` restricts the stream to uid 0, got {session:?}"
        );
    }
}

// --- add / remove / set user ---------------------------------------------------------------

#[test]
fn should_report_a_failed_action_result_when_adding_a_user_unprivileged() {
    require_unprivileged();
    let run = ono("add user testuser-ono --shell /bin/false | to json");
    assert_failed_for_lack_of_privilege(&run, "add");
}

#[test]
fn should_not_create_the_account_when_adding_a_user_fails() {
    require_unprivileged();
    // The attempt is honest about its outcome: nothing appears in the account database.
    let run = ono("add user testuser-ono --uid 4242 | to json; get user testuser-ono | to json");
    assert_not_unimplemented(&run);
    let lines: Vec<&str> = run.stdout().lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "the action result and the lookup each produce a document, got {:?}",
        run.output()
    );
    assert_eq!(
        json(lines[1]),
        Value::Sequence(vec![]),
        "a failed `add user` leaves no account behind (spec §16.5)"
    );
}

#[test]
fn should_report_a_failed_action_result_when_removing_a_user_unprivileged() {
    require_unprivileged();
    let run = ono("remove user root | to json");
    assert_failed_for_lack_of_privilege(&run, "remove");
}

#[test]
fn should_report_a_structured_not_found_when_removing_a_user_that_does_not_exist() {
    require_unprivileged();
    // A selector nothing answers to is a resolution failure (E0102) or, per target, `io.not_found`
    // (E0301) — structured either way, and never a success with an empty stream.
    let run = ono("remove user nobody-such-user-ono | to json");
    assert_not_unimplemented(&run);
    assert!(
        !run.status().is_success(),
        "removing an account that does not exist is not a success, got {:?}",
        run.output()
    );
    let whole = run.output();
    assert!(
        whole.contains("E0102") || whole.contains("E0301") || whole.contains("not_found"),
        "the refusal is the structured not-found error (spec §43), got {whole:?}"
    );
    assert!(
        whole.contains("nobody-such-user-ono"),
        "the refusal names the account it could not find, got {whole:?}"
    );
}

#[test]
fn should_report_a_failed_action_result_when_setting_a_users_shell_unprivileged() {
    require_unprivileged();
    let run = ono("set user root --shell /bin/false | to json");
    assert_failed_for_lack_of_privilege(&run, "set");
}

#[test]
fn should_leave_the_account_unchanged_when_setting_a_user_fails() {
    require_unprivileged();
    let before = last_json(&ono("get user root | select shell | to json"));
    let run =
        ono("set user root --shell /bin/false | to json; get user root | select shell | to json");
    assert_not_unimplemented(&run);
    let after = last_json(&run);
    assert_eq!(
        after, before,
        "a failed `set user` changes nothing (action-result `changed: false` is the truth)"
    );
}

// --- add / remove / set group --------------------------------------------------------------

#[test]
fn should_report_a_failed_action_result_when_adding_a_group_unprivileged() {
    require_unprivileged();
    let run = ono("add group testgroup-ono | to json");
    assert_failed_for_lack_of_privilege(&run, "add");
}

#[test]
fn should_report_a_failed_action_result_when_adding_a_member_to_a_group_unprivileged() {
    require_unprivileged();
    // identity.yaml `ono.group.add --member`: extends an existing group instead of creating one.
    // The member is this test's own account, which is certainly not in `root` already.
    let me = last_json(&ono(&format!(
        "get user | where uid == {} | select name | to json",
        my_uid()
    )));
    let me = items(&me)[0]["name"]
        .as_str()
        .expect("the running user resolves to a login name")
        .to_owned();
    let run = ono(&format!("add group root --member {me} | to json"));
    assert_failed_for_lack_of_privilege(&run, "add");
}

#[test]
fn should_report_a_failed_action_result_when_removing_a_group_unprivileged() {
    require_unprivileged();
    let run = ono("remove group root | to json");
    assert_failed_for_lack_of_privilege(&run, "remove");
}

#[test]
fn should_report_a_failed_action_result_when_setting_a_group_id_unprivileged() {
    require_unprivileged();
    let run = ono("set group root --gid 999 | to json");
    assert_failed_for_lack_of_privilege(&run, "set");
}

#[test]
fn should_leave_the_group_unchanged_when_setting_a_group_fails() {
    require_unprivileged();
    let run = ono("set group root --gid 999 | to json; get group root | select gid | to json");
    assert_not_unimplemented(&run);
    let after = last_json(&run);
    assert_eq!(
        items(&after)[0]["gid"].as_i64(),
        Some(0),
        "a failed `set group` leaves gid 0 where it was"
    );
}

// --- watch user / watch group --------------------------------------------------------------

#[test]
fn should_begin_a_user_watch_with_a_snapshot_when_bounded() {
    // ADR-0024: a subscription always begins with a snapshot; spec §18.3: bounded with `take`
    // and serialised, the stream is an ordinary document. identity.yaml declares no `--every`
    // for `watch user`, so the default cadence of ADR-0034 applies — the snapshot is immediate.
    let run = ono("watch user | take 1 | select kind | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"}]"#,
        "the first event of `watch user` is the current state (ADR-0024)"
    );
}

#[test]
fn should_say_how_a_user_watch_observes_changes() {
    // Spec §18.2: polling is explicit in metadata. Every event says whether it came from a
    // provider subscription or from the runtime comparing snapshots (ADR-0034).
    let run = ono("watch user | take 1 | select source | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    let events = last_json(&run);
    let source = items(&events)[0]["source"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    assert!(
        source == "poll" || source == "subscription",
        "`source` is `poll` or `subscription` (spec §18.2), got {source:?}"
    );
}

#[test]
fn should_begin_a_group_watch_with_a_snapshot_when_bounded() {
    let run = ono("watch group | take 1 | select kind | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"kind":"snapshot"}]"#,
        "the first event of `watch group` is the current state (ADR-0024)"
    );
}

// --- trace user -----------------------------------------------------------------------------

#[test]
fn should_trace_a_user_to_its_processes_and_groups() {
    // identity.yaml `ono.user.trace`: a user's processes, sessions, groups and owned files as
    // one `ono.graph/1`. Pid 1 is owned by root on every Linux system and root's primary group
    // is gid 0, so both must be nodes of `trace user root`.
    let run = ono("trace user root | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    let graphs = last_json(&run);
    let graphs = items(&graphs);
    assert_eq!(
        graphs.len(),
        1,
        "`trace` yields one Graph (spec §9.1), got {graphs:?}"
    );
    let graph = &graphs[0];
    assert_eq!(
        graph["root"]["schema"].as_str(),
        Some("ono.user/1"),
        "the graph's root is the traced user (graph.v1.yaml `root`), got {:?}",
        graph["root"]
    );
    assert_eq!(
        graph["root"]["identity"]["uid"].as_i64(),
        Some(0),
        "the root's identity is the uid (user.v1.yaml `identity: [uid]`), got {:?}",
        graph["root"]
    );
    let nodes = items(&graph["nodes"]);
    assert!(
        nodes
            .iter()
            .any(|node| node["kind"].as_str() == Some("ono.process/1")
                && node["value"]["pid"].as_i64() == Some(1)),
        "pid 1 (owned by root) is among the traced processes (spec §22.3), got {} nodes",
        nodes.len()
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["kind"].as_str() == Some("ono.group/1")
                && node["value"]["gid"].as_i64() == Some(0)),
        "root's primary group (gid 0) is among the traced groups, got {} nodes",
        nodes.len()
    );
    assert!(
        !items(&graph["edges"]).is_empty(),
        "the relationships are edges, not implied by node order (spec §22.1)"
    );
}

#[test]
fn should_refuse_to_trace_a_user_that_does_not_exist() {
    let run = ono("trace user nobody-such-user-ono | to json");
    assert_not_unimplemented(&run);
    assert!(
        !run.status().is_success(),
        "tracing nothing is a failure, got {:?}",
        run.output()
    );
    let stderr = run.stderr();
    assert!(
        stderr.contains("Ono-Sendai-E0102") || stderr.contains("Ono-Sendai-E0301"),
        "the refusal is the structured not-found error (spec §43), got {stderr:?}"
    );
    assert!(
        stderr.contains("nobody-such-user-ono"),
        "the refusal names the account it could not find, got {stderr:?}"
    );
}

// --- enter user / enter group --------------------------------------------------------------

#[test]
fn should_enter_a_user_and_show_it_on_the_context_stack() {
    // Spec §14.3 and context.v1.yaml: an entered object is a frame of kind `object` naming its
    // target and identity, with the explicit spelling it stands for (ADR-0023).
    let run = ono("enter user root; get context | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    let frames = last_json(&run);
    let frames = items(&frames);
    assert_eq!(
        frames.len(),
        2,
        "the ground frame plus the user frame (spec §14.1), got {frames:?}"
    );
    let top = &frames[1];
    assert_eq!(
        top["kind"].as_str(),
        Some("object"),
        "an entered user is an object frame (context.v1.yaml `kind`), got {top:?}"
    );
    assert_eq!(
        top["target"].as_str(),
        Some("user"),
        "the frame narrows to the `user` target, got {top:?}"
    );
    let identity = top["identity"].as_str().unwrap_or_default();
    assert!(
        identity.contains("root") || identity.contains('0'),
        "the frame carries the user's identity, got {top:?}"
    );
    assert!(
        top["selector"]
            .as_str()
            .unwrap_or_default()
            .contains("--user"),
        "the frame's contribution is spelled out as `--user …` (spec §14.5, ADR-0023), got {top:?}"
    );
}

#[test]
fn should_narrow_processes_to_the_entered_user() {
    // Spec §14.3: the frame is an implicit selector — `get process` inside `enter user root` is
    // `get process --user root`, so every process it returns belongs to uid 0 and pid 1 is one.
    let run = ono("enter user root; get process | select pid user | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    let processes = last_json(&run);
    let processes = items(&processes);
    assert!(
        processes
            .iter()
            .any(|process| process["pid"].as_i64() == Some(1)),
        "root's processes include pid 1, got {} processes",
        processes.len()
    );
    for process in processes {
        assert_eq!(
            process["user"]["uid"].as_i64(),
            Some(0),
            "inside the user frame every process belongs to uid 0 (spec §14.3), got {process:?}"
        );
    }
}

#[test]
fn should_pop_the_user_frame_when_leaving() {
    let run = ono(
        "enter user root; get context | select depth | to json; leave; get context | select depth | to json",
    );
    assert_not_unimplemented(&run);
    run.assert_success();
    let lines: Vec<&str> = run
        .stdout()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    assert_eq!(lines.len(), 2, "two stack listings, got {:?}", run.output());
    assert_eq!(
        items(&json(lines[0])).len(),
        2,
        "the user frame is on the stack after `enter user root`"
    );
    assert_eq!(
        items(&json(lines[1])).len(),
        1,
        "`leave` pops the user frame and only the ground remains (ADR-0023)"
    );
}

#[test]
fn should_enter_a_group_and_show_it_on_the_context_stack() {
    let run = ono("enter group root; get context | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    let frames = last_json(&run);
    let frames = items(&frames);
    assert_eq!(
        frames.len(),
        2,
        "the ground frame plus the group frame (spec §14.1), got {frames:?}"
    );
    let top = &frames[1];
    assert_eq!(
        top["kind"].as_str(),
        Some("object"),
        "an entered group is an object frame (context.v1.yaml `kind`), got {top:?}"
    );
    assert_eq!(
        top["target"].as_str(),
        Some("group"),
        "the frame narrows to the `group` target, got {top:?}"
    );
    let identity = top["identity"].as_str().unwrap_or_default();
    assert!(
        identity.contains("root") || identity.contains('0'),
        "the frame carries the group's identity (gid 0), got {top:?}"
    );
}

#[test]
fn should_narrow_processes_to_the_entered_group() {
    // process.v1.yaml `group`: the effective group. Inside `enter group root` every process has
    // gid 0, and pid 1 is among them.
    let run = ono("enter group root; get process | select pid group | to json");
    assert_not_unimplemented(&run);
    run.assert_success();
    let processes = last_json(&run);
    let processes = items(&processes);
    assert!(
        processes
            .iter()
            .any(|process| process["pid"].as_i64() == Some(1)),
        "gid 0's processes include pid 1, got {} processes",
        processes.len()
    );
    for process in processes {
        assert_eq!(
            process["group"]["gid"].as_i64(),
            Some(0),
            "inside the group frame every process runs as gid 0 (spec §14.3), got {process:?}"
        );
    }
}

#[test]
fn should_refuse_to_enter_a_user_that_does_not_exist() {
    let run = ono("enter user nobody-such-user-ono");
    assert_not_unimplemented(&run);
    assert!(
        !run.status().is_success(),
        "entering an account nothing answers to fails, got {:?}",
        run.output()
    );
    let stderr = run.stderr();
    assert!(
        stderr.contains("Ono-Sendai-E1001") && stderr.contains("nobody-such-user-ono"),
        "the refusal is structured and names the account, and a failed `enter` is \
         `spatial.not_found` in either grammar (spec §43, ADR-0023, ADR-0191), got {stderr:?}"
    );
}

#[test]
fn should_keep_the_stack_unchanged_when_entering_a_user_fails() {
    let run = ono("enter user nobody-such-user-ono; get context | to json");
    assert!(
        run.stderr().contains("nobody-such-user-ono"),
        "the refusal names the account rather than the command's phase, got {:?}",
        run.stderr()
    );
    let frames = last_json(&run);
    assert_eq!(
        items(&frames).len(),
        1,
        "a refused `enter` pushes nothing — only the ground frame remains (ADR-0023)"
    );
}
