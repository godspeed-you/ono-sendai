//! A KUANG/11 instance is the session's, whichever of its evaluators runs the package (spec §31.8,
//! §31.10, §31.37; ADR-0952, ADR-0959, ADR-0995).
//!
//! A background job has an evaluator of its own (ADR-0952). Issue #304: a package command in a job
//! found no loaded package in the job's own host and started a second, unlisted instance, which
//! the job's end then tore down without an unload. These tests hold the shell to one host that
//! knows every instance: what a job runs is what `get plugin` lists, and the audit trail records
//! one load and one unload for every instance that ran.
//!
//! No test here asserts a duration; the shell's watchdog bounds every run.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0913).
#![cfg(feature = "kuang")]
#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::{Scratch, Shell};

/// The example package, laid out as installed, declaring `emit` as `get echo-item` so that a
/// job can reach it before anything has loaded it (spec §31.68).
fn plugin_home() -> Scratch {
    let scratch = ono_testkit::scratch();
    let manifest = r#"
format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/echo
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
capabilities:
  optional:
    - clock.read
network:
  outbound: none
contributions:
  commands: [contributions/commands.yaml]
"#;
    scratch.write("plugins/dev.example.echo/manifest.yaml", manifest);
    scratch.write(
        "plugins/dev.example.echo/contributions/commands.yaml",
        r#"
commands:
  - id: dev.example.echo.command.emit
    verb: get
    target: echo-item
    summary: Emit a counted stream of integers.
    output: stream<int>
    argument_mode: expression
    capabilities: []
    options:
      - name: count
        type: int
        doc: How many integers to emit.
        default: 2
    examples:
      - get echo-item --count 3
"#,
    );
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = scratch.path().join("plugins/dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    ono_testkit::executable_copy(&binary, &entry);
    scratch
}

/// `ono -c script` with the plugin home and a state root of its own, so the audit trail outlives
/// the run (spec §31.37).
fn ono(home: &Scratch, script: &str) -> ono_testkit::Run {
    let root = home.path();
    Shell::new()
        .args(["-c", script])
        .env(
            "ONO_PLUGIN_PATH",
            root.join("plugins").display().to_string(),
        )
        .env("HOME", root.join("home").display().to_string())
        .env("XDG_STATE_HOME", root.join("state").display().to_string())
        .env("XDG_CONFIG_HOME", root.join("config").display().to_string())
        .env("XDG_CACHE_HOME", root.join("cache").display().to_string())
        .env(
            "ONO_CONFIG_DIR",
            root.join("config/ono").display().to_string(),
        )
        .timeout(Duration::from_secs(60))
        .run()
}

/// The lifecycle actions the host recorded for the package, in the order the trail holds them,
/// as a later session reads them back from disk.
fn lifecycle_trail(home: &Scratch) -> Vec<String> {
    let run = ono(
        home,
        r#"get audit --plugin dev.example.echo | where invocation == "host" | where action == "lifecycle.load" or action == "lifecycle.unload" | select action | to json"#,
    );
    run.assert_success();
    support::last_json_rows(&run)
        .iter()
        .map(|row| support::text(row, "action"))
        .collect()
}

#[test]
fn should_list_the_instance_a_background_job_loaded_in_get_plugin() {
    // Issue #304: a job that loads a package lazily loads it into the session's host, so the
    // shell's own `get plugin` lists it as the running instance — not as a package no code of
    // which has run.
    let home = plugin_home();
    let run = ono(
        &home,
        "get echo-item --count 3 | to json &\nfg %1\n\
         get plugin | where id == \"dev.example.echo\" | select state | to json",
    );

    run.assert_success();
    assert!(
        run.stdout().contains("[1,2,3]"),
        "the job ran the package's command and `fg` collected it: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains(r#"{"state":"loaded"}"#),
        "spec §31.8: an instance a job started is the session's, visible through `get plugin`: {:?}",
        run.output()
    );
}

#[test]
fn should_record_one_load_and_one_unload_when_a_job_and_the_shell_use_one_package() {
    // Issue #304's exit test: `get plugin` and the audit log account for every instance that ran,
    // and each instance that ends records its unload. The shell loads the package, a job and the
    // shell both run its command — one instance, so one load and, when the shell leaves, one
    // unload.
    let home = plugin_home();
    let run = ono(
        &home,
        "load plugin dev.example.echo\n\
         get echo-item --count 2 | to json &\nfg %1\n\
         get echo-item --count 1 | to json",
    );
    run.assert_success();
    assert!(
        run.stdout().contains("[1,2]") && run.stdout().contains("[1]"),
        "the job and the shell each got their own answer: {:?}",
        run.output()
    );

    assert_eq!(
        lifecycle_trail(&home),
        ["lifecycle.load", "lifecycle.unload"],
        "spec §31.37: one instance ran, so the trail holds one load and the unload that ended it"
    );
}

#[test]
fn should_unload_the_instance_in_order_when_the_shell_leaves_with_a_job_still_running_it() {
    // ADR-0959: a leaving shell ends its jobs within one bounded grace period, and the instance
    // a job is still streaming from ends with them — through its unload, not by being dropped.
    // Meanwhile the shell's own invocation of the same instance is answered correctly.
    let home = plugin_home();
    let runtime = home
        .path()
        .join("plugins/dev.example.echo/runtime/echo")
        .display()
        .to_string();
    let run = ono(
        &home,
        // The shell waits until the job's invocation has made the instance active — spec §31.8's
        // state, read through `get plugin` — so the shell's own invocation below runs while the
        // job's is still streaming. A job running a second instance never gets there, and the
        // watchdog ends the run.
        "load plugin dev.example.echo\n\
         echo:count-forever | count &\n\
         while (get plugin | where id == \"dev.example.echo\" and state == \"active\" | count) == 0 { sleep 0.05 }\n\
         echo:emit --count 3 | to json",
    );

    run.assert_success();
    assert!(
        run.stdout().contains("[1,2,3]"),
        "the shell's invocation is answered while a job streams from the same instance: {:?}",
        run.output()
    );
    let left: Vec<u32> = support::processes_naming(&runtime);
    support::kill_processes_naming(&runtime);
    assert!(
        left.is_empty(),
        "no instance outlives the shell that ran it: {left:?}"
    );
    assert_eq!(
        lifecycle_trail(&home),
        ["lifecycle.load", "lifecycle.unload"],
        "spec §31.37: the instance the job was still using ended with its unload"
    );
}

/// The rows `get job` answered last, as `id` and `state`.
fn job_states(run: &ono_testkit::Run) -> Vec<(i64, String)> {
    support::last_json_rows(run)
        .iter()
        .map(|row| {
            (
                row.get("id")
                    .and_then(serde_yaml_ng::Value::as_i64)
                    .expect("a job row names its number"),
                support::text(row, "state"),
            )
        })
        .collect()
}

#[test]
fn should_end_a_killed_jobs_invocation_and_leave_the_instance_loaded_and_usable() {
    // ADR-0952, ADR-0995: a job runs packages in the session's instance, so `kill %N` cannot end
    // the job by ending an instance of its own. The job's cancellation is delivered to the
    // invocation it has running (spec §31.14): the job ends, the instance goes back to `loaded`
    // and answers the next invocation.
    let home = plugin_home();
    let run = ono(
        &home,
        "load plugin dev.example.echo\n\
         echo:count-forever | count &\n\
         while (get plugin | where id == \"dev.example.echo\" and state == \"active\" | count) == 0 { sleep 0.05 }\n\
         kill %1\n\
         get plugin | where id == \"dev.example.echo\" | select state | to json\n\
         echo:emit --count 2 | to json\n\
         get job | select id state | to json",
    );

    assert!(
        !run.stderr().contains("has not stopped yet"),
        "ADR-0959: `kill %N` ends a job whose line is in a package invocation: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains(r#"{"state":"loaded"}"#),
        "spec §31.8: once the killed job's invocation has ended, nothing runs in the instance: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains("[1,2]"),
        "the instance outlives the job's cancellation and answers the shell: {:?}",
        run.output()
    );
    assert_eq!(
        job_states(&run),
        [],
        "spec §18.4: a killed job that has ended leaves the table: {:?}",
        run.output()
    );
}

#[test]
fn should_end_only_the_killed_jobs_invocation_when_another_job_and_the_shell_use_the_instance() {
    // The cancellation is the killed job's alone: the instance and another job's invocation in it
    // keep running, and the shell's own invocation is answered meanwhile (spec §31.14, ADR-0995).
    // The shell waits until job 1's invocation has made the instance active, so job 1 is
    // certainly streaming when job 2 is killed.
    let home = plugin_home();
    let run = ono(
        &home,
        "load plugin dev.example.echo\n\
         echo:count-forever | count &\n\
         while (get plugin | where id == \"dev.example.echo\" and state == \"active\" | count) == 0 { sleep 0.05 }\n\
         echo:count-forever | count &\n\
         sleep 0.2\n\
         kill %2\n\
         echo:emit --count 3 | to json\n\
         get plugin | where id == \"dev.example.echo\" | select state | to json\n\
         get job | select id state | to json\n\
         kill %1\n\
         get job | select id state | to json",
    );

    assert!(
        !run.stderr().contains("has not stopped yet"),
        "ADR-0959: `kill %N` ends a job whose line is in a package invocation: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains("[1,2,3]"),
        "the shell's invocation is answered while job 1 streams from the same instance: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains(r#"{"state":"active"}"#),
        "spec §31.8: job 1's invocation still runs in the instance after job 2 was killed: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains(r#"[{"id":1,"state":"running"}]"#),
        "job 2 has ended and job 1 runs on: {:?}",
        run.output()
    );
    assert_eq!(
        job_states(&run),
        [],
        "killing job 1 then ends it too: {:?}",
        run.output()
    );
}

#[test]
fn should_count_the_invocation_a_job_holds_open_in_get_plugins_jobs() {
    // `ono.plugin/2`'s `jobs`: how many invocations are running, zero for anything not `active`.
    // A job can hold an invocation open in the session's instance while the shell reads the
    // table (ADR-0995), so the count is the instance's, not a constant.
    let home = plugin_home();
    let run = ono(
        &home,
        "load plugin dev.example.echo\n\
         echo:count-forever | count &\n\
         while (get plugin | where id == \"dev.example.echo\" and state == \"active\" | count) == 0 { sleep 0.05 }\n\
         get plugin | where id == \"dev.example.echo\" | select state jobs | to json\n\
         kill %1\n\
         get plugin | where id == \"dev.example.echo\" | select state jobs | to json",
    );

    run.assert_success();
    assert!(
        run.stdout().contains(r#"[{"state":"active","jobs":1}]"#),
        "an active instance counts the invocation the job runs in it: {:?}",
        run.output()
    );
    assert!(
        run.stdout().contains(r#"[{"state":"loaded","jobs":0}]"#),
        "once the job's invocation has ended, nothing is counted: {:?}",
        run.output()
    );
}

#[test]
fn should_load_a_package_once_when_a_job_and_the_shell_both_load_it_lazily() {
    // R-F1 of the v0.6.5 review: a job and the shell share one host (ADR-0995), so both can find
    // the package not yet loaded and both start it. The second instance replaced the first under
    // the invocation that was running in it. A lazy load is one load per package however many
    // evaluators ask at once: the loser of the race runs in the winner's instance. Twenty rounds,
    // each unloading the package so that the next one loads it lazily again from both sides.
    let home = plugin_home();
    let rounds = 20;
    let answers = home.path().join("answers");
    std::fs::create_dir_all(&answers).expect("a directory for the jobs' answers");
    let script: String = (1..=rounds)
        .map(|round| {
            format!(
                "get echo-item --count 3 | to json > '{}/{round}.json' &\n\
                 get echo-item --count 2 | to json\n\
                 while (get job | where state == \"running\" | count) > 0 {{ sleep 0.01 }}\n\
                 unload plugin dev.example.echo | count | to json\n",
                answers.display()
            )
        })
        .collect();
    let run = ono(&home, &script);

    run.assert_success();
    assert!(
        !run.stderr().contains("Ono-Sendai"),
        "no invocation failed, in the job or in the shell: {:?}",
        run.output()
    );
    for round in 1..=rounds {
        let answer =
            std::fs::read_to_string(answers.join(format!("{round}.json"))).unwrap_or_default();
        assert_eq!(
            answer.trim(),
            "[1,2,3]",
            "round {round}: the job's invocation was answered: {:?}",
            run.output()
        );
    }
    assert_eq!(
        run.stdout().matches("[1,2]").count(),
        rounds,
        "every foreground invocation was answered: {:?}",
        run.output()
    );
    let expected: Vec<&str> = (0..rounds)
        .flat_map(|_| ["lifecycle.load", "lifecycle.unload"])
        .collect();
    assert_eq!(
        lifecycle_trail(&home),
        expected,
        "spec §31.37: each round started one instance, so the trail holds one load and one \
         unload per round"
    );
}
