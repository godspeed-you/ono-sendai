//! Running the Unix world: external commands, pipelines, redirection, chaining and status.
//!
//! AUTONOMOUS_IMPLEMENTATION.md §12 is explicit that invoking `echo` is not evidence of shell
//! compatibility, so these run real programs and observe real files.

use ono_testkit::ono;
use ono_testkit::{Shell, scratch};

#[test]
fn should_pass_arguments_to_an_external_command_exactly_as_typed() {
    let run = ono("printf '%s|%s|%s\\n' a-b ./c --flag=1");
    run.assert_success();
    assert_eq!(run.stdout(), "a-b|./c|--flag=1\n");
}

#[test]
fn should_pass_a_commands_own_exit_status_through_unchanged() {
    for code in [0u8, 1, 2, 42, 126, 127] {
        let run = ono(&format!("sh -c 'exit {code}'"));
        assert_eq!(run.status().code(), code, "for exit {code}");
    }
}

#[test]
fn should_report_a_signalled_command_as_128_plus_the_signal() {
    assert_eq!(ono("sh -c 'kill -TERM $$'").status().code(), 143);
}

#[test]
fn should_connect_the_stages_of_a_pipeline_when_one_is_written() {
    let run = ono("printf 'b\\na\\nc\\n' | sort | tr -d '\\n'");
    run.assert_success();
    assert_eq!(run.stdout(), "abc");
}

#[test]
fn should_report_the_last_stages_status_for_a_pipeline() {
    assert_eq!(ono("true | false").status().code(), 1);
    assert_eq!(ono("false | true").status().code(), 0);
}

#[test]
fn should_terminate_promptly_when_a_downstream_stage_stops_reading() {
    // `yes | head -1` hangs in a shell that shuttles the bytes itself (ADR-0013).
    let run = Shell::new()
        .args(["-c", "yes | head -1"])
        .timeout(std::time::Duration::from_secs(10))
        .try_run()
        .expect("the pipeline must not hang");
    assert_eq!(run.stdout(), "y\n");
}

#[test]
fn should_write_output_to_a_file_when_redirected() {
    let dir = scratch();
    let target = dir.path().join("out.txt");
    ono(&format!("echo written > {}", target.display())).assert_success();
    assert_eq!(dir.read("out.txt"), "written\n");
}

#[test]
fn should_append_rather_than_replace_when_the_append_operator_is_used() {
    let dir = scratch();
    let target = dir.path().join("out.txt");
    ono(&format!(
        "echo one > {t}\necho two >> {t}",
        t = target.display()
    ))
    .assert_success();
    assert_eq!(dir.read("out.txt"), "one\ntwo\n");
}

#[test]
fn should_read_input_from_a_file_when_redirected() {
    let dir = scratch();
    dir.write("in.txt", "alpha\nbravo\n");
    let run = ono(&format!("wc -l < {}", dir.path().join("in.txt").display()));
    run.assert_success();
    assert!(run.stdout().trim().starts_with('2'), "{:?}", run.stdout());
}

#[test]
fn should_send_a_commands_complaint_to_the_error_stream_when_it_is_not_redirected() {
    let dir = scratch();
    let target = dir.path().join("out.txt");
    let run = ono(&format!(
        "sh -c 'echo out; echo err >&2' > {}",
        target.display()
    ));
    run.assert_success();
    assert_eq!(dir.read("out.txt"), "out\n");
    assert!(run.stderr().contains("err"), "{:?}", run.stderr());
}

#[test]
fn should_fold_the_error_stream_into_output_when_asked_to() {
    let run = ono("sh -c 'echo err >&2' 2>&1");
    run.assert_success();
    assert!(run.stdout().contains("err"), "{:?}", run.stdout());
}

#[test]
fn should_report_a_redirection_it_cannot_open_and_not_run_the_command() {
    let dir = scratch();
    let marker = dir.path().join("marker");
    let run = ono(&format!(
        "sh -c 'touch {}' < /definitely/not/here",
        marker.display()
    ));
    assert!(!run.status().is_success());
    assert!(!marker.exists(), "the command must not have run");
    assert!(
        run.stderr().contains("Ono-Sendai-E0301"),
        "got {:?}",
        run.stderr()
    );
}

#[test]
fn should_run_the_right_side_only_when_the_left_side_succeeded() {
    assert_eq!(ono("true && echo yes").stdout(), "yes\n");
    assert_eq!(ono("false && echo yes").stdout(), "");
}

#[test]
fn should_run_the_right_side_only_when_the_left_side_failed() {
    assert_eq!(ono("false || echo fallback").stdout(), "fallback\n");
    assert_eq!(ono("true || echo fallback").stdout(), "");
}

#[test]
fn should_chain_from_left_to_right_when_several_operators_are_written() {
    assert_eq!(ono("false && echo a || echo b").stdout(), "b\n");
    assert_eq!(ono("true && echo a || echo b").stdout(), "a\n");
}

#[test]
fn should_separate_statements_written_on_one_line_with_a_semicolon() {
    assert_eq!(ono("echo one; echo two").stdout(), "one\ntwo\n");
}

#[test]
fn should_run_a_path_directly_without_searching_the_command_path() {
    let run = ono("/bin/echo direct");
    run.assert_success();
    assert_eq!(run.stdout(), "direct\n");
}

#[test]
fn should_report_a_file_that_is_not_a_program_as_not_executable() {
    // ADR-0017: status 126, reachable only because the format is checked in the parent.
    let dir = scratch();
    dir.write("notaprogram", [0x00u8, 0x01, 0x02, 0x03]);
    let path = dir.path().join("notaprogram");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");

    assert_eq!(ono(&path.display().to_string()).status().code(), 126);
}

