//! What the shell refuses when it would have to hold too much (v0.4.1 §21–§24, §54.3).
//!
//! §65.6 names the defect these tests exist against: *"Allowing `N` values while each may contain
//! arbitrarily large payloads, with no byte budget, is forbidden for retained/materialized
//! collections."* Every proof here is written from outside the process — a shell run with a
//! configured ceiling, and the refusal or the figure a user sees — because the claim is about
//! what the product does, not about which function counts (AGENTS.md §11).
//!
//! The ceilings are set through `limits.*` rather than reached with real data: Appendix A's
//! defaults are 100 000 values and 128 MiB, and a test that allocated 128 MiB to prove a byte
//! ceiling would be a test nobody runs. That the *defaults* are Appendix A's is asserted
//! separately, in `meta_config.rs` and against the contract registry below.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

use support::isolated;

/// The three refusal codes of v0.4.1 §21.4, as a user reads them on stderr.
const ITEM_LIMIT: &str = "Ono-Sendai-E1101";
const BYTE_LIMIT: &str = "Ono-Sendai-E1102";
const UNBOUNDED: &str = "Ono-Sendai-E0801";

/// Runs `script` in a shell that sees nothing of this machine's configuration.
fn run(dir: &Scratch, script: &str) -> ono_testkit::Run {
    shell(dir).args(["-c", script]).run()
}

/// A shell isolated from the developer's environment, with every `limits.*` override removed.
fn shell(dir: &Scratch) -> Shell {
    let mut shell = isolated(dir);
    for key in LIMIT_KEYS {
        shell = shell.env_remove(format!(
            "ONO_{}",
            key.to_ascii_uppercase().replace('.', "_")
        ));
    }
    shell
}

/// Every key `docs/spec/hardening/limits.yaml` declares, read from the registry itself.
///
/// A test that listed them would be the second copy §52.2 forbids; this reads the contract.
fn registry() -> Vec<Value> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/spec/hardening/limits.yaml"),
    )
    .expect("docs/spec/hardening/limits.yaml is the registry of v0.4.1 §52.1");
    let document: Value = serde_yaml_ng::from_str(&text).expect("the registry is YAML");
    document["limits"]
        .as_sequence()
        .expect("the registry declares a `limits` sequence")
        .clone()
}

const LIMIT_KEYS: [&str; 13] = [
    "limits.materialize_items",
    "limits.materialize_bytes",
    "limits.command_capture_bytes",
    "limits.history_results",
    "limits.history_items_per_result",
    "limits.history_bytes_per_result",
    "limits.history_bytes_total",
    "limits.completion_soft_ms",
    "limits.completion_hard_ms",
    "limits.remote_connections",
    "limits.remote_pending_handshakes",
    "limits.remote_connections_per_client",
    "limits.remote_handshake_timeout_ms",
];

// --- §22: every global collection goes through the budget (issue #67) -------------------------

#[test]
fn should_route_every_global_collection_through_the_budget_aware_helper() {
    // Appendix E's global classes, as a user spells them. §6.2 puts the enforcement in the
    // materialization primitive rather than in each caller, and the observable form of that is
    // that *every* one of them refuses under the same configured ceiling, with the same code.
    let dir = scratch();
    // `sort` and `group` hold their upstream; `diff` holds the earlier snapshot it was given, so
    // its proof needs a snapshot with something in it (ADR-0454).
    let collections = [
        "get process | sort pid | count | to json",
        "get process | group name | count | to json",
        "get process | take 4 | select pid\nget process | take 4 | diff @-1 | count | to json",
    ];
    for collection in collections {
        let run = run(
            &dir,
            &format!("set config limits.materialize_items = 2\n{collection}"),
        );
        assert!(
            run.stderr().contains(ITEM_LIMIT),
            "`{collection}` collected past its 2-value ceiling without refusing; §65.6 calls a \
             retained collection with no enforced bound a limit that is not one. stderr: {:?}",
            run.stderr()
        );
        assert!(
            run.stderr().contains("limits.materialize_items"),
            "§54.1: the refusal names the limit the user would raise: {:?}",
            run.stderr()
        );
    }
}

#[test]
fn should_refuse_on_the_byte_ceiling_before_the_item_ceiling_when_values_are_large() {
    // §60.5's shape: the item count stays far below its own ceiling and the byte ceiling is what
    // stops it, which is the whole point of §2.4 having two bounds rather than one.
    let dir = scratch();
    let run = run(
        &dir,
        "set config limits.materialize_items = 100000\n\
         set config limits.materialize_bytes = 4096\n\
         get process | sort pid | count | to json",
    );
    assert!(
        run.stderr().contains(BYTE_LIMIT),
        "a 4 KiB materialization budget must stop `sort` on bytes, not on values: {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("limits.materialize_bytes"),
        "the refusal names the byte limit rather than the item one: {:?}",
        run.stderr()
    );
}

