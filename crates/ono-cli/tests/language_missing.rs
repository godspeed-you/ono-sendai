//! RED outcome tests for the language features the shell declares but does not run yet: pipeline
//! capture by `let` (spec §19.2), nested-pipeline values and `$( … )` interpolation (ADR-0009,
//! `docs/spec/grammar.ebnf` `paren_value`, `interpolation`), callable functions (spec §19.3,
//! §6.5 step 2, ADR-0011), aliases (spec §6.5, §30 `aliases`, ADR-0011 step 3), `now()` and the
//! timestamp literal (spec §6.3, `docs/spec/language.yaml` `builtin_functions`), prefix assignment
//! (spec §54), blocks under `each` (spec §19.4, `docs/spec/commands/data.yaml` `ono.data.each`),
//! string arithmetic (spec §6.3), keyless `sort` (`ono.data.sort`) and `kill %N` on the job table
//! (spec §18.1, §18.4).
//!
//! Every test fails today because the behaviour is missing, and passes once it is built. They
//! assert what a user observes at the CLI boundary — stdout through `to json`, stderr, exit
//! status — never how the interpreter gets there (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Shell, scratch};

fn ono(script: &str) -> ono_testkit::Run {
    Shell::new().args(["-c", script]).run()
}

// --- let captures a pipeline (spec §19.2) --------------------------------------------------

#[test]
fn should_bind_the_pipelines_value_when_let_captures_a_native_pipeline() {
    let run = ono("let n = get process | where pid == 1 | count; $n | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[1]\n",
        "spec §19.2: `let` binds the pipeline's value and prints nothing itself — today the \
         pipeline renders to the terminal and `$n` is the exit status, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_bind_a_replayable_stream_when_let_captures_records() {
    let run = ono("let hot = get process | where pid == 1; $hot | select pid | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[{\"pid\":1}]\n",
        "spec §19.2: `let hot = get process | where …` binds the records so `$hot | select …` \
         can consume them, got {:?}",
        run.stdout()
    );
}

// --- the value of a nested pipeline (grammar.ebnf `paren_value`, `interpolation`) ----------

#[test]
fn should_substitute_the_nested_pipelines_output_inside_a_double_quoted_string() {
    let run = ono(r#"echo "$(echo hi)""#);
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "hi\n",
        "grammar.ebnf `interpolation = \"$(\" pipeline \")\"`: the nested pipeline's output is \
         substituted into the string, not its exit status, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_pass_the_nested_pipelines_output_as_an_argument_when_written_in_parentheses() {
    let run = ono("echo (echo hi)");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "hi\n",
        "grammar.ebnf `paren_value`: `( … )` is the value of the nested pipeline (ADR-0009), \
         which `echo` then prints once — not `hi` on the terminal followed by the exit status, \
         got {:?}",
        run.stdout()
    );
}

#[test]
fn should_pass_a_native_pipelines_value_as_an_argument_when_written_in_parentheses() {
    let run = ono("echo (get process | where pid == 1 | count)");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "1\n",
        "grammar.ebnf `paren_value`: the count of a nested native pipeline is the argument, and \
         the nested pipeline does not render its own table, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_bind_the_nested_native_pipelines_value_when_let_uses_parentheses() {
    let run = ono("let x = (get process | where pid == 1 | count); $x | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[1]\n",
        "ADR-0009: `( … )` yields the nested pipeline's value, so `let x = (…)` binds the count, \
         got {:?}",
        run.stdout()
    );
}

// --- functions (spec §19.3, §6.5 step 2, ADR-0011) -----------------------------------------

#[test]
fn should_call_a_declared_function_by_name() {
    let run = ono("fn f() { echo hi }; f");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "hi\n",
        "spec §19.3: a declared function is callable by its name, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_resolve_a_function_before_an_external_command_of_the_same_name() {
    let run = ono("fn ls() { echo shadowed }; ls");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "shadowed\n",
        "spec §6.5 / ADR-0011: a user function is step 2 and wins over the external on PATH, \
         got {:?}",
        run.stdout()
    );
}

