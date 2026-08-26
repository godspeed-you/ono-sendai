//! How `ono` may be started, and what it does with each form (ADR-0010).

use ono_testkit::{Shell, scratch};

#[test]
fn should_run_a_command_given_on_the_command_line_and_exit_with_its_status() {
    let run = Shell::new().args(["-c", "echo hello"]).run();
    run.assert_success();
    assert_eq!(run.stdout(), "hello\n");
}

#[test]
fn should_exit_with_the_commands_status_when_it_fails() {
    Shell::new().args(["-c", "exit 3"]).run().assert_status(3);
    Shell::new().args(["-c", "false"]).run().assert_status(1);
}

#[test]
fn should_run_a_script_file_when_given_a_path() {
    let dir = scratch();
    dir.write("work.ono", "echo from-a-file\nexit 7\n");
    let run = Shell::new()
        .args([dir.path().join("work.ono").display().to_string()])
        .run();
    run.assert_status(7);
    assert!(run.stdout().contains("from-a-file"), "{:?}", run.stdout());
}

#[test]
fn should_read_a_script_from_standard_input_when_asked() {
    let run = Shell::new().args(["-"]).stdin("echo piped-in\n").run();
    run.assert_success();
    assert_eq!(run.stdout(), "piped-in\n");
}

#[test]
fn should_report_a_missing_script_rather_than_starting_a_shell() {
    let run = Shell::new().args(["/definitely/not/here.ono"]).run();
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E0301") || run.stderr().contains("not found"),
        "got {:?}",
        run.stderr()
    );
}

#[test]
fn should_fail_with_the_usage_status_when_the_source_cannot_be_parsed() {
    // ADR-0008: a command line that cannot be understood exits 2.
    let run = Shell::new().args(["-c", "echo )"]).run();
    run.assert_status(2);
    assert!(
        run.stderr().contains("Ono-Sendai-E0001"),
        "a parse error must carry its code, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_point_at_the_offending_text_when_it_reports_a_parse_error() {
    // Spec §16.3: parse errors point at the relevant span.
    let run = Shell::new().args(["-c", "echo )"]).run();
    assert!(
        run.stderr().contains("echo )"),
        "the diagnostic must show the line, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_report_an_unknown_command_with_its_code_and_the_command_not_found_status() {
    let run = Shell::new().args(["-c", "definitely-not-a-command"]).run();
    run.assert_status(127);
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("definitely-not-a-command"),
        "the error must name what could not be found, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_print_nothing_extra_when_it_runs_a_command_non_interactively() {
    // Spec §4.1 and §4.6: no identity line, no prompt, nothing that would corrupt a pipe.
    let run = Shell::new().args(["-c", "echo only-this"]).run();
    assert_eq!(run.stdout(), "only-this\n");
    assert_eq!(run.stderr(), "");
}

#[test]
fn should_keep_reading_the_script_after_a_command_fails_unless_told_otherwise() {
    let run = Shell::new().args(["-c", "false\necho after"]).run();
    assert!(run.stdout().contains("after"), "{:?}", run.stdout());
}

#[test]
fn should_exit_with_the_last_statements_status_when_a_script_ends() {
    Shell::new()
        .args(["-c", "true\nfalse"])
        .run()
        .assert_status(1);
    Shell::new()
        .args(["-c", "false\ntrue"])
        .run()
        .assert_success();
}

#[test]
fn should_ignore_a_configuration_file_when_asked_to() {
    let dir = scratch();
    dir.write("config.ono", "this is not valid ono ) ) )\n");
    let run = Shell::new()
        .args(["--no-config", "-c", "echo started"])
        .env(
            "ONO_CONFIG",
            dir.path().join("config.ono").display().to_string(),
        )
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "started\n");
    assert_eq!(run.stderr(), "", "--no-config means the file is never read");
}

#[test]
fn should_start_anyway_and_say_so_when_the_configuration_is_broken() {
    // ADR-0010: a shell that refuses to start has removed the tool needed to repair its config.
    let dir = scratch();
    dir.write("config.ono", "echo )\n");
    let run = Shell::new()
        .args(["-c", "echo started"])
        .env(
            "ONO_CONFIG",
            dir.path().join("config.ono").display().to_string(),
        )
        .run();
    run.assert_success();
    assert!(run.stdout().contains("started"), "{:?}", run.stdout());
    assert!(
        !run.stderr().is_empty(),
        "a broken config must be reported, not swallowed"
    );
}

#[test]
fn should_refuse_to_run_an_external_command_while_reading_the_configuration() {
    // ADR-0010: config mode cannot execute, reach the network or load a plugin.
    let dir = scratch();
    let marker = dir.path().join("touched");
    dir.write("config.ono", format!("touch {}\n", marker.display()));
    let run = Shell::new()
        .args(["-c", "echo started"])
        .env(
            "ONO_CONFIG",
            dir.path().join("config.ono").display().to_string(),
        )
        .run();
    run.assert_success();
    assert!(
        !marker.exists(),
        "config mode must not run external commands"
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0702"),
        "the refusal must be a structured policy error, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_still_answer_version_and_help_when_the_interpreter_exists() {
    Shell::new().args(["--version"]).run().assert_success();
    Shell::new().args(["--help"]).run().assert_success();
}

#[test]
fn should_report_an_unusable_option_with_the_usage_status() {
    Shell::new()
        .args(["--definitely-not-a-flag"])
        .run()
        .assert_status(2);
}

#[test]
fn should_run_a_script_piped_into_it_rather_than_opening_a_prompt() {
    // `echo 'command' | ono` is how a shell is driven from another program. Opening a prompt here
    // would read the terminal instead and silently ignore what was piped in — the failure is
    // invisible, because the shell exits successfully having done nothing.
    let run = Shell::new().stdin("echo from-a-pipe\nexit 4\n").run();
    run.assert_status(4);
    assert!(
        run.stdout().contains("from-a-pipe"),
        "got {:?}",
        run.stdout()
    );
}

#[test]
fn should_exit_quietly_when_standard_input_is_closed() {
    let run = Shell::new().stdin("").run();
    run.assert_success();
    assert_eq!(run.stdout(), "");
}

#[test]
fn should_print_no_prompt_and_no_identity_line_when_driven_from_a_pipe() {
    // Spec §4.1 and §4.6: nothing a pipe would have to filter.
    let run = Shell::new().stdin("echo only-this\n").run();
    run.assert_success();
    assert_eq!(run.stdout(), "only-this\n");
    assert_eq!(run.stderr(), "");
}