#[test]
fn should_answer_the_same_resource_code_for_the_same_refusal_every_time() {
    // §53.2: automation matches on the code, so the code has to be a function of the condition
    // and of nothing else — not of the load, the ordering or the run.
    let dir = scratch();
    let script =
        "set config limits.materialize_items = 1\nget process | sort pid | count | to json";
    let codes: Vec<String> = (0..5)
        .map(|_| {
            let run = run(&dir, script);
            run.stderr()
                .lines()
                .find_map(|line| {
                    line.split_whitespace()
                        .find(|word| word.starts_with("Ono-Sendai-"))
                        .map(str::to_owned)
                })
                .unwrap_or_else(|| panic!("the run must refuse: {:?}", run.stderr()))
        })
        .collect();
    assert_eq!(
        codes,
        vec![ITEM_LIMIT.to_owned(); 5],
        "§53.2: five identical refusals answer one code"
    );
}

#[test]
fn should_carry_the_limit_and_the_consumption_but_no_payload_in_a_resource_refusal() {
    // §21.4: "Errors SHOULD include the configured limit and observed/estimated consumption
    // without dumping the retained values themselves." A resource error that printed what it was
    // holding would be a second resource problem.
    let dir = scratch();
    let run = run(
        &dir,
        "set config limits.materialize_items = 2\n\
         try { get process | sort pid | count } catch e { $e | select name message | to json }",
    );
    let caught = run.stdout();
    assert!(
        caught.contains("resource.item_limit"),
        "the structured value carries the dotted selector automation matches on (§53.2): \
         {caught:?} / {:?}",
        run.stderr()
    );
    assert!(
        caught.contains('2'),
        "§21.4: the refusal states the configured limit: {caught:?}"
    );
    for leaked in ["\"pid\"", "\"command\"", "Record", "provenance"] {
        assert!(
            !caught.contains(leaked),
            "§21.4: the refusal dumped {leaked} from the values it was holding: {caught:?}"
        );
    }
}

// --- §22.3: an operation that needs finite input refuses at once (issue #68) -------------------

#[test]
fn should_name_the_finiteness_requirement_and_the_declaring_stage_in_the_refusal() {
    // §54.1's own example sentence: "sort requires finite input; upstream is declared unbounded".
    // The source is the shell's own `tail --follow`, which never ends, and the file it follows is
    // never written to — so a refusal that arrived only when the source ended would never arrive
    // at all, and the test would hang rather than pass (ADR-0431's discipline).
    let dir = scratch();
    let path = dir.write("waiting/source.log", "one\n");
    let run = run(
        &dir,
        &format!(
            "tail file {} --lines 1 --follow | sort name | to json",
            path.display()
        ),
    );
    let refusal = run.stderr();
    assert!(
        refusal.contains(UNBOUNDED),
        "§22.3: a finite-required stage refuses a declared-unbounded upstream: {refusal:?}"
    );
    assert!(
        refusal.contains("sort") && refusal.contains("finite input"),
        "§54.1: the refusal names the stage and the requirement: {refusal:?}"
    );
    assert!(
        refusal.contains("unbounded"),
        "§54.1: the refusal says what the upstream declared itself to be: {refusal:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the source file"),
        "one\n",
        "the refusal arrived while the source was still waiting, which is what §22.3's \
         'MUST NOT wait forever' means when written as something a test can read"
    );
}

// --- §23: capture buffers share one command ceiling (issue #70) --------------------------------

/// A shell whose captures may retain `bytes` in total, running `script`.
///
/// The ceiling is set in the configuration file rather than by a `set config` statement, so
/// `script` is one statement and its exit status is the statement's own — §23.4's ceiling is per
/// command, and a two-statement script would report the second one's status.
fn with_capture_ceiling(dir: &Scratch, bytes: u64, script: &str) -> ono_testkit::Run {
    dir.write(
        "ono/config.ono",
        format!("set config limits.command_capture_bytes = {bytes}\n"),
    );
    run(dir, script)
}

