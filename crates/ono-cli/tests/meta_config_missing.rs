//! Outcome tests for the meta family the wiki lists as missing: `resolve command`, `get config`
//! and `set config`, and the settings that nothing reads yet.
//!
//! Contracts: `docs/spec/commands/meta.yaml` (`ono.command.resolve`, `ono.config.get`,
//! `ono.config.set`), `docs/spec/schemas/config-setting.v1.yaml`, spec §6.5 (resolution order),
//! §13.3 (truncation is visible), §15.4 (discovery suggestions), §30 (configuration with
//! provenance), ADR-0010 (the five layers, `ONO_*` mapping, a bad setting never stops the shell)
//! and ADR-0011 (resolution order and namespaces).
//!
//! Every test asserts what a user sees — values through `to json`, exit status, structured
//! error codes, rendered output — never how the shell produces them (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::PathBuf;

use ono_testkit::{Scratch, Shell, scratch};
use serde_yaml_ng::Value;

/// Runs a one-liner in a shell that sees no configuration but its built-in defaults.
fn ono(script: &str) -> ono_testkit::Run {
    let dir = scratch();
    isolated(&dir).args(["-c", script]).run()
}

/// A shell whose configuration tree lives entirely in `dir`, so neither this machine's
/// `~/.config/ono` nor an `ONO_*` variable of the developer's environment leaks in (ADR-0010).
fn isolated(dir: &Scratch) -> Shell {
    Shell::new()
        .env("HOME", dir.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            dir.path().join("xdg").display().to_string(),
        )
        .env(
            "ONO_CONFIG_DIR",
            dir.path().join("ono").display().to_string(),
        )
        .env_remove("ONO_CONFIG")
        .env_remove("ONO_RENDER_TABLE_MAX_ROWS")
}

/// Parses a `to json` document (spec §33.5); JSON is YAML, so the YAML parser reads it.
fn rows(run: &ono_testkit::Run) -> Vec<Value> {
    let text = run.stdout().trim();
    let parsed: Value = serde_yaml_ng::from_str(text).unwrap_or_else(|error| {
        panic!(
            "`to json` must emit a JSON document, got {text:?} ({error}); stderr: {}",
            run.stderr()
        )
    });
    parsed
        .as_sequence()
        .unwrap_or_else(|| panic!("`to json` emits the stream as a JSON array, got {text:?}"))
        .clone()
}

/// The one record a single-value command emits.
fn single(run: &ono_testkit::Run) -> Value {
    let rows = rows(run);
    assert_eq!(
        rows.len(),
        1,
        "a single-value output is a one-element array (spec §33.5), got {rows:?}"
    );
    rows.into_iter().next().expect("one row")
}

fn field<'a>(record: &'a Value, name: &str) -> &'a Value {
    let value = &record[name];
    assert!(
        !value.is_null() || record.get(name).is_some(),
        "the record must carry the field `{name}`, got {record:?}"
    );
    value
}

fn text(record: &Value, name: &str) -> String {
    field(record, name)
        .as_str()
        .unwrap_or_else(|| panic!("`{name}` must be a string, got {record:?}"))
        .to_owned()
}

/// What a head word resolved to. `ono.command/1` has no dedicated resolution field in the
/// registry today; the deferred contract and ADR-0011 name the stages `keyword`, `function`,
/// `alias`, `native` and `external`, and this reads whichever of `kind` or `namespace` the
/// record uses to carry that name.
fn resolution_kind(record: &Value) -> String {
    record
        .get("kind")
        .or_else(|| record.get("namespace"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            panic!("spec §6.5 / ADR-0011: a resolution names its stage in `kind`, got {record:?}")
        })
        .to_owned()
}

/// The first executable named `program` on this test's `PATH`, as `which` would find it.
fn on_path(program: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH is set in a test environment");
    std::env::split_paths(&path)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("`{program}` must exist on PATH for this test"))
}

fn assert_not_the_placeholder(run: &ono_testkit::Run) {
    assert!(
        !run.stderr().contains("implements nothing"),
        "the command must do its job rather than answer with the not-implemented placeholder: {}",
        run.stderr()
    );
}

// --- resolve command, spec §6.5 and ADR-0011 ----------------------------------------------