#[test]
fn should_run_a_script_with_a_shebang_when_it_is_executable() {
    let dir = scratch();
    dir.write("hello.sh", "#!/bin/sh\necho from-shebang\n");
    let path = dir.path().join("hello.sh");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");

    let run = ono(&path.display().to_string());
    run.assert_success();
    assert_eq!(run.stdout(), "from-shebang\n");
}

#[test]
fn should_force_an_external_program_when_the_exec_namespace_is_used() {
    // ADR-0011: `exec:` resolves in step 5 only.
    let run = ono("exec:echo forced");
    run.assert_success();
    assert_eq!(run.stdout(), "forced\n");
}

#[test]
fn should_not_fall_back_to_another_namespace_when_a_forced_one_misses() {
    // ADR-0011: forcing a namespace is a statement of intent, never a suggestion.
    let run = ono("ono:definitely-not-a-native-command");
    assert_eq!(run.status().code(), 127);
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "{:?}",
        run.stderr()
    );
}

#[test]
fn should_run_no_stage_at_all_when_one_of_them_cannot_be_resolved() {
    // Bash runs the stages it can, so `nonesuch | cat` succeeds and reports 0 — an empty result
    // that looks like a real one. ADR-0008: a pipeline that cannot be built runs nothing.
    let dir = scratch();
    let marker = dir.path().join("ran");

    let run = ono(&format!(
        "sh -c 'touch {}' | definitely-not-a-command",
        marker.display()
    ));
    assert_eq!(run.status().code(), 127);
    assert!(!marker.exists(), "the resolvable stage must not have run");

    let run = ono(&format!(
        "definitely-not-a-command | sh -c 'touch {}'",
        marker.display()
    ));
    assert_eq!(run.status().code(), 127);
    assert!(!marker.exists(), "the resolvable stage must not have run");
}

#[test]
fn should_run_no_stage_at_all_when_a_redirection_cannot_be_opened() {
    let dir = scratch();
    let marker = dir.path().join("ran");
    let run = ono(&format!(
        "sh -c 'touch {}' | cat < /definitely/not/here",
        marker.display()
    ));
    assert!(!run.status().is_success());
    assert!(
        !marker.exists(),
        "nothing runs when a redirection cannot be opened"
    );
}

#[test]
fn should_report_a_write_that_the_system_refused_rather_than_reporting_success() {
    // A shell that reported success for a write that failed would lose data silently, which is
    // the one thing a shell must never do.
    let run = ono("echo x > /proc/version");
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E03"),
        "{:?}",
        run.stderr()
    );
}

// ---------------------------------------------------------------------------------------------
// The raw bypass (spec v0.3 §1.17, ADR-0054): `raw <program> …` runs the program with nothing
// between it and the terminal — no argv rewrite, no decoder, no renderer, its own exit status.

#[test]
fn should_run_the_program_exactly_as_typed_under_raw() {
    let run = ono("raw printf '%s|%s\\n' a-b --flag=1");
    run.assert_success();
    assert_eq!(run.stdout(), "a-b|--flag=1\n");
}

#[test]
fn should_pass_the_programs_exit_status_through_under_raw() {
    assert_eq!(ono("raw sh -c 'exit 3'").status().code(), 3);
}

#[test]
fn should_keep_bytes_verbatim_at_a_terminal_under_raw() {
    // At a terminal a high-confidence adapter may render (spec v0.3 §1.4); `raw` is the promise
    // that it never does. A tab and a trailing space survive untouched.
    let mut executor = ono_process::Executor::detached();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .args(["-c", "raw printf 'a\\tb \\n'"])
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env("HOME", std::env::temp_dir().display().to_string());
    let mut session = executor
        .run_pty(&command, ono_process::WindowSize::new(24, 80))
        .expect("a pseudo-terminal must be available");
    let mut seen = Vec::new();
    let mut buffer = [0u8; 4096];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match session.read_timeout(&mut buffer, std::time::Duration::from_millis(200)) {
            Ok(Some(0)) | Err(_) => break,
            Ok(Some(count)) => seen.extend_from_slice(&buffer[..count]),
            Ok(None) => {}
        }
    }
    assert!(
        seen.windows(6).any(|window| window == b"a\tb \r\n"),
        "the terminal sees the program's bytes (tab, trailing space, newline), got {:?}",
        String::from_utf8_lossy(&seen)
    );
}

#[test]
fn should_refuse_raw_without_a_program() {
    let run = ono("raw");
    assert_eq!(
        run.status().code(),
        127,
        "nothing to run is not found (ADR-0008), got {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0101") && run.stderr().contains("raw"),
        "spec §43: the shell says what `raw` needs, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_never_resolve_a_native_command_under_raw() {
    // `raw` means the program on PATH and nothing else: a native verb behind it is not found.
    let run = ono("raw get process");
    assert_ne!(run.status().code(), 0);
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "`get` is not a program, so raw cannot run it, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_run_the_program_under_raw_even_where_structure_reaches_it() {
    // `sort` after a stream of records is the transform (ADR-0028); `raw sort` is /usr/bin/sort,
    // fed the rendered bytes, whatever arrives.
    let run = ono("printf 'b\\na\\n' | raw sort");
    run.assert_success();
    assert_eq!(run.stdout(), "a\nb\n");
    // Objects still need a representation before a program (spec §12.3); `raw` changes what
    // runs, not what the join requires.
    let counted = ono("get process | where pid == 1 | to json | raw wc -c");
    counted.assert_success();
    assert!(
        counted
            .stdout()
            .trim()
            .parse::<u32>()
            .is_ok_and(|bytes| bytes > 0),
        "`wc` counted the serialised bytes, got {:?}",
        counted.stdout()
    );
}