#[test]
fn should_charge_a_nested_command_capture_against_the_shared_budget() {
    // §23.1: "Any evaluator mechanism that captures pipeline output for later use MUST use the
    // shared materialization budget." A command substitution is that mechanism, and a ceiling of
    // 64 bytes is smaller than one process record.
    let dir = scratch();
    let refused = with_capture_ceiling(&dir, 64, "let x = (get process | take 4)\n$x | count");
    assert!(
        refused.stderr().contains(BYTE_LIMIT),
        "a command substitution retained values without charging the capture budget: {:?}",
        refused.stderr()
    );
    assert!(
        refused.stderr().contains("limits.command_capture_bytes"),
        "the refusal names the ceiling the user would raise, and not a different one: {:?}",
        refused.stderr()
    );

    // The same capture under the documented default is ordinary work, not a refusal. A fresh
    // configuration home, because the ceiling above lives in a file.
    let untouched = scratch();
    let allowed = run(
        &untouched,
        "let x = (get process | take 4)\n$x | count | to json",
    );
    allowed.assert_success();
    assert!(
        !allowed.stderr().contains(BYTE_LIMIT),
        "the Appendix A default holds four process records comfortably: {:?}",
        allowed.stderr()
    );
}

#[test]
fn should_accumulate_nested_captures_against_the_one_per_command_ceiling() {
    // §23.4: "Nested captures MUST not each independently consume the full global allowance."
    // The differential: the same values captured once, and captured twice by nesting a function
    // body inside a command substitution. A budget that reset per capture would admit both.
    let dir = scratch();
    let ceiling = one_capture_ceiling(&dir);

    let once = with_capture_ceiling(&dir, ceiling, ONE_CAPTURE);
    once.assert_success();
    assert!(!once.stderr().contains(BYTE_LIMIT));

    // The same four records, captured twice: once by the function body that is consumed by a
    // later stage, and once by the substitution that binds the result. A ceiling that holds one
    // capture cannot hold two, because `ceiling` is the smallest power of two that held one — so
    // one capture costs more than half of it, and two cost more than all of it.
    let twice = with_capture_ceiling(&dir, ceiling, NESTED_CAPTURES);
    assert!(
        twice.stderr().contains(BYTE_LIMIT),
        "two nested captures of the same values fitted a ceiling that holds one; §23.4 calls \
         that each capture independently consuming the full allowance. stderr: {:?}",
        twice.stderr()
    );
}

/// One capture of four process records, bound to a name.
const ONE_CAPTURE: &str = "let x = (get process | take 4 | where pid > 0)";

/// The same four records held by two captures at once: a function body consumed by a later
/// stage, inside the substitution that binds what the stage produced.
const NESTED_CAPTURES: &str = "fn four() { get process | take 4 }\nlet x = (four | where pid > 0)";

#[test]
fn should_refuse_a_capture_that_would_exceed_the_command_ceiling() {
    // §21.3: the operation stops with a structured resource error rather than collecting on
    // while warning. The proof that it stopped is the exit status and the absent result.
    let dir = scratch();
    let refused = with_capture_ceiling(&dir, 64, "let x = (get process | take 8)");
    assert!(
        !refused.status().is_success(),
        "a refused capture fails the command: {:?}",
        refused.stderr()
    );
    assert!(
        refused.stderr().contains(BYTE_LIMIT),
        "§21.3: the refusal is the structured resource error, not a warning: {:?}",
        refused.stderr()
    );
    assert!(
        !refused.stdout().contains("\"pid\""),
        "§21.3: nothing of the refused capture is shown as though it had been collected: {:?}",
        refused.stdout()
    );
}

/// The smallest capture ceiling under which one capture of four process records still fits.
///
/// Measured rather than assumed: a process record's estimated size depends on the command line
/// of whatever is running on the machine, so a literal here would be a number that is right on
/// one host and wrong on the next.
fn one_capture_ceiling(dir: &Scratch) -> u64 {
    let mut ceiling = 4096;
    while ceiling < 1 << 24 {
        let run = with_capture_ceiling(dir, ceiling, ONE_CAPTURE);
        if run.status().is_success() && !run.stderr().contains(BYTE_LIMIT) {
            return ceiling;
        }
        ceiling *= 2;
    }
    panic!("no ceiling under 16 MiB held four process records");
}

// --- §54.3: the effective limits are inspectable (issue #120) ----------------------------------

