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
fn should_change_the_directory_a_native_command_sees_when_cd_has_run() {
    // `cd` moved the shell's own idea of where it stands and the directory an external command
    // inherits, and left every native command resolving `.` against wherever the process
    // happened to start. `find file .` after a `cd` therefore walked the shell's launch
    // directory, which is a different machine from the one the user is looking at.
    let dir = scratch();
    dir.write("inside/one.txt", "1\n");
    dir.write("inside/two.txt", "2\n");
    let run = ono(&format!(
        "cd {}/inside\nfind file . | select name | to text",
        dir.path().display()
    ));
    run.assert_success();
    assert!(
        run.stdout().contains("one.txt") && run.stdout().contains("two.txt"),
        "a relative path names the directory the shell is standing in, got {:?}",
        run.stdout()
    );
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

#[test]
fn should_report_what_each_external_stage_is_asked_to_produce() {
    // Spec v0.3 §1.4: adaptation is demand-driven, and the demand is decided while planning —
    // so `explain` can state it before anything runs.
    let structured = Shell::new()
        .args(["-c", "explain ps aux | where cpu > 20"])
        .run();
    structured.assert_success();
    assert!(
        structured
            .stdout()
            .contains("demand       structured (`where cpu > 20` consumes objects)"),
        "a native transform downstream asks the external stage for values, got {:?}",
        structured.stdout()
    );

    let bytes = Shell::new().args(["-c", "explain ps aux | grep x"]).run();
    bytes.assert_success();
    assert!(
        bytes
            .stdout()
            .contains("demand       bytes (`grep x` consumes bytes)"),
        "a process downstream keeps Unix byte semantics, got {:?}",
        bytes.stdout()
    );
    assert!(
        bytes
            .stdout()
            .contains("demand       bytes (stdout is not a terminal)"),
        "the last stage's stdout is this test's pipe, got {:?}",
        bytes.stdout()
    );

    let discarded = Shell::new()
        .args(["-c", "explain \"ps aux > /dev/null\""])
        .run();
    discarded.assert_success();
    assert!(
        discarded
            .stdout()
            .contains("demand       discard (stdout goes to /dev/null)"),
        "got {:?}",
        discarded.stdout()
    );
}

#[test]
fn should_explain_that_raw_bypasses_adaptation() {
    // Spec v0.3 §1.17: the bypass is inspectable like everything else. The plan names the
    // program, says adaptation is bypassed, and the demand row shows bytes regardless of the
    // consumer.
    let run = Shell::new()
        .args(["-c", "explain raw ps aux | where cpu > 20"])
        .run();
    run.assert_success();
    let text = run.stdout();
    assert!(
        text.contains("adaptation   bypassed (`raw`, spec v0.3 §1.17)"),
        "got {text:?}"
    );
    assert!(
        text.contains("demand       bytes (`raw` bypasses adaptation)"),
        "got {text:?}"
    );
    assert!(
        text.contains("`ps` is an external program and resolves to"),
        "the program behind `raw` is the one that is resolved, got {text:?}"
    );
}

#[test]
fn should_document_raw_in_help() {
    let run = Shell::new().args(["-c", "help raw"]).run();
    run.assert_success();
    assert!(
        run.stdout().contains("raw") && run.stdout().contains("adapt"),
        "spec v0.3 §1.17: the escape hatch is documented where the rest is, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_explain_which_adapter_answers_and_what_it_will_run() {
    // Spec v0.3 §1.23, §1.57: the hidden machinery is inspectable. lsblk is on every Linux
    // machine this suite runs on (util-linux), so the bundled adapter answers.
    let adapted = Shell::new()
        .args(["-c", "explain lsblk | where type == disk"])
        .run();
    adapted.assert_success();
    let text = adapted.stdout();
    assert!(
        text.contains("adaptation   adapted by org.ono.compat.util-linux.lsblk"),
        "the winner is named, got {text:?}"
    );
    assert!(
        text.contains("argv         lsblk --json --list --bytes --output"),
        "the rewritten invocation is shown (spec v0.3 §1.8), got {text:?}"
    );
    assert!(
        text.contains("candidates   org.ono.compat.util-linux.lsblk (the only candidate)"),
        "spec v0.3 §1.25: candidates and the selection reason, got {text:?}"
    );

    let unsupported = Shell::new()
        .args(["-c", "explain lsblk -p | where type == disk"])
        .run();
    unsupported.assert_success();
    assert!(
        unsupported
            .stdout()
            .contains("adaptation   unsupported invocation: org.ono.compat.util-linux.lsblk cannot guarantee `-p`; fails"),
        "spec v0.3 §1.16, §1.18: an undeclared flag under a structured demand fails rather than downgrades, got {:?}",
        unsupported.stdout()
    );

    let bytes = Shell::new().args(["-c", "explain lsblk | grep sda"]).run();
    bytes.assert_success();
    assert!(
        bytes
            .stdout()
            .contains("adaptation   raw (downstream bytes)"),
        "got {:?}",
        bytes.stdout()
    );

    let none = Shell::new()
        .args(["-c", "explain grep x | where a == 1"])
        .run();
    none.assert_success();
    assert!(
        none.stdout().contains("adaptation   raw (no adapter)"),
        "spec v0.3 §1.70: text tools stay raw, got {:?}",
        none.stdout()
    );
}

#[test]
fn should_answer_type_with_the_adapters_schema_and_check_fields_before_running() {
    // Spec v0.3 §1.61: `type` knows what an adapted stage produces; spec §11.3: a field typo is
    // caught before anything runs — for an adapted program as for a native one.
    let typed = Shell::new()
        .args(["-c", "type \"lsblk | where type == \\\"disk\\\"\""])
        .run();
    typed.assert_success();
    assert!(
        typed.stdout().contains("ono.block-device/1"),
        "the adapter's schema flows through the plan, got {:?}",
        typed.stdout()
    );
    let typo = Shell::new()
        .args(["-c", "lsblk | where colour == \"blue\""])
        .run();
    assert_ne!(typo.status().code(), 0);
    assert!(
        typo.stderr().contains("Ono-Sendai-E0202") && typo.stderr().contains("ono.block-device/1"),
        "type.unknown_field against the adapter's schema, before lsblk runs, got {:?}",
        typo.stderr()
    );
}

// --- `set` and `remove` are builtins only for the shell's own state -------------------------

#[test]
fn should_dispatch_set_of_a_system_target_through_the_registry_rather_than_the_builtin() {
    // `set env` and `set config` change the session, so they are the shell's own. `set file` is
    // a native command like any other (docs/spec/commands/file.yaml, `ono.file.set`): the
    // registry's implementation answers — with an ActionResult row naming `ono.file.set`
    // whose failure is the file's absence (ADR-0068 §2, ADR-0082) — never E0102 claiming the
    // verb has no such target.
    let run = ono("set file /definitely/not/here --mode 0755 | to json");
    run.assert_status(1);
    assert!(
        run.stdout().contains("\"operation\":\"ono.file.set\"")
            && run.stdout().contains("Ono-Sendai-E0301"),
        "the registry answers for `set file`, got {:?} / {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        !run.stderr().contains("Ono-Sendai-E0102"),
        "`set` is not a builtin for a system target, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_let_remove_of_a_system_target_stand_in_a_pipeline() {
    // `remove file … | to json` is a mutation whose ActionResults flow on; only `remove env`
    // runs in the shell itself.
    let run = ono("remove file /definitely/not/here | to json");
    run.assert_status(1);
    assert!(
        run.stdout().contains("\"operation\":\"ono.file.remove\"")
            && run.stdout().contains("Ono-Sendai-E0301"),
        "the registry answers for `remove file` in pipeline position, got {:?} / {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        !run.stderr().contains("cannot be a pipeline stage"),
        "a native mutation is not refused as a builtin, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_keep_set_env_and_remove_env_in_the_shell_itself() {
    let run = ono(
        "set env SEAM_MARK = kept; echo $SEAM_MARK; remove env SEAM_MARK; echo \"<$SEAM_MARK>\"",
    );
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "kept\n<>\n",
        "`set env` binds and `remove env` withdraws in the running shell"
    );
}

#[test]
fn should_answer_get_env_with_a_variable_set_env_bound_in_the_same_session() {
    // `get env` describes the session's environment, so what `set env` just bound is in it —
    // exactly as `$NAME` and a child's `printenv` already see it.
    let run =
        ono("set env LIVE_PROBE = live; get env LIVE_PROBE | select name value source | to json");
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        r#"[{"name":"LIVE_PROBE","value":"live","source":"shell"}]"#,
        "ono.env-var/1: the variable bound this session is listed, with its source, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_not_list_a_variable_remove_env_withdrew_in_the_same_session() {
    let run = Shell::new()
        .env("STALE_PROBE", "inherited")
        .args(["-c", "remove env STALE_PROBE; get env STALE_PROBE | count"])
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout().trim(),
        "VALUE\n0",
        "a withdrawn variable is no longer in the session's environment, got {:?}",
        run.stdout()
    );
}

// --- a head that is both a native command and a program (ADR-0028, ADR-0260) -----------------

#[test]
fn should_run_the_program_with_its_flags_when_a_native_head_is_reached_by_bytes() {
    // `sort` is a transform of the object pipeline and a coreutils program. Reached by bytes it
    // is the program (ADR-0028), and a program's arguments are the words the user typed —
    // `-r` is a flag, not the negation of a field called `r`.
    let run = Shell::new()
        .args(["-c", "sort -r"])
        .stdin("b\na\nc\n")
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "c\nb\na\n",
        "`sort -r` is coreutils sort with its flag: {:?}",
        run.output()
    );
}

#[test]
fn should_pass_paths_to_the_program_unmerged_when_a_native_head_is_reached_by_bytes() {
    // Read as an expression, `-u /a/b /c/d` is one arithmetic term — a negation, four divisions
    // and two subtractions — and the program would receive it as a single argument.
    let dir = scratch();
    dir.write("left.txt", "a\n");
    dir.write("right.txt", "b\n");
    let run = Shell::new()
        .args([
            "-c",
            &format!(
                "diff -u {} {}",
                dir.path().join("left.txt").display(),
                dir.path().join("right.txt").display()
            ),
        ])
        .run();
    assert!(
        run.stdout().contains("left.txt") && run.stdout().contains("right.txt"),
        "diff received two paths and a flag: {:?}",
        run.output()
    );
}

#[test]
fn should_keep_the_transform_when_objects_reach_a_head_of_the_same_name() {
    // The other side of ADR-0028: reached by objects, `sort` is the transform.
    let run = Shell::new()
        .args([
            "-c",
            "get process | where pid == 1 | sort pid desc | select pid | to json",
        ])
        .run();
    run.assert_success();
    assert_eq!(run.stdout().trim(), "[{\"pid\":1}]", "{:?}", run.output());
}
