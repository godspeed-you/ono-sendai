//! The commands the shell must implement itself, because no child process can.

use ono_testkit::{Shell, scratch};

fn ono(source: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", source]).run()
}

#[test]
fn should_change_the_working_directory_for_the_commands_that_follow() {
    let dir = scratch();
    dir.write("inside/marker.txt", "here\n");
    let run = Shell::new()
        .args(["-c", &format!("cd {}/inside\npwd", dir.path().display())])
        .run();
    run.assert_success();
    assert!(
        std::fs::canonicalize(run.stdout().trim()).ok()
            == std::fs::canonicalize(dir.path().join("inside")).ok(),
        "got {:?}",
        run.stdout()
    );
}

#[test]
fn should_change_the_directory_an_external_command_sees_when_cd_has_run() {
    let dir = scratch();
    dir.write("inside/marker.txt", "here\n");
    let run = Shell::new()
        .args(["-c", &format!("cd {}/inside\nls", dir.path().display())])
        .run();
    run.assert_success();
    assert!(run.stdout().contains("marker.txt"), "{:?}", run.stdout());
}

#[test]
fn should_report_a_directory_it_cannot_enter_and_leave_the_shell_where_it_was() {
    let dir = scratch();
    let run = Shell::new()
        .args(["-c", "cd /definitely/not/here\npwd"])
        .cwd(dir.path())
        .run();
    assert!(
        run.stderr().contains("Ono-Sendai-E0301"),
        "{:?}",
        run.stderr()
    );
    assert!(
        std::fs::canonicalize(run.stdout().trim()).ok() == std::fs::canonicalize(dir.path()).ok(),
        "the shell must not have moved, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_return_to_the_home_directory_when_cd_is_given_no_argument() {
    let dir = scratch();
    let run = Shell::new()
        .args(["-c", "cd\npwd"])
        .env("HOME", dir.path().display().to_string())
        .run();
    run.assert_success();
    assert_eq!(
        std::fs::canonicalize(run.stdout().trim()).ok(),
        std::fs::canonicalize(dir.path()).ok()
    );
}

#[test]
fn should_exit_immediately_with_the_status_it_is_given() {
    let run = ono("exit 5\necho never");
    run.assert_status(5);
    assert_eq!(run.stdout(), "", "nothing after `exit` may run");
}

#[test]
fn should_exit_with_success_when_exit_is_given_no_status() {
    ono("exit").assert_success();
}

#[test]
fn should_bind_a_value_and_read_it_back_in_a_later_statement() {
    let run = ono("let name = \"world\"\necho hello $name");
    run.assert_success();
    assert_eq!(run.stdout(), "hello world\n");
}

#[test]
fn should_prefer_a_shell_binding_over_an_environment_variable_of_the_same_name() {
    // ADR-0010 fixes the order: the innermost binding, then the environment.
    let run = Shell::new()
        .args(["-c", "let PROBE = \"bound\"\necho $PROBE"])
        .env("PROBE", "from-environment")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "bound\n");
}

#[test]
fn should_pass_an_exported_variable_to_an_external_command() {
    let run = ono("set env PROBE = \"exported\"\nsh -c 'echo $PROBE'");
    run.assert_success();
    assert_eq!(run.stdout(), "exported\n");
}

#[test]
fn should_remove_a_variable_from_the_environment_when_asked() {
    let run = Shell::new()
        .args(["-c", "remove env PROBE\nsh -c 'echo [$PROBE]'"])
        .env("PROBE", "present")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "[]\n");
}

#[test]
fn should_explain_a_whole_pipeline_written_without_quotes() {
    // Spec §11.3's own spelling: `explain get process | where cpu > 20 | stop process`. The
    // pipes belong to the pipeline being explained, not to a pipeline around `explain`.
    let run = Shell::new()
        .args(["-c", "explain get process | where cpu > 20 | to json"])
        .run();
    run.assert_success();

    let text = run.stdout();
    assert!(
        text.contains("get process") && text.contains("where cpu > 20") && text.contains("to json"),
        "every stage of the subject appears in the plan, got {text:?}"
    );
    assert!(
        text.contains("linux.procfs"),
        "the plan names the provider that would answer (spec §42), got {text:?}"
    );
}

#[test]
fn should_explain_without_running_anything() {
    // `explain` in front of a pipeline that would fail loudly must stay silent about running it.
    let run = Shell::new()
        .args(["-c", "explain sh -c 'echo RAN >&2; exit 9'"])
        .run();
    run.assert_success();
    assert!(
        !run.stderr().contains("RAN"),
        "spec §15.3: explain never executes its subject, got {:?}",
        run.stderr()
    );
}