#[test]
fn should_answer_the_effective_non_secret_limits_when_inspect_limits_runs() {
    let dir = scratch();
    let run = run(&dir, "inspect limits | to json");
    run.assert_success();
    let rows = support::rows(&run);

    for key in LIMIT_KEYS {
        let row = rows
            .iter()
            .find(|row| row["key"].as_str() == Some(key))
            .unwrap_or_else(|| panic!("§54.3: `inspect limits` omits `{key}`: {rows:?}"));
        assert!(
            !row["value"].is_null(),
            "every limit reports the figure in force: {row:?}"
        );
        assert_eq!(
            row["layer"].as_str(),
            Some("default"),
            "an isolated shell is at Appendix A's defaults: {row:?}"
        );
    }

    // §53.3: "Error detail fields MAY include limits, fingerprints and capability IDs. They MUST
    // avoid secrets." A diagnostic that answers ceilings answers ceilings and nothing else, so
    // every row is a `limits.*` key and nothing in the output carries key material.
    for row in &rows {
        let key = row["key"].as_str().expect("a row names its key");
        assert!(
            key.starts_with("limits."),
            "`inspect limits` answered `{key}`, which is not a limit: {row:?}"
        );
    }
    let printed = run.stdout();
    for material in ["sha256:", "-----BEGIN", "PRIVATE KEY", "Bearer "] {
        assert!(
            !printed.contains(material),
            "`inspect limits` printed {material:?}, which is not a limit: {printed:?}"
        );
    }
}

#[test]
fn should_answer_the_same_figures_inspect_limits_shows_from_the_contract_registry() {
    // §52.2: "A number such as `max_connections = 32` MUST not be independently typed into five
    // files if one contract can generate the others." So the shell and the registry are compared
    // in both directions, and neither is allowed to hold a key the other has not heard of.
    let dir = scratch();
    let run = run(&dir, "inspect limits | to json");
    run.assert_success();
    let rows = support::rows(&run);
    let declared = registry();

    for entry in &declared {
        let key = entry["key"].as_str().expect("a registry row names its key");
        let row = rows
            .iter()
            .find(|row| row["key"].as_str() == Some(key))
            .unwrap_or_else(|| {
                panic!("the registry declares `{key}` and the shell has never heard of it")
            });
        let shown = row["bytes"]
            .as_u64()
            .or_else(|| row["value"].as_u64())
            .unwrap_or_else(|| panic!("`{key}` reports no figure: {row:?}"));
        assert_eq!(
            Some(shown),
            entry["default"].as_u64(),
            "`{key}` defaults to {shown} in the shell and to {:?} in \
             docs/spec/hardening/limits.yaml; Appendix A fixes one number",
            entry["default"]
        );
        assert_eq!(
            row["min"]["bytes"].as_u64().or_else(|| row["min"].as_u64()),
            entry["min"].as_u64(),
            "the permitted range of `{key}` disagrees with the registry: {row:?}"
        );
        assert_eq!(
            row["max"]["bytes"].as_u64().or_else(|| row["max"].as_u64()),
            entry["max"].as_u64(),
            "the permitted range of `{key}` disagrees with the registry: {row:?}"
        );
    }

    for row in &rows {
        let key = row["key"].as_str().expect("a row names its key");
        assert!(
            declared
                .iter()
                .any(|entry| entry["key"].as_str() == Some(key)),
            "the shell enforces `{key}` and docs/spec/hardening/limits.yaml does not declare it; \
             §52.2 wants one home per number"
        );
    }
}

// --- §24: the retained history is bounded, and truthful about it (issue #72) -------------------

#[test]
fn should_leave_the_pipeline_output_complete_when_history_could_not_keep_it_all() {
    // §60.6: "A pipeline producing more than history limits MUST still emit its complete result
    // to the user/downstream. Only retained history is truncated/evicted." The proof is the
    // difference between the two figures in one run: fifty values emitted, four retained.
    let dir = scratch();
    dir.write(
        "ono/config.ono",
        "set config limits.history_items_per_result = 4\n",
    );
    let run = run(
        &dir,
        "get process | take 50 | select pid\n@-1 | count | to json",
    );
    run.assert_success();

    // §24.3, §54.1: the notice carries both figures, so one sentence is the proof — fifty values
    // were emitted to the user, and four are what history kept of them.
    let notice = run.stderr();
    assert!(
        notice.contains("result history kept 4 of 50 values"),
        "§54.1, §60.6: the notice says how much of how much was retained: {notice:?}"
    );
    assert!(
        notice.contains("the command's own output was complete"),
        "§24.3: and does not present the retained subset as the whole output: {notice:?}"
    );
    assert_eq!(
        support::last_line(&run),
        "[4]",
        "§24.2: what `@-1` answers is the retained subset, and only that: {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().lines().count() > 40,
        "§60.6: the user's own fifty rows were all shown: {:?}",
        run.stdout()
    );
}

