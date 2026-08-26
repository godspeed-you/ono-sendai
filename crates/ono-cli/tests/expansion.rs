//! Word expansion, exactly as ADR-0019 fixes it.

use ono_testkit::{Shell, scratch};

fn ono(source: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", source]).run()
}

fn ono_in(dir: &std::path::Path, source: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", source]).cwd(dir).run()
}

#[test]
fn should_expand_an_environment_variable_inside_a_word() {
    let run = Shell::new()
        .args(["-c", "echo prefix-$PROBE-suffix"])
        .env("PROBE", "value")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "prefix-value-suffix\n");
}

#[test]
fn should_expand_a_braced_variable_so_it_can_abut_ordinary_text() {
    let run = Shell::new()
        .args(["-c", "echo ${PROBE}able"])
        .env("PROBE", "read")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "readable\n");
}

#[test]
fn should_expand_an_unset_variable_to_nothing_rather_than_refusing_to_run() {
    let run = Shell::new()
        .args(["-c", "echo \"[$DEFINITELY_UNSET_PROBE]\""])
        .env_remove("DEFINITELY_UNSET_PROBE")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "[]\n");
}

#[test]
fn should_keep_a_value_containing_spaces_as_one_argument() {
    // ADR-0019's central decision: a value's content never becomes a command's structure.
    let run = Shell::new()
        .args(["-c", "printf '[%s]' $PROBE"])
        .env("PROBE", "two words")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "[two words]");
}

#[test]
fn should_not_expand_a_glob_that_arrived_from_a_variable() {
    let dir = scratch();
    dir.write("a.txt", "");
    let run = Shell::new()
        .args(["-c", "printf '[%s]' $PROBE"])
        .env("PROBE", "*.txt")
        .cwd(dir.path())
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[*.txt]",
        "an expanded value is data, not syntax"
    );
}

#[test]
fn should_read_a_variable_from_the_environment_record_when_named_explicitly() {
    let run = Shell::new()
        .args(["-c", "echo $env.PROBE"])
        .env("PROBE", "explicit")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "explicit\n");
}

#[test]
fn should_expand_a_leading_tilde_to_the_home_directory() {
    let run = Shell::new()
        .args(["-c", "echo ~/inside"])
        .env("HOME", "/home/probe")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "/home/probe/inside\n");
}

#[test]
fn should_leave_a_tilde_alone_when_it_is_not_at_the_start_of_a_word() {
    let run = Shell::new()
        .args(["-c", "echo a~b"])
        .env("HOME", "/home/probe")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "a~b\n");
}

#[test]
fn should_expand_a_glob_to_the_matching_names_in_sorted_order() {
    let dir = scratch();
    for name in ["c.txt", "a.txt", "b.txt", "skip.md"] {
        dir.write(name, "");
    }
    let run = ono_in(dir.path(), "printf '[%s]' *.txt");
    run.assert_success();
    assert_eq!(run.stdout(), "[a.txt][b.txt][c.txt]");
}

#[test]
fn should_expand_a_glob_across_a_path_component() {
    let dir = scratch();
    dir.write("one/x.rs", "");
    dir.write("two/y.rs", "");
    let run = ono_in(dir.path(), "printf '[%s]' */*.rs");
    run.assert_success();
    assert_eq!(run.stdout(), "[one/x.rs][two/y.rs]");
}

#[test]
fn should_not_match_a_hidden_file_with_a_leading_star() {
    let dir = scratch();
    dir.write(".hidden", "");
    dir.write("visible", "");
    let run = ono_in(dir.path(), "printf '[%s]' *");
    run.assert_success();
    assert_eq!(run.stdout(), "[visible]");
}

#[test]
fn should_refuse_the_command_when_a_glob_matches_nothing() {
    // ADR-0019: an unresolvable pattern stops before the command rather than travelling into it
    // as a filename.
    let dir = scratch();
    let run = ono_in(dir.path(), "echo *.nothing-matches-this");
    assert!(!run.status().is_success());
    assert!(
        run.stderr().contains("Ono-Sendai-E0301"),
        "{:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("*.nothing-matches-this"),
        "the error must name the pattern, got {:?}",
        run.stderr()
    );
    assert_eq!(run.stdout(), "", "the command must not have run");
}

#[test]
fn should_treat_a_quoted_pattern_as_literal_text() {
    let dir = scratch();
    let run = ono_in(dir.path(), "printf '[%s]' '*.nothing'");
    run.assert_success();
    assert_eq!(run.stdout(), "[*.nothing]");
}

#[test]
fn should_treat_an_escaped_pattern_as_literal_text() {
    let dir = scratch();
    let run = ono_in(dir.path(), "printf '[%s]' \\*.nothing");
    run.assert_success();
    assert_eq!(run.stdout(), "[*.nothing]");
}

#[test]
fn should_keep_an_escaped_space_in_one_argument() {
    let run = ono("printf '[%s]' one\\ argument");
    run.assert_success();
    assert_eq!(run.stdout(), "[one argument]");
}

#[test]
fn should_not_expand_anything_inside_a_raw_string() {
    let run = Shell::new()
        .args(["-c", "printf '[%s]' '$PROBE ~ *'"])
        .env("PROBE", "value")
        .env("HOME", "/home/probe")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "[$PROBE ~ *]");
}

#[test]
fn should_expand_a_variable_inside_a_double_quoted_string_but_not_split_it() {
    let run = Shell::new()
        .args(["-c", "printf '[%s]' \"a $PROBE b\""])
        .env("PROBE", "two words")
        .run();
    run.assert_success();
    assert_eq!(run.stdout(), "[a two words b]");
}

#[test]
fn should_splice_a_list_into_several_arguments_because_it_is_several_values() {
    // ADR-0019: a list splices; nothing else does.
    let run = ono("printf '[%s]' [\"a b\", \"c\"]");
    run.assert_success();
    assert_eq!(run.stdout(), "[a b][c]");
}

#[test]
fn should_expand_a_variable_in_a_redirection_target() {
    let dir = scratch();
    let run = Shell::new()
        .args(["-c", "echo written > $TARGET"])
        .env("TARGET", dir.path().join("out.txt").display().to_string())
        .run();
    run.assert_success();
    assert_eq!(dir.read("out.txt"), "written\n");
}