#[test]
fn should_apply_the_parameter_default_when_a_function_is_called_without_an_argument() {
    let run = ono("fn twice(n: Int = 1) { ($n * 2) }; twice | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[2]\n",
        "spec §19.3 `fn name(param: Type = default)`: the default binds when no argument is \
         given, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_bind_an_argument_to_the_parameter_when_a_function_is_called_with_one() {
    let run = ono("fn twice(n: Int = 1) { ($n * 2) }; twice 4 | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[8]\n",
        "spec §19.3: the positional argument binds to the declared parameter, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_yield_the_blocks_stream_when_a_function_body_is_a_pipeline() {
    let run = ono("fn one() { get process | where pid == 1 }; one | select pid | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[{\"pid\":1}]\n",
        "spec §19.3: a function returns its block's stream, which the caller's pipeline consumes, \
         got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_answer_with_the_returned_value_when_a_function_uses_return() {
    let run = ono("fn five() { return 5 }; five | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[5]\n",
        "spec §19.5 `return`: the returned expression is the function's value, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_resolve_only_user_functions_when_the_fn_namespace_is_forced() {
    let run = ono("fn f() { echo hi }; fn:f");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "hi\n",
        "language.yaml `namespaces`: `fn:` resolves in user functions only (spec §6.5), got \
         stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

// --- aliases (spec §6.5 step 2, §30 `aliases`, ADR-0011 step 3) ----------------------------

#[test]
fn should_run_the_expansion_when_an_alias_is_called() {
    let run = ono("alias hi = echo hello; hi");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "hello\n",
        "spec §6.5: an alias resolves at step 2 and runs its expansion, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_let_an_alias_shadow_an_external_command_of_the_same_name() {
    let run = ono("alias ls = echo shadowed; ls");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "shadowed\n",
        "ADR-0011: alias expansion happens before the native and PATH lookups, so an alias may \
         shadow a command — deliberately, and visibly through `explain`, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_expand_an_alias_exactly_once_so_a_self_reference_terminates() {
    let run = ono("alias echo = echo prefixed; echo x");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "prefixed x\n",
        "ADR-0011 step 3: an alias is expanded exactly once and re-resolved from step 1, so \
         `alias echo = echo …` reaches the real `echo` instead of looping, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_name_the_alias_and_its_expansion_when_explain_inspects_the_resolution() {
    let run = ono("alias hi = echo hello; explain hi");
    run.assert_success();
    let text = run.output();
    assert!(
        text.contains("alias") && text.contains("echo hello"),
        "spec §6.5 / ADR-0011: `explain` reports which resolution step matched — for an alias, \
         that it is one and what it expands to, got {text:?}"
    );
    assert!(
        !text.contains("resolves to nothing on PATH"),
        "an alias that exists is not `command not found`, got {text:?}"
    );
}

// --- now() and the timestamp literal (spec §6.3, language.yaml `builtin_functions`) --------

#[test]
fn should_evaluate_now_to_a_timestamp() {
    let run = ono("let t = (now()); $t | type | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[{\"fields\":null,\"schema\":null,\"type\":\"timestamp\"}]\n",
        "language.yaml `builtin_functions`: `now()` returns a timestamp, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_compare_a_file_time_against_now_plus_a_duration() {
    let dir = scratch();
    let file = dir.write("f.txt", "x");
    let run = ono(&format!(
        "get file {} | where modified < now() + 1d | count | to json",
        file.display()
    ));
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[1]\n",
        "spec §6.3 `modified < now() - 7d`: timestamp arithmetic with `now()` filters the file \
         written a moment ago, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        run.stderr().is_empty(),
        "no `no function to call` diagnostic for the specification's own function, got {:?}",
        run.stderr()
    );
}

#[test]
fn should_compare_a_file_time_against_an_iso_8601_literal() {
    let dir = scratch();
    let file = dir.write("f.txt", "x");
    let run = ono(&format!(
        "get file {} | where modified > 2000-01-01T00:00:00Z | count | to json",
        file.display()
    ));
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[1]\n",
        "spec §6.3 dates: an ISO-8601 literal is a timestamp operand, not a field path `T00`, \
         got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_type_an_iso_8601_literal_as_a_timestamp() {
    let run = ono("let t = (2000-01-01T00:00:00Z); $t | type | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[{\"fields\":null,\"schema\":null,\"type\":\"timestamp\"}]\n",
        "spec §6.3 / §10.2: `2000-01-01T00:00:00Z` is a timestamp value, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

// --- prefix assignment (spec §54, Unix muscle memory) ---------------------------------------

#[test]
fn should_set_a_variable_for_one_command_only_when_it_is_prefixed_to_the_command() {
    let run = ono(r#"FOO=bar sh -c 'echo $FOO'; sh -c 'echo "[$FOO]"'"#);
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "bar\n[]\n",
        "spec §54: `FOO=bar cmd` exports FOO to that command alone; the next command does not \
         see it, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_leave_the_shell_environment_alone_after_a_prefix_assignment() {
    let run = ono("FOO=bar sh -c 'echo $FOO'; get env FOO | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "bar\n[]\n",
        "spec §54: a prefix assignment is scoped to its command, so `get env FOO` afterwards \
         finds nothing, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

// --- blocks under each (spec §19.4, ono.data.each) -----------------------------------------

#[test]
fn should_map_every_item_through_a_block_in_each() {
    let run = ono("echo '[1,2,3]' | from json | each { @ * 2 } | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[2,4,6]\n",
        "ono.data.each: a block is evaluated per value with `@` bound to it (spec §19.4), got \
         stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_run_a_command_per_item_when_the_each_block_contains_one() {
    let run = ono("get process | where pid == 1 | each { echo @.pid }");
    run.assert_success();
    assert_eq!(
        run.stdout().lines().next(),
        Some("1"),
        "spec §19.4 `each {{ restart service @ }}`: a block with a command runs it once per item \
         with `@` bound, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        !run.stderr().contains("E0402"),
        "running a block is the evaluator's job, not a missing provider capability, got {:?}",
        run.stderr()
    );
}

// --- string arithmetic (spec §6.3 string operations) ----------------------------------------

#[test]
fn should_concatenate_strings_when_let_adds_two_of_them() {
    let run = ono(r#"let s = "a" + "b"; $s | to json"#);
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[\"ab\"]\n",
        "spec §6.3 string operations: `+` on two strings concatenates them, got stdout {:?} \
         stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

// --- sort without a key (ono.data.sort) -----------------------------------------------------

#[test]
fn should_sort_scalars_by_themselves_when_no_key_is_given() {
    let run = ono("echo '[3,1,2]' | from json | sort | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[1,2,3]\n",
        "ono.data.sort: a stream of bare scalars sorts by identity when no key is given, got \
         stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_sort_strings_by_themselves_when_no_key_is_given() {
    let run = ono(r#"echo '["b","a"]' | from json | sort | to json"#);
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[\"a\",\"b\"]\n",
        "ono.data.sort: identity is the default key for strings too, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_sort_scalars_descending_when_only_the_direction_is_given() {
    let run = ono("echo '[3,1,2]' | from json | sort desc | to json");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "[3,2,1]\n",
        "ono.data.sort `direction: asc|desc`: a bare `desc` with no key is the direction over \
         the identity key, not a field named `desc`, got stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

// --- kill %N on the job table (spec §18.1, §18.4) -------------------------------------------

#[test]
fn should_terminate_an_external_job_when_kill_names_it_by_job_number() {
    // The sleeper closes its copies of the test harness pipes so the run ends when the shell does.
    let run = ono("sleep 30 >/dev/null 2>&1 &; kill %1; sleep 0.2; jobs");
    run.assert_success();
    assert!(
        !run.stderr().contains("/usr/bin/kill"),
        "spec §18.1: `%1` is a job spec the shell understands; it must not reach /usr/bin/kill, \
         got {:?}",
        run.stderr()
    );
    assert!(
        !run.output().contains("running"),
        "the killed job is no longer running in the job table, got {:?}",
        run.output()
    );
}

#[test]
fn should_stop_a_native_job_when_kill_names_it_by_job_number() {
    let run = ono("watch process --every 200ms &; kill %1; sleep 0.2; jobs");
    run.assert_success();
    assert!(
        !run.stderr().contains("/usr/bin/kill"),
        "spec §18.4: a backgrounded live view is a job, and `kill %1` is how it is stopped, \
         got {:?}",
        run.stderr()
    );
    assert!(
        !run.output().contains("running"),
        "the killed watch is no longer running in the job table, got {:?}",
        run.output()
    );
}

// --- `let` and block scope (ADR-0119) ---------------------------------------------------------

#[test]
fn should_rebind_an_enclosing_binding_when_let_names_it_inside_a_loop_body() {
    // ADR-0119: `let` on a name an enclosing scope already binds rebinds that binding, the way
    // every shell's assignment does — otherwise a counter loop can never terminate.
    let run = Shell::new()
        .args([
            "-c",
            "let i = 0; while $i < 3 { echo $i; let i = $i + 1 }; echo done",
        ])
        .timeout(std::time::Duration::from_secs(10))
        .run();
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "0\n1\n2\ndone\n",
        "`let i = $i + 1` in the body advances the `i` the condition reads, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_rebind_an_enclosing_binding_when_let_names_it_inside_an_if_branch() {
    let run = ono("let n = 1; if true { let n = 2 }; echo $n");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "2\n",
        "ADR-0119: the branch's `let n` rebinds the enclosing `n`, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_keep_a_name_first_bound_inside_a_block_local_to_that_block() {
    let run = ono("if true { let inner = 9 }; echo \"<$inner>\"");
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "<>\n",
        "ADR-0119: a name introduced inside a block does not outlive it, got {:?}",
        run.stdout()
    );
}

#[test]
fn should_let_a_function_body_rebind_a_binding_of_the_calling_scope() {
    // Shell-like, as ADR-0119 fixes it: a function has no `local`; a parameter of the same name
    // is the only thing that shadows.
    let run = ono(
        "fn bump() { let n = $n + 1 }; fn keep(n: Int = 0) { let n = 99 }; let n = 1; bump; keep 5; echo $n",
    );
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "2\n",
        "`bump` rebinds the caller's `n`; `keep`'s `let n` rebinds its own parameter, got {:?}",
        run.stdout()
    );
}