#[test]
fn should_stop_retaining_a_result_at_its_configured_byte_ceiling() {
    // §24.1 adds byte ceilings beside the count ones. A ceiling of 512 bytes holds nothing like
    // a process record, so retention stops at once while the pipeline is unaffected.
    let dir = scratch();
    dir.write(
        "ono/config.ono",
        "set config limits.history_bytes_per_result = 512\n",
    );
    let run = run(
        &dir,
        "get process | take 20 | select pid\n@-1 | count | to json",
    );
    run.assert_success();

    assert!(
        run.stderr().contains("of 20 values"),
        "§24.3: a byte-truncated retention says so too, not only an item-truncated one, and \
         names what the pipeline really produced: {:?}",
        run.stderr()
    );
    assert!(
        !run.stderr().contains(BYTE_LIMIT),
        "§21.3, §24.2: history evicts, it does not raise a resource error at the user: {:?}",
        run.stderr()
    );
}

#[test]
fn should_evict_the_oldest_retained_result_when_the_total_history_ceiling_is_reached() {
    // §24.2 rule 4, from outside: with two slots and three results, `@-3` is gone and `@-1` is
    // the newest. History evicting is never an error, so the run succeeds throughout.
    let dir = scratch();
    dir.write("ono/config.ono", "set config limits.history_results = 2\n");
    let run = run(
        &dir,
        "get process | take 1 | select pid\n\
         get process | take 2 | select pid\n\
         get process | take 3 | select pid\n\
         @-1 | count | to json\n\
         try { @-3 | count } catch e { $e | select name | to json }",
    );
    run.assert_success();
    let printed = run.stdout();
    assert!(
        printed.contains("[3]"),
        "`@-1` is the newest of the two retained results: {printed:?}"
    );
    assert!(
        printed.contains("resolve.target_not_found"),
        "§24.2: the oldest was evicted, so `@-3` names nothing: {printed:?}"
    );
}

// --- §23.3: cancelling a filling capture releases it (issue #71) -------------------------------

#[test]
fn should_stop_capture_growth_within_the_cancellation_budget() {
    // §23.3: "Cancellation while capturing MUST stop upstream consumption promptly and release
    // retained values as soon as the owning operation unwinds." §28.3 puts cancellation above
    // capacity, so a capture that has not reached its ceiling still stops.
    //
    // The proof is what a person at the terminal sees, and it is not a duration: a capture over a
    // walk of the whole filesystem — which would fill until the command budget refused it —
    // stops at Ctrl-C, reports 128 + SIGINT, and leaves a shell that answers the next command.
    // The retained values cannot still be held by a shell that has returned to its prompt.
    //
    // The *latency* half of this box — p95 < 100 ms, p99 < 250 ms — is measured by
    // `ono-pipeline/tests/cancellation.rs::should_meet_the_p95_and_p99_cancellation_targets_over_repeated_runs`
    // and belongs to phase H7's benchmark harness (#83, #84, ADR-0459): a millisecond threshold
    // asserted on shared hardware is issue #21's defect, not a proof.
    let mut shell = interactive_shell();
    support::read_until(&mut shell, ">", std::time::Duration::from_secs(10));

    shell
        .write_all(b"let everything = (find file / | select path)\n")
        .expect("the terminal accepts a command");
    std::thread::sleep(std::time::Duration::from_millis(600));
    shell
        .write_all(&[0x03])
        .expect("the terminal accepts Ctrl-C");

    shell
        .write_all(b"echo alive-$?\n")
        .expect("the terminal accepts the follow-up");
    let seen = support::read_until(&mut shell, "alive-130", std::time::Duration::from_secs(8));
    assert!(
        seen.contains("alive-130"),
        "§23.3, §28.3: the filling capture was cancelled with 128+SIGINT and the shell kept \
         going; saw:\n{seen}"
    );

    // And the shell is usable afterwards, which a session still holding an unbounded capture
    // would not be: the next command runs, and its own capture succeeds.
    shell
        .write_all(b"let small = (get process | take 1 | select pid)\necho after-$?\n")
        .expect("the terminal accepts another capture");
    let after = support::read_until(&mut shell, "after-0", std::time::Duration::from_secs(8));
    assert!(
        after.contains("after-0"),
        "§23.3: the retained values were released when the operation unwound, so the next \
         capture has its whole allowance; saw:\n{after}"
    );

    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

/// Starts `ono` interactively on a pseudo-terminal, as a person would.
fn interactive_shell() -> ono_process::PtySession {
    let mut executor = ono_process::Executor::detached();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    executor
        .run_pty(&command, ono_process::WindowSize::new(24, 80))
        .expect("a pseudo-terminal must be available")
}