#[test]
fn should_resolve_an_external_program_to_its_path_when_nothing_earlier_claims_the_name() {
    let run = ono("resolve command ls | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let record = single(&run);
    assert_eq!(
        resolution_kind(&record),
        "external",
        "spec §6.5 step 4: `ls` is an external executable on PATH, got {record:?}"
    );
    let expected = on_path("ls");
    let reported = text(&record, "path");
    assert!(
        std::path::Path::new(&reported) == expected
            || std::fs::canonicalize(&reported).ok() == std::fs::canonicalize(&expected).ok(),
        "ADR-0011: an external hit reports its absolute path; expected {}, got {reported}",
        expected.display()
    );
}

#[test]
fn should_resolve_a_native_verb_to_the_registry_when_the_head_word_is_one() {
    let run = ono("resolve command get | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let record = single(&run);
    assert_eq!(
        resolution_kind(&record),
        "native",
        "spec §6.5 step 3: `get` is a native Ono command, got {record:?}"
    );
    assert_eq!(
        text(&record, "verb"),
        "get",
        "ono.command/1 carries the verb the head word names, got {record:?}"
    );
}

#[test]
fn should_resolve_a_language_keyword_before_anything_else() {
    let run = ono("resolve command if | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let record = single(&run);
    assert_eq!(
        resolution_kind(&record),
        "keyword",
        "spec §6.5 step 1: `if` is a language control form, got {record:?}"
    );
}

#[test]
fn should_resolve_a_function_declared_in_the_same_script_before_the_registry_and_path() {
    let run = ono("fn hi() { echo hi }\nresolve command hi | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let record = single(&run);
    assert_eq!(
        resolution_kind(&record),
        "function",
        "spec §6.5 step 2: a user function shadows natives and PATH, got {record:?}"
    );
}

#[test]
fn should_report_command_not_found_with_suggestions_when_no_stage_answers() {
    let run = ono("resolve command lss | to json");
    assert_not_the_placeholder(&run);
    assert_ne!(
        run.status().code(),
        0,
        "a head word nothing answers to is an error, not a success"
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "spec §6.5 step 5 / ADR-0011: the failure is resolve.command_not_found: {}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("lss"),
        "the error names the word that was not found: {}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("did you mean") && run.stderr().contains("ls"),
        "spec §15.4 / ADR-0011: the error carries discovery suggestions, got: {}",
        run.stderr()
    );
    assert_eq!(
        run.stdout().trim(),
        "",
        "a resolution failure produces no record"
    );
}

#[test]
fn should_not_retry_another_namespace_when_the_head_word_forces_one() {
    // `ono:ls` asks for a native command only; ADR-0011 forbids falling back to PATH.
    let run = ono("resolve command ono:ls | to json");
    assert_not_the_placeholder(&run);
    assert!(
        run.stderr().contains("Ono-Sendai-E0101"),
        "ADR-0011: a qualified name that misses is resolve.command_not_found: {}",
        run.stderr()
    );
    assert!(
        !run.output().contains("/bin/ls"),
        "a forced namespace is never silently retried on PATH: {}",
        run.output()
    );
}

// --- get config, spec §30, ADR-0010 and config-setting.v1 ---------------------------------

const LAYERS: [&str; 5] = ["default", "system", "user", "environment", "invocation"];

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_list_every_setting_at_the_default_layer_when_no_config_file_exists() {
    let run = ono("get config | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let settings = rows(&run);
    assert!(
        !settings.is_empty(),
        "spec §30: the shell has built-in settings to report"
    );
    for setting in &settings {
        let key = text(setting, "key");
        let layer = text(setting, "layer");
        assert!(
            LAYERS.contains(&layer.as_str()),
            "config-setting.v1: `layer` is one of {LAYERS:?}, got {layer:?} for {key}"
        );
        assert_eq!(
            layer, "default",
            "ADR-0010: with no file and no ONO_* variable every value is the built-in default ({key})"
        );
        assert!(
            field(setting, "source").is_null() && field(setting, "line").is_null(),
            "config-setting.v1: the default layer has no file and no line ({key}): {setting:?}"
        );
        assert!(
            !text(setting, "type").is_empty(),
            "config-setting.v1: every setting declares its type ({key})"
        );
        assert!(
            setting.get("value").is_some(),
            "config-setting.v1: `value` is required ({key})"
        );
    }
    assert!(
        settings
            .iter()
            .any(|setting| text(setting, "key") == "render.table.max_rows"),
        "spec §30 names `render.table.max_rows`; the settings were {settings:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_return_one_typed_setting_when_given_an_exact_key() {
    let run = ono("get config render.table.max_rows | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(text(&setting, "key"), "render.table.max_rows");
    assert!(
        field(&setting, "value").as_i64().is_some(),
        "config-setting.v1: the value keeps its own type — an int, not a string: {setting:?}"
    );
    assert!(
        text(&setting, "type").contains("int"),
        "config-setting.v1: `type` is the declared type of the setting: {setting:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_return_the_settings_under_a_dotted_prefix() {
    let run = ono("get config safety. | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let settings = rows(&run);
    assert!(
        !settings.is_empty(),
        "spec §30 lists `safety` among the configuration domains"
    );
    for setting in &settings {
        assert!(
            text(setting, "key").starts_with("safety."),
            "meta.yaml: the selector is a dotted prefix, got {setting:?}"
        );
    }
    assert!(
        settings
            .iter()
            .any(|setting| text(setting, "key") == "safety.confirm.bulk_threshold"),
        "spec §30 names `safety.confirm.bulk_threshold`; got {settings:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_the_user_file_and_line_that_set_a_value() {
    let dir = scratch();
    let file = dir.write(
        "ono/config.ono",
        "# the user's configuration\nset config render.table.max_rows = 3\n",
    );
    let run = isolated(&dir)
        .args(["-c", "get config render.table.max_rows | to json"])
        .run();
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(
        field(&setting, "value").as_i64(),
        Some(3),
        "the file's value is the effective one: {setting:?}"
    );
    assert_eq!(
        text(&setting, "layer"),
        "user",
        "ADR-0010: `$ONO_CONFIG_DIR/config.ono` is the user layer: {setting:?}"
    );
    assert_eq!(
        text(&setting, "source"),
        file.display().to_string(),
        "spec §30: the user can see which file set the value: {setting:?}"
    );
    assert_eq!(
        field(&setting, "line").as_i64(),
        Some(2),
        "config-setting.v1: `line` is the line within `source`: {setting:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_the_environment_layer_when_an_ono_variable_sets_a_value() {
    let dir = scratch();
    let run = isolated(&dir)
        .env("ONO_RENDER_TABLE_MAX_ROWS", "4")
        .args(["-c", "get config render.table.max_rows | to json"])
        .run();
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(
        field(&setting, "value").as_i64(),
        Some(4),
        "ADR-0010: `ONO_RENDER_TABLE_MAX_ROWS` sets `render.table.max_rows`, typed as the setting declares: {setting:?}"
    );
    assert_eq!(
        text(&setting, "layer"),
        "environment",
        "spec §30: the user can see which environment variable set the value: {setting:?}"
    );
    assert!(
        field(&setting, "source").is_null(),
        "config-setting.v1: the environment layer has no file: {setting:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_report_the_invocation_layer_after_set_config_in_the_same_script() {
    let run =
        ono("set config render.table.max_rows = 2\nget config render.table.max_rows | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(
        field(&setting, "value").as_i64(),
        Some(2),
        "`set config` changes the setting in the current scope (meta.yaml): {setting:?}"
    );
    assert_eq!(
        text(&setting, "layer"),
        "invocation",
        "ADR-0010: a value set in the running shell is the invocation layer: {setting:?}"
    );
    assert!(
        field(&setting, "source").is_null() && field(&setting, "line").is_null(),
        "config-setting.v1: the invocation layer has no file and no line: {setting:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_let_a_later_layer_override_an_earlier_one_and_show_the_overridden_value_when_asked() {
    let dir = scratch();
    dir.write("ono/config.ono", "set config render.table.max_rows = 3\n");
    let shell = || isolated(&dir).env("ONO_RENDER_TABLE_MAX_ROWS", "4");

    let run = shell()
        .args(["-c", "get config render.table.max_rows | to json"])
        .run();
    assert_not_the_placeholder(&run);
    run.assert_success();
    let setting = single(&run);
    assert_eq!(
        (field(&setting, "value").as_i64(), text(&setting, "layer")),
        (Some(4), "environment".to_owned()),
        "ADR-0010: later layers override earlier ones: {setting:?}"
    );

    let run = shell()
        .args([
            "-c",
            "get config render.table.max_rows --overridden | to json",
        ])
        .run();
    run.assert_success();
    let document = run.stdout();
    assert!(
        document.contains("user") && document.contains('3'),
        "meta.yaml `--overridden`: the user layer's 3 is shown beside the effective value: {document}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_start_anyway_and_expose_the_problem_when_a_config_file_sets_an_unknown_key() {
    let dir = scratch();
    dir.write("ono/config.ono", "set config no.such.key = 1\n");
    let run = isolated(&dir).args(["-c", "echo started"]).run();
    run.assert_success();
    assert_eq!(
        run.stdout(),
        "started\n",
        "ADR-0010: a bad setting never stops the shell from starting"
    );
    assert!(
        run.stderr().contains("no.such.key"),
        "ADR-0010: the unknown key is reported as a diagnostic at startup: {:?}",
        run.stderr()
    );

    let run = isolated(&dir)
        .args(["-c", "get config --problems | to json"])
        .run();
    assert_not_the_placeholder(&run);
    run.assert_success();
    let problems = rows(&run);
    assert!(
        !problems.is_empty(),
        "meta.yaml `--problems`: the load diagnostics stay available as values"
    );
    assert!(
        run.stdout().contains("no.such.key"),
        "the problem names the offending key: {}",
        run.stdout()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_keep_the_earlier_layers_value_when_a_config_file_gives_the_wrong_type() {
    let dir = scratch();
    dir.write(
        "ono/config.ono",
        "set config render.table.max_rows = \"many\"\n",
    );
    let run = isolated(&dir)
        .args(["-c", "get config render.table.max_rows | to json"])
        .run();
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(
        text(&setting, "layer"),
        "default",
        "ADR-0010: a wrongly typed setting leaves the previous layer's value in force: {setting:?}"
    );
    assert!(
        field(&setting, "value").as_i64().is_some(),
        "the built-in default is still an int, not the rejected string: {setting:?}"
    );
    assert!(
        run.stderr().contains("Ono-Sendai-E0201") && run.stderr().contains("render.table.max_rows"),
        "ADR-0010: the rejected assignment is a structured type.mismatch diagnostic naming the key: {:?}",
        run.stderr()
    );
}

// --- set config, spec §30 and meta.yaml ----------------------------------------------------

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_answer_set_config_with_an_action_result() {
    let run = ono("set config render.table.max_rows = 2 | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let result = single(&run);
    assert_eq!(
        text(&result, "status"),
        "success",
        "meta.yaml: `set config` emits an ono.action-result/1 row: {result:?}"
    );
    assert_eq!(
        field(&result, "changed").as_bool(),
        Some(true),
        "the default was replaced, so the row says something changed: {result:?}"
    );
    assert!(
        run.stdout().contains("render.table.max_rows"),
        "action-result/1: `target` names what was changed: {}",
        run.stdout()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_reject_an_unknown_key_with_a_structured_error() {
    let run = ono("set config no.such.key = 1");
    run.assert_status(1);
    // Configuration is a record of declared, typed settings (config-setting.v1 carries the
    // declared `type` of each); a key that is not declared is a field the schema does not have,
    // which errors.yaml calls type.unknown_field. resolve.target_not_found is about the verb's
    // *target*, and `config` is a perfectly good target of `set`.
    assert!(
        run.stderr().contains("Ono-Sendai-E0202"),
        "errors.yaml: an undeclared setting is type.unknown_field: {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("no.such.key"),
        "the error names the key: {:?}",
        run.stderr()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_reject_a_value_of_the_wrong_type_with_type_mismatch() {
    let run = ono("set config render.table.max_rows = \"many\"");
    run.assert_status(1);
    assert!(
        run.stderr().contains("Ono-Sendai-E0201"),
        "errors.yaml: a string where the setting declares an int is type.mismatch: {:?}",
        run.stderr()
    );
    assert!(
        run.stderr().contains("render.table.max_rows"),
        "the error names the setting: {:?}",
        run.stderr()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_leave_the_setting_untouched_when_an_assignment_is_rejected() {
    let run = ono(
        "set config render.table.max_rows = \"many\"\nget config render.table.max_rows | to json",
    );
    assert_not_the_placeholder(&run);

    let setting = single(&run);
    assert_eq!(
        text(&setting, "layer"),
        "default",
        "a rejected `set config` changes nothing: {setting:?}"
    );
    assert!(
        field(&setting, "value").as_i64().is_some(),
        "the built-in int survives the rejected string: {setting:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_store_a_bytesize_setting_as_a_bytesize() {
    let run =
        ono("set config history.result_cache = 64MiB\nget config history.result_cache | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(
        field(&setting, "value").as_i64(),
        Some(64 * 1024 * 1024),
        "config-setting.v1: `64MiB` is a bytesize, serialised as its byte count, not the string \"64MiB\": {setting:?}"
    );
    assert!(
        text(&setting, "type").contains("byte"),
        "the declared type of `history.result_cache` is a byte size (spec §30): {setting:?}"
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_store_a_string_setting_as_a_string() {
    let run = ono("set config prompt.path = \"smart\"\nget config prompt.path | to json");
    assert_not_the_placeholder(&run);
    run.assert_success();

    let setting = single(&run);
    assert_eq!(
        field(&setting, "value").as_str(),
        Some("smart"),
        "spec §30: `set config prompt.path = \"smart\"` is read back as that string: {setting:?}"
    );
    assert_eq!(
        text(&setting, "type"),
        "string",
        "config-setting.v1: the declared type is reported: {setting:?}"
    );
}

// --- the settings do something: rendering reads render.table.max_rows (spec §13.3) ----------

/// The data rows of a rendered table, and the truncation marker if one was printed.
fn table_rows_and_marker(stdout: &str) -> (Vec<&str>, Option<&str>) {
    let mut lines: Vec<&str> = stdout.lines().filter(|line| !line.is_empty()).collect();
    assert!(
        lines.len() >= 2,
        "a rendered table has a header and at least one row, got {stdout:?}"
    );
    lines.remove(0); // the header
    let marker = lines
        .last()
        .copied()
        .filter(|line| line.contains("...") && line.contains("more"));
    if marker.is_some() {
        lines.pop();
    }
    (lines, marker)
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_truncate_the_rendered_table_at_the_configured_row_count() {
    // `get process` sees far more than two rows on any machine; the shell itself is one of them.
    let run = ono("set config render.table.max_rows = 2\nget process");
    run.assert_success();

    let (rows, marker) = table_rows_and_marker(run.stdout());
    assert!(
        rows.len() <= 2,
        "spec §30 / §13.3: `render.table.max_rows = 2` limits the rendered table to two data rows, got {} rows:\n{}",
        rows.len(),
        run.stdout()
    );
    assert!(
        marker.is_some(),
        "spec §13.3: truncation MUST be visible — a `... N more` line follows the rows:\n{}",
        run.stdout()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_apply_the_configured_row_count_to_an_explicit_table_too() {
    let run = ono("set config render.table.max_rows = 2\nget process | format table");
    run.assert_success();

    let (rows, marker) = table_rows_and_marker(run.stdout());
    assert!(
        rows.len() <= 2 && marker.is_some(),
        "spec §13.3: `format table` without `--max-rows` truncates at the configured count, visibly:\n{}",
        run.stdout()
    );
}

#[test]
#[ignore = "REASON: RED suite for a component v0.2 declares but does not build yet; un-ignored by the increment that delivers it (docs/STATE.md)"]
fn should_truncate_the_rendered_table_when_the_user_file_sets_the_row_count() {
    let dir = scratch();
    dir.write("ono/config.ono", "set config render.table.max_rows = 1\n");
    let run = isolated(&dir).args(["-c", "get process"]).run();
    run.assert_success();

    let (rows, marker) = table_rows_and_marker(run.stdout());
    assert!(
        rows.len() <= 1 && marker.is_some(),
        "spec §30: a setting from the user's config.ono is read by the renderer, and truncation is visible (§13.3):\n{}",
        run.stdout()
    );
}
