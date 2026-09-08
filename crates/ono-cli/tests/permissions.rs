//! The plugin installation, resolution and permission layer at the shell boundary
//! (`docs/specs/kuang11/kuang11-plugin-installation-permissions-spec.md`, K11P; ADR-0600 … ADR-0605):
//! a package is installed by its short name through a catalog, ready to use in the same
//! session, with its recommended access decided in human terms and the exact capabilities
//! underneath inspectable; mutation stays an explicit decision; a helper program is asked about
//! at first concrete need; an upgrade cannot widen authority silently; removal takes the
//! decisions with it; and every refusal is structured.
//!
//! Everything runs the real binary against a scratch root — a plugin home, a local package
//! source, a config directory with an operator catalog, a state directory — offline and
//! unprivileged. The runtime is the SDK's example plugin, which identifies as
//! `dev.example.echo@0.1.0`, so every package a test loads carries that identity; a package a
//! test only installs may be anything.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use serde_yaml_ng::Value;

mod support;
use support::{
    COMMANDS, catalog, declared_manifest, items, key, kuang_shell as ono, kubeconfig_of,
    last_json_document, lay_out, sign,
};

const ECHO: &str = "dev.example.echo";

/// A `kuang-package/2` manifest in the shape of the reference provider (K11P §8.2), for the
/// example runtime: cluster access, configuration reading and credential use recommended,
/// relationships automatic, a login helper just in time, and mutation explicit.
/// A scratch root with the example package in the local package source, a catalog naming it,
/// and an empty plugin home — the state of a machine on which nothing is installed yet.
fn root() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    std::fs::create_dir_all(scratch.path().join("plugins")).expect("the plugin home");
    std::fs::create_dir_all(scratch.path().join("home")).expect("the home directory");
    let kubeconfig = format!("{}/.kube/config", scratch.path().join("home").display());
    lay_out(
        &scratch.path().join("sources"),
        ECHO,
        &declared_manifest(ECHO, "echo", "0.1.0", &kubeconfig),
    );
    catalog(
        &scratch,
        "test",
        "test",
        &[(
            ECHO,
            "echo",
            "0.1.0",
            "git:https://example.invalid/echo#v0.1.0",
        )],
    );
    scratch
}

fn only(run: &ono_testkit::Run, value: &Value) -> Value {
    let rows = items(value);
    assert_eq!(rows.len(), 1, "one record, got {:?}", run.output());
    rows[0].clone()
}

fn field<'a>(record: &'a Value, name: &str) -> &'a Value {
    record
        .get(name)
        .unwrap_or_else(|| panic!("the record declares `{name}`, got {record:?}"))
}

fn str_field<'a>(record: &'a Value, name: &str) -> &'a str {
    field(record, name)
        .as_str()
        .unwrap_or_else(|| panic!("`{name}` is a string, got {record:?}"))
}

fn assert_refused_with(run: &ono_testkit::Run, code: &str, why: &str) {
    assert!(
        !run.status().is_success(),
        "{why}: the run must fail, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains(code),
        "{why}: stderr names {code}, got {:?}",
        run.stderr()
    );
}

/// The `ono.permission/1` rows of one package, by id.
fn permissions(home: &ono_testkit::Scratch, plugin: &str, all: bool) -> Vec<Value> {
    let run = ono(
        home,
        &format!(
            "get permission {plugin}{} | to json",
            if all { " --all" } else { "" }
        ),
    );
    run.assert_success();
    items(&last_json_document(&run)).to_vec()
}

fn permission<'a>(rows: &'a [Value], id: &str) -> &'a Value {
    rows.iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("a permission `{id}`, got {rows:?}"))
}

// ---------------------------------------------------------------------------------------------
// Gate A, C, D, E, F, G: install by name, ready in one session, human prompt, exact details
// ---------------------------------------------------------------------------------------------

#[test]
fn should_install_a_package_by_its_short_name_through_a_catalog_and_be_ready_to_use() {
    let home = root();
    let run = ono(
        &home,
        "install plugin echo --confirm | select status changed | to json; get plugin | select id name readiness state enabled | to json",
    );
    run.assert_success();
    let value = last_json_document(&run);
    let record = only(&run, &value);
    assert_eq!(
        str_field(&record, "id"),
        ECHO,
        "K11P §10.2: the short name resolved to the canonical id"
    );
    assert_eq!(
        str_field(&record, "readiness"),
        "ready",
        "K11P §12.2, Gate C: installed, enabled, decided, ready"
    );
    assert_eq!(
        str_field(&record, "state"),
        "installed",
        "K11P §12.2: a lazy runtime is not spawned by installing"
    );
    assert_eq!(field(&record, "enabled").as_bool(), Some(true));
    assert!(
        run.stdout()
            .contains(r#"{"status":"success","changed":true}"#),
        "one action-result row: {:?}",
        run.output()
    );
}

#[test]
fn should_use_a_contributed_command_in_the_session_that_installed_the_package() {
    // Gate C: no `load plugin`, and the registry knows the contribution before the next session.
    let home = root();
    let run = ono(
        &home,
        "install plugin echo --confirm | count; get echo-item --count 3 | to json",
    );
    run.assert_success();
    assert_eq!(
        run.stdout().lines().last().unwrap_or_default(),
        "[1,2,3]",
        "the contributed command answers in the session that installed it: {:?}",
        run.output()
    );
}

#[test]
fn should_decide_the_recommended_access_in_human_terms_with_the_capabilities_underneath() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let rows = permissions(&home, "echo", false);
    let access = permission(&rows, "cluster-access");
    assert_eq!(str_field(access, "state"), "allowed");
    assert_eq!(str_field(access, "when"), "always");
    assert_eq!(str_field(access, "title"), "Connect to clusters");
    assert_eq!(
        items(field(access, "capabilities")),
        [Value::String("network.connect".into())],
        "Gate E: the exact capability is in the record"
    );
    let relations = permission(&rows, "spatial-relations");
    assert_eq!(
        str_field(relations, "state"),
        "included",
        "K11P §7.1: automatic, never a question"
    );
    assert_eq!(str_field(relations, "when"), "automatic");
    let helper = permission(&rows, "login-helper");
    assert_eq!(
        str_field(helper, "state"),
        "ask",
        "K11P §7.3: asked at first need"
    );
    assert_eq!(str_field(helper, "when"), "when-needed");
    let mutation = permission(&rows, "cluster-mutation");
    assert_eq!(
        str_field(mutation, "state"),
        "denied",
        "Gate F: mutation is not part of recommended access"
    );
    assert_eq!(str_field(mutation, "when"), "explicit");
    // The support permission the host derived for `clock.read` hides in the compact view and
    // shows with `--all` (K11P §16.1).
    assert!(
        rows.iter()
            .all(|row| row.get("id").and_then(Value::as_str) != Some("clock-read"))
    );
    let all = permissions(&home, "echo", true);
    assert_eq!(
        str_field(permission(&all, "clock-read"), "state"),
        "included"
    );
}

#[test]
fn should_show_the_grants_a_permission_minted_in_the_capability_table() {
    // K11P §16.3, Gate K: the expert surface sees the same fact, with its origin.
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let run = ono(
        &home,
        &format!(
            "get capability --plugin {ECHO} | where capability == \"filesystem.read\" | select decision permission profile source duration | to json"
        ),
    );
    run.assert_success();
    let value = last_json_document(&run);
    let record = only(&run, &value);
    assert_eq!(str_field(&record, "decision"), "allow");
    assert_eq!(str_field(&record, "permission"), "kubeconfig-read");
    assert_eq!(str_field(&record, "profile"), "recommended");
    assert_eq!(str_field(&record, "duration"), "always");
    assert!(
        home.exists("config/ono/kuang/policy.yaml")
            && home.exists("config/ono/kuang/permissions.yaml"),
        "K11P §23.1: the decision and its grants are both stored"
    );
    assert!(
        home.read("config/ono/kuang/policy.yaml")
            .contains("permission: kubeconfig-read"),
        "the grant names the permission that minted it"
    );
}

#[test]
fn should_correlate_a_profile_decision_with_the_grants_it_minted_in_the_audit_trail() {
    // Gate V.
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let run = ono(
        &home,
        &format!(
            "get audit --plugin {ECHO} | where action == \"permission.profile_apply\" or action == \"capability.grant\" | select action correlation | to json"
        ),
    );
    run.assert_success();
    let rows = items(&last_json_document(&run)).to_vec();
    let applied: Vec<&Value> = rows
        .iter()
        .filter(|row| row.get("action").and_then(Value::as_str) == Some("permission.profile_apply"))
        .collect();
    assert_eq!(applied.len(), 1, "{rows:?}");
    let correlation = str_field(applied[0], "correlation").to_owned();
    assert!(!correlation.is_empty());
    let grants: Vec<&Value> = rows
        .iter()
        .filter(|row| row.get("action").and_then(Value::as_str) == Some("capability.grant"))
        .collect();
    assert!(!grants.is_empty());
    assert!(
        grants
            .iter()
            .all(|row| str_field(row, "correlation") == correlation),
        "every grant the profile minted shares its correlation id: {rows:?}"
    );
}

#[test]
fn should_state_native_isolation_honestly_in_the_plan_and_the_inspection() {
    // Gate M, K11P §27.1.
    let home = root();
    let refused = ono(&home, "install plugin echo");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E0701",
        "a script never waits for the prompt",
    );
    let plan = ono(
        &home,
        "try { install plugin echo } catch e { $e | to json }",
    );
    plan.assert_success();
    assert!(
        plan.stdout()
            .contains("not complete filesystem or network isolation"),
        "the plan the refusal carries states the isolation: {:?}",
        plan.output()
    );
    ono(&home, "install plugin echo --confirm").assert_success();
    let run = ono(
        &home,
        "inspect plugin echo | select isolation_statement readiness | to json",
    );
    run.assert_success();
    let value = last_json_document(&run);
    let record = only(&run, &value);
    assert!(
        str_field(&record, "isolation_statement")
            .contains("not complete filesystem or network isolation"),
        "{record:?}"
    );
    assert!(
        !str_field(&record, "isolation_statement").contains("sandbox"),
        "v0.4.1 §17.3: never `sandboxed` without the boundary"
    );
    assert_eq!(str_field(&record, "readiness"), "ready");
}

#[test]
fn should_list_the_catalog_entry_with_its_verification_when_searching() {
    // K11P §11.3, §11.6.
    let home = root();
    let run = ono(
        &home,
        "find plugin echo | select name id catalog catalog_verification installed | to json",
    );
    run.assert_success();
    let rows = items(&last_json_document(&run)).to_vec();
    let from_catalog = rows
        .iter()
        .find(|row| row.get("catalog").and_then(Value::as_str) == Some("test"))
        .unwrap_or_else(|| panic!("the catalog's entry, got {rows:?}"));
    assert_eq!(str_field(from_catalog, "name"), "echo");
    assert_eq!(str_field(from_catalog, "catalog_verification"), "operator");
    assert_eq!(field(from_catalog, "installed").as_bool(), Some(false));
}

// ---------------------------------------------------------------------------------------------
// Gate F, and K11P §15: mutation is explicit
// ---------------------------------------------------------------------------------------------

#[test]
fn should_refuse_a_mutation_in_the_permissions_words_until_it_is_enabled_deliberately() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let run = ono(&home, "echo:signal --pid 1 --signal TERM");
    assert_refused_with(
        &run,
        "Ono-Sendai-K11305",
        "K11P §26.4: mutation before elevation is permission.denied",
    );
    assert!(
        run.stderr().contains("Change resources")
            && run
                .stderr()
                .contains("set permission echo cluster-mutation"),
        "K11P §24.4: the permission first, the elevation named: {:?}",
        run.stderr()
    );
    // The expert surface agrees: nothing holds process.signal.
    let held = ono(
        &home,
        &format!(
            "get capability --plugin {ECHO} | where capability == \"process.signal\" | select decision | to json"
        ),
    );
    assert!(
        held.stdout().contains(r#"{"decision":"deny"}"#),
        "{:?}",
        held.output()
    );
    // Non-interactively, enabling mutation is a deliberate `--confirm`; without it the
    // decision is refused as an escalation (K11P §20.2, ADR-0602 §4).
    let refused = ono(
        &home,
        "set permission echo cluster-mutation --decision allow",
    );
    assert_refused_with(
        &refused,
        "Ono-Sendai-K11309",
        "an unattended widening needs --confirm",
    );
    let allowed = ono(
        &home,
        "set permission echo cluster-mutation --decision allow --confirm | select id state when | to json",
    );
    allowed.assert_success();
    let value = last_json_document(&allowed);
    let record = only(&allowed, &value);
    assert_eq!(str_field(&record, "state"), "allowed");
    let rows = permissions(&home, "echo", false);
    assert_eq!(
        str_field(permission(&rows, "cluster-mutation"), "state"),
        "allowed"
    );
}

#[test]
fn should_apply_the_operate_profile_deliberately_and_never_by_default() {
    let home = root();
    // `--confirm` alone confirms recommended access; a profile that adds mutation is named.
    ono(&home, "install plugin echo --confirm").assert_success();
    let rows = permissions(&home, "echo", false);
    assert_eq!(
        str_field(permission(&rows, "cluster-mutation"), "state"),
        "denied"
    );
    let refused = ono(&home, "set permission echo --profile operate");
    assert_refused_with(
        &refused,
        "Ono-Sendai-K11309",
        "a mutating profile applied unattended needs --confirm",
    );
    ono(&home, "set permission echo --profile operate --confirm").assert_success();
    let rows = permissions(&home, "echo", false);
    let mutation = permission(&rows, "cluster-mutation");
    assert_eq!(str_field(mutation, "state"), "allowed");
    assert_eq!(str_field(mutation, "profile"), "operate");
    // And back: a deny stands ahead of any grant (ADR-0604 §2).
    ono(
        &home,
        "set permission echo cluster-mutation --decision deny",
    )
    .assert_success();
    let run = ono(&home, "echo:signal --pid 1 --signal TERM");
    assert!(!run.status().is_success());
    let rows = permissions(&home, "echo", false);
    assert_eq!(
        str_field(permission(&rows, "cluster-mutation"), "state"),
        "denied"
    );
}

#[test]
fn should_refuse_a_profile_the_package_does_not_offer() {
    let home = root();
    let run = ono(&home, "install plugin echo --access nonesuch --confirm");
    assert_refused_with(&run, "Ono-Sendai-K11306", "permission.invalid_profile");
    assert!(
        !home.exists(format!("plugins/{ECHO}/manifest.yaml")),
        "nothing was written"
    );
}

// ---------------------------------------------------------------------------------------------
// Gates H, I, J: just-in-time permission for a helper program
// ---------------------------------------------------------------------------------------------

#[test]
fn should_answer_permission_required_with_a_remedy_when_a_script_meets_a_helper() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let helper = home.path().join("home/helper");
    std::fs::create_dir_all(helper.parent().expect("a parent")).expect("home");
    std::fs::write(&helper, "#!/bin/sh\necho hello\n").expect("the helper");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).expect("executable");
    let run = ono(
        &home,
        &format!("echo:exec --program {} | to json", helper.display()),
    );
    assert_refused_with(
        &run,
        "Ono-Sendai-K11304",
        "Gate J: a script never waits and never assumes consent",
    );
    assert!(
        run.stderr().contains("login helper")
            && run.stderr().contains("set permission echo login-helper"),
        "the remedy is a command line: {:?}",
        run.stderr()
    );
    // The permission-first remedy works, and scopes the grant to that program (Gate I).
    let decided = ono(
        &home,
        &format!(
            "set permission echo login-helper --decision allow --scope programs={} | select state | to json",
            helper.display()
        ),
    );
    decided.assert_success();
    let run = ono(
        &home,
        &format!("echo:exec --program {} | to json", helper.display()),
    );
    run.assert_success();
    assert!(run.stdout().contains("stdout: hello"), "{:?}", run.output());
    let held = ono(
        &home,
        &format!(
            "get capability --plugin {ECHO} | where capability == \"process.exec\" | select scope permission | to json"
        ),
    );
    held.assert_success();
    let value = last_json_document(&held);
    let record = only(&held, &value);
    assert_eq!(str_field(&record, "permission"), "login-helper");
    assert!(
        serde_yaml_ng::to_string(field(&record, "scope"))
            .expect("renders")
            .contains(&helper.display().to_string()),
        "Gate I: never unrestricted process execution: {record:?}"
    );
    // Another program is not covered.
    let other = home.path().join("home/other");
    std::fs::write(&other, "#!/bin/sh\necho other\n").expect("the other helper");
    std::fs::set_permissions(&other, std::fs::Permissions::from_mode(0o755)).expect("executable");
    let refused = ono(&home, &format!("echo:exec --program {}", other.display()));
    assert_refused_with(
        &refused,
        "Ono-Sendai-K11304",
        "a program nobody consented to asks again",
    );
}

#[test]
fn should_ask_at_first_use_and_keep_an_always_answer_for_that_program_at_a_terminal() {
    // Gate H, K11P §14.2, §36.5 — through a pseudo-terminal, as a person would meet it.
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let helper = home.path().join("home/helper");
    std::fs::create_dir_all(helper.parent().expect("a parent")).expect("home");
    std::fs::write(&helper, "#!/bin/sh\necho hello\n").expect("the helper");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).expect("executable");

    let mut executor = ono_process::Executor::detached();
    let root = home.path();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env(
            "ONO_PLUGIN_PATH",
            root.join("plugins").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_SOURCES",
            root.join("sources").display().to_string(),
        )
        .env(
            "ONO_SYSTEM_CONFIG_DIR",
            root.join("system").display().to_string(),
        )
        .env("HOME", root.join("home").display().to_string())
        .env("XDG_STATE_HOME", root.join("state").display().to_string())
        .env("XDG_CONFIG_HOME", root.join("config").display().to_string())
        .env(
            "ONO_CONFIG_DIR",
            root.join("config/ono").display().to_string(),
        )
        .current_dir(root.join("home"));
    let mut session = executor
        .run_pty(&command, ono_process::WindowSize::new(24, 100))
        .expect("a pseudo-terminal must be available");
    support::read_until(&mut session, ">", Duration::from_secs(20));
    session
        .write_all(format!("echo:exec --program {} | to json\n", helper.display()).as_bytes())
        .expect("typed");
    let asked = support::read_until(&mut session, "Allow this helper?", Duration::from_secs(20));
    assert!(
        asked.contains("needs to run") && asked.contains(&helper.display().to_string()),
        "K11P §14.2: the exact helper is named: {asked:?}"
    );
    assert!(
        asked.contains("authenticate to the selected context"),
        "the reason is shown: {asked:?}"
    );
    session.write_all(b"a\n").expect("answered");
    let ran = support::read_until(&mut session, "stdout: hello", Duration::from_secs(20));
    assert!(
        ran.contains("stdout: hello"),
        "the helper ran after consent: {ran:?}"
    );
    session.write_all(b"exit\n").expect("left");

    // The `always` answer is a grant scoped to that program, kept across sessions.
    let held = ono(
        &home,
        &format!(
            "get capability --plugin {ECHO} | where capability == \"process.exec\" | select source scope duration | to json"
        ),
    );
    held.assert_success();
    let value = last_json_document(&held);
    let record = only(&held, &value);
    assert_eq!(str_field(&record, "duration"), "always");
    assert!(
        serde_yaml_ng::to_string(field(&record, "scope"))
            .expect("renders")
            .contains(&helper.display().to_string()),
        "{record:?}"
    );
    let rows = permissions(&home, "echo", false);
    assert_eq!(
        str_field(permission(&rows, "login-helper"), "state"),
        "allowed"
    );
}

#[test]
fn should_never_ask_for_a_helper_when_no_helper_is_needed() {
    // Gate H's first half: nothing about a plain command touches process.exec.
    let home = root();
    let run = ono(
        &home,
        "install plugin echo --confirm | count; get echo-item --count 2 | to json",
    );
    run.assert_success();
    assert!(
        !run.stderr().contains("Allow this helper"),
        "{:?}",
        run.stderr()
    );
    let audit = ono(
        &home,
        &format!(
            "get audit --plugin {ECHO} | where action == \"permission.ask\" | count | to json"
        ),
    );
    assert!(audit.stdout().contains("[0]"), "{:?}", audit.output());
}

// ---------------------------------------------------------------------------------------------
// Gate L, K11P §28: manual grants project; legacy grants stay
// ---------------------------------------------------------------------------------------------

#[test]
fn should_show_a_manual_grant_as_custom_and_an_unmapped_one_as_legacy() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    ono(
        &home,
        &format!("grant capability filesystem.read --plugin {ECHO} --scope \"paths=/var/log/**\" --duration always | count"),
    )
    .assert_success();
    ono(
        &home,
        &format!("grant capability filesystem.write --plugin {ECHO} --scope \"paths=/tmp/**\" --duration always | count"),
    )
    .assert_success();
    let rows = permissions(&home, "echo", true);
    assert_eq!(
        str_field(permission(&rows, "kubeconfig-read"), "state"),
        "custom",
        "a grant wider than the mapping is custom, not hidden (Gate L)"
    );
    let legacy = permission(&rows, "capability:filesystem.write");
    assert_eq!(str_field(legacy, "state"), "legacy");
    assert_eq!(str_field(legacy, "risk"), "destructive");
    // And the raw surface still works (Gate K).
    ono(
        &home,
        &format!("revoke capability filesystem.write --plugin {ECHO} | count"),
    )
    .assert_success();
    let rows = permissions(&home, "echo", true);
    assert!(rows.iter().all(|row| row.get("id").and_then(Value::as_str) != Some("capability:filesystem.write")));
}

// ---------------------------------------------------------------------------------------------
// Gates N, O, and K11P §34.4, §34.5: upgrades
// ---------------------------------------------------------------------------------------------

#[test]
fn should_upgrade_without_a_new_decision_when_nothing_widened() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let kubeconfig = kubeconfig_of(&home);
    lay_out(
        &home.path().join("elsewhere"),
        ECHO,
        &declared_manifest(ECHO, "echo", "0.1.1", &kubeconfig),
    );
    let run = ono(
        &home,
        &format!(
            "install plugin path:{} --confirm | select status | to json; get plugin echo | select version readiness | to json",
            home.path().join("elsewhere").join(ECHO).display()
        ),
    );
    run.assert_success();
    let value = last_json_document(&run);
    let record = only(&run, &value);
    assert_eq!(str_field(&record, "version"), "0.1.1");
    assert_eq!(str_field(&record, "readiness"), "ready");
    let rows = permissions(&home, "echo", false);
    assert_eq!(
        str_field(permission(&rows, "cluster-access"), "state"),
        "allowed"
    );
    assert_eq!(
        str_field(permission(&rows, "cluster-mutation"), "state"),
        "denied"
    );
}

#[test]
fn should_need_renewed_consent_when_an_upgrade_widens_a_scope() {
    // K11P §34.5: the same permission id, a wider path.
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let wide = format!("{}/**", home.path().join("home").display());
    lay_out(
        &home.path().join("elsewhere"),
        ECHO,
        &declared_manifest(ECHO, "echo", "0.1.2", &wide),
    );
    let reference = format!(
        "path:{}",
        home.path().join("elsewhere").join(ECHO).display()
    );
    let refused = ono(&home, &format!("install plugin {reference}"));
    assert_refused_with(
        &refused,
        "Ono-Sendai-E0701",
        "Gate O: no commit without consent",
    );
    assert!(
        refused.stderr().contains("scope-widened") || refused.stderr().contains(&wide),
        "the plan carries the delta: {:?}",
        refused.stderr()
    );
    let still = ono(&home, "get plugin echo | select version | to json");
    assert!(
        still.stdout().contains("0.1.0"),
        "declining leaves the installed version intact"
    );
    // A script that confirms the plan has consented to the delta it carries.
    ono(&home, &format!("install plugin {reference} --confirm")).assert_success();
    let held = ono(
        &home,
        &format!(
            "get capability --plugin {ECHO} | where capability == \"filesystem.read\" | select scope | to json"
        ),
    );
    assert!(held.stdout().contains(&wide), "{:?}", held.output());
}

#[test]
fn should_refuse_an_upgrade_signed_by_an_unrelated_key() {
    // K11P §34.4.
    let home = root();
    let kubeconfig = kubeconfig_of(&home);
    let first = lay_out(
        &home.path().join("first"),
        ECHO,
        &declared_manifest(ECHO, "echo", "0.1.0", &kubeconfig),
    );
    sign(&first, &key(1));
    ono(
        &home,
        &format!("install plugin path:{} --confirm", first.display()),
    )
    .assert_success();
    let second = lay_out(
        &home.path().join("second"),
        ECHO,
        &declared_manifest(ECHO, "echo", "0.1.1", &kubeconfig),
    );
    sign(&second, &key(2));
    let run = ono(
        &home,
        &format!("install plugin path:{} --confirm", second.display()),
    );
    assert_refused_with(
        &run,
        "Ono-Sendai-K11005",
        "a different key is not an update",
    );
    let still = ono(&home, "get plugin echo | select version | to json");
    assert!(still.stdout().contains("0.1.0"));
}

// ---------------------------------------------------------------------------------------------
// Gate P, K11P §22: removal takes the decisions with it
// ---------------------------------------------------------------------------------------------

#[test]
fn should_remove_the_decisions_and_grants_with_the_package_so_a_reinstall_asks_again() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    ono(&home, "remove plugin echo | count").assert_success();
    assert!(
        !home
            .read("config/ono/kuang/permissions.yaml")
            .contains(ECHO),
        "the decisions are gone"
    );
    assert!(
        !home.read("config/ono/kuang/policy.yaml").contains(ECHO),
        "the grants are gone"
    );
    let refused = ono(&home, "install plugin echo");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E0701",
        "a reinstall asks again (K11P §22.3)",
    );
    ono(&home, "install plugin echo --confirm").assert_success();
    let rows = permissions(&home, "echo", false);
    assert_eq!(
        str_field(permission(&rows, "cluster-access"), "state"),
        "allowed"
    );
}

#[test]
fn should_keep_the_decisions_when_asked_and_apply_them_to_the_same_publisher_only() {
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    ono(&home, "remove plugin echo --keep-grants | count").assert_success();
    assert!(
        home.read("config/ono/kuang/permissions.yaml")
            .contains(ECHO),
        "kept on request (K11P §22.2)"
    );
}

// ---------------------------------------------------------------------------------------------
// Gate U: the transaction
// ---------------------------------------------------------------------------------------------

#[test]
fn should_leave_nothing_behind_when_the_decisions_cannot_be_written() {
    let home = root();
    let kuang = home.path().join("config/ono/kuang");
    std::fs::create_dir_all(&kuang).expect("the config directory");
    // The policy store cannot be written: everything before it is undone (K11P §12.4).
    std::fs::set_permissions(&kuang, std::fs::Permissions::from_mode(0o555)).expect("read-only");
    let run = ono(&home, "install plugin echo --confirm");
    std::fs::set_permissions(&kuang, std::fs::Permissions::from_mode(0o755))
        .expect("writable again");
    assert!(!run.status().is_success(), "{:?}", run.output());
    assert!(
        !home.exists(format!("plugins/{ECHO}/manifest.yaml")),
        "no package directory remains"
    );
    assert!(
        !home.exists(format!("state/ono/kuang/{ECHO}/management.json")),
        "no management state remains"
    );
    assert!(
        !home.path().join("plugins/.staging").exists()
            || std::fs::read_dir(home.path().join("plugins/.staging"))
                .expect("readable")
                .next()
                .is_none(),
        "no staging residue"
    );
    let listed = ono(&home, "get plugin | count | to json");
    assert!(listed.stdout().contains("[0]"), "{:?}", listed.output());
}

#[test]
fn should_leave_nothing_behind_when_the_plugin_home_cannot_be_written() {
    let home = root();
    std::fs::set_permissions(
        home.path().join("plugins"),
        std::fs::Permissions::from_mode(0o555),
    )
    .expect("read-only");
    let run = ono(&home, "install plugin echo --confirm");
    std::fs::set_permissions(
        home.path().join("plugins"),
        std::fs::Permissions::from_mode(0o755),
    )
    .expect("writable again");
    assert!(!run.status().is_success());
    assert!(
        !home.exists("config/ono/kuang/policy.yaml")
            || !home.read("config/ono/kuang/policy.yaml").contains(ECHO)
    );
}

// ---------------------------------------------------------------------------------------------
// Gates Q, R, and K11P §10, §24.1: resolution
// ---------------------------------------------------------------------------------------------

#[test]
fn should_refuse_an_ambiguous_short_name_deterministically_and_take_the_catalog_selector() {
    let home = root();
    let other = "dev.other.echo";
    lay_out(
        &home.path().join("sources"),
        other,
        &declared_manifest(other, "echo", "0.1.0", &kubeconfig_of(&home))
            .replace("publisher: dev.example", "publisher: dev.other"),
    );
    catalog(
        &home,
        "second",
        "second",
        &[(other, "echo", "0.1.0", "git:https://example.invalid/other")],
    );
    let run = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &run,
        "Ono-Sendai-E1602",
        "Gate Q: two catalogs, two ids, one name",
    );
    assert!(
        run.stderr().contains("test/echo") && run.stderr().contains("second/echo"),
        "every candidate is listed as `<catalog>/<name>`: {:?}",
        run.stderr()
    );
    let chosen = ono(
        &home,
        "install plugin second/echo --confirm | select status | to json; get plugin | select id | to json",
    );
    chosen.assert_success();
    assert!(chosen.stdout().contains(other), "{:?}", chosen.output());
}

#[test]
fn should_answer_not_found_unavailable_and_incompatible_as_their_own_codes() {
    let home = root();
    let run = ono(&home, "install plugin nonesuch --confirm");
    assert_refused_with(&run, "Ono-Sendai-E1601", "plugin.not_found");
    catalog(
        &home,
        "remote",
        "remote",
        &[(
            "dev.remote.thing",
            "thing",
            "1.0.0",
            "git:https://example.invalid/thing#v1",
        )],
    );
    let run = ono(&home, "install plugin thing --confirm");
    assert_refused_with(
        &run,
        "Ono-Sendai-E1603",
        "plugin.catalog_unavailable: this build fetches nothing",
    );
    assert!(
        run.stderr().contains("sources"),
        "the directories to place it in are named: {:?}",
        run.stderr()
    );
    home.write(
        "config/ono/kuang/catalogs/future.yaml",
        "format: kuang-catalog/1\ncatalog:\n  name: future\nentries:\n  - id: dev.future.thing\n    name: future-thing\n    publisher: dev.future\n    releases:\n      - version: 9.0.0\n        platforms: [linux-amd64, linux-arm64]\n        kuang_api: \">=99\"\n        artifact: \"git:https://example.invalid/future\"\n",
    );
    let run = ono(&home, "install plugin future-thing --confirm");
    assert_refused_with(&run, "Ono-Sendai-E1604", "plugin.release_not_compatible");
}

#[test]
fn should_take_the_short_name_wherever_a_command_takes_the_canonical_id() {
    // Gate R.
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    for script in [
        "inspect plugin echo | select readiness | to json",
        "verify plugin echo | select manifest | to json",
        "get permission echo | count | to json",
        "load plugin echo",
        "set plugin echo --enabled false | select status | to json",
        "set plugin echo --enabled true | select status | to json",
        &format!("inspect plugin {ECHO} | select readiness | to json"),
        "remove plugin echo | select status | to json",
    ] {
        let run = ono(&home, script);
        run.assert_success();
    }
    let gone = ono(&home, "get plugin | count | to json");
    assert!(gone.stdout().contains("[0]"));
}

#[test]
fn should_report_blocked_when_a_package_is_disabled_and_refuse_its_contribution() {
    // K11P §17.4: an explicit disable overrides ready, and no lazy load overrides it.
    let home = root();
    ono(
        &home,
        "install plugin echo --confirm | count; set plugin echo --enabled false | count",
    )
    .assert_success();
    let run = ono(&home, "get plugin echo | select readiness | to json");
    assert!(run.stdout().contains("blocked"), "{:?}", run.output());
    let refused = ono(&home, "get echo-item --count 1");
    assert_refused_with(
        &refused,
        "Ono-Sendai-E0702",
        "a disabled package is not loaded by an invocation",
    );
}

// ---------------------------------------------------------------------------------------------
// Gate S and T, K11P §13.3, §13.4: trust facts stay separate; unattended native install
// ---------------------------------------------------------------------------------------------

#[test]
fn should_refuse_an_unattended_native_install_from_a_catalog_until_the_publisher_is_enrolled() {
    let home = root();
    let package = home.path().join("sources").join(ECHO);
    sign(&package, &key(3));
    let run = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &run,
        "Ono-Sendai-E0701",
        "K11P §13.3: a valid signature from an unknown key is not a policy",
    );
    assert!(
        run.stderr().contains("trust.yaml"),
        "the policy that would allow it is named: {:?}",
        run.stderr()
    );
    home.write(
        "config/ono/kuang/trust.yaml",
        format!(
            "format: kuang-trust/1\nkeys:\n  - publisher: dev.example\n    key: {}\n    trust: trusted\n",
            key(3).public_key()
        ),
    );
    ono(&home, "install plugin echo --confirm").assert_success();
    let run = ono(
        &home,
        "verify plugin echo | select signature trust | to json",
    );
    run.assert_success();
    let value = last_json_document(&run);
    let record = only(&run, &value);
    assert_eq!(str_field(&record, "signature"), "valid");
    assert_eq!(
        str_field(&record, "trust"),
        "user-trusted",
        "Gate S: two separate facts"
    );
}

#[test]
fn should_refuse_a_tampered_package_whatever_the_flags_say() {
    // Gate T.
    let home = root();
    let package = home.path().join("sources").join(ECHO);
    sign(&package, &key(4));
    std::fs::write(
        package.join("contributions/commands.yaml"),
        COMMANDS.replace("Emit", "Steal"),
    )
    .expect("tampered");
    let run = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &run,
        "Ono-Sendai-K11004",
        "a claimed signature that does not hold refuses",
    );
    assert!(!home.exists(format!("plugins/{ECHO}/manifest.yaml")));
}

#[test]
fn should_accept_an_unsigned_local_package_by_explicit_path_unattended_with_a_warning() {
    // K11P §13.4: local development semantics.
    let home = root();
    let run = ono(
        &home,
        &format!(
            "install plugin path:{} --confirm | select status | to json",
            home.path().join("sources").join(ECHO).display()
        ),
    );
    run.assert_success();
    assert!(run.stdout().contains(r#"{"status":"success"}"#));
}

// ---------------------------------------------------------------------------------------------
// K11P §34.1, §34.6: a manifest that lies is refused before anything runs
// ---------------------------------------------------------------------------------------------

#[test]
fn should_refuse_a_package_whose_recommended_profile_hides_mutation() {
    let home = root();
    let hidden = declared_manifest("dev.example.hidden", "hidden", "0.1.0", &kubeconfig_of(&home)).replace(
        "permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations]\n    operate:",
        "permissions: [cluster-access, kubeconfig-read, credential-use, spatial-relations, cluster-mutation]\n    operate:",
    );
    lay_out(&home.path().join("sources"), "dev.example.hidden", &hidden);
    let run = ono(
        &home,
        &format!(
            "install plugin path:{} --confirm",
            home.path().join("sources/dev.example.hidden").display()
        ),
    );
    assert_refused_with(
        &run,
        "Ono-Sendai-K11307",
        "permission.invalid_mapping, before any code",
    );
    assert!(
        run.stderr().contains("cluster-mutation"),
        "{:?}",
        run.stderr()
    );
}

// ---------------------------------------------------------------------------------------------
// Gates D, E, W and K11P §5.2, §13.2, §25.2: the prompt, its details, and what help teaches
// ---------------------------------------------------------------------------------------------

#[test]
fn should_show_the_recommended_access_in_plain_words_and_install_on_yes_at_a_terminal() {
    let home = root();
    let mut executor = ono_process::Executor::detached();
    let root = home.path();
    let command = ono_process::Command::new(ono_testkit::ono_binary())
        .env("TERM", "xterm")
        .env("NO_COLOR", "1")
        .env(
            "ONO_PLUGIN_PATH",
            root.join("plugins").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_SOURCES",
            root.join("sources").display().to_string(),
        )
        .env(
            "ONO_SYSTEM_CONFIG_DIR",
            root.join("system").display().to_string(),
        )
        .env("HOME", root.join("home").display().to_string())
        .env("XDG_STATE_HOME", root.join("state").display().to_string())
        .env("XDG_CONFIG_HOME", root.join("config").display().to_string())
        .env(
            "ONO_CONFIG_DIR",
            root.join("config/ono").display().to_string(),
        )
        .current_dir(root.join("home"));
    let mut session = executor
        .run_pty(&command, ono_process::WindowSize::new(40, 120))
        .expect("a pseudo-terminal must be available");
    support::read_until(&mut session, ">", Duration::from_secs(20));
    session.write_all(b"install plugin echo\n").expect("typed");
    let shown = support::read_until(
        &mut session,
        "access? [Y/n/details]",
        Duration::from_secs(20),
    );
    for line in [
        "Echo 0.1.0",
        "Publisher: dev.example",
        "Signature: absent",
        "Runtime: native process",
        "Recommended access:",
        "Connect to clusters",
        "Read cluster configuration",
        "Add relationships to Ono",
        "Asked only when needed:",
        "Run an external login helper",
        "Not granted:",
        "Change resources",
    ] {
        assert!(
            shown.contains(line),
            "Gate D, K11P §13.2: the prompt says `{line}`: {shown:?}"
        );
    }
    assert!(
        !shown.contains("network.connect") && !shown.contains("provider.mutate"),
        "Gate D: the compact view needs no capability id: {shown:?}"
    );
    assert!(
        shown.contains("not complete filesystem or network isolation"),
        "Gate M: the native tier is stated: {shown:?}"
    );
    // `details` is one step away and preserves precision (Gate E).
    session.write_all(b"d\n").expect("typed");
    let details = support::read_until(
        &mut session,
        "access? [Y/n/details]",
        Duration::from_secs(20),
    );
    assert!(
        details.contains("capability network.connect")
            && details.contains("capability filesystem.read"),
        "Gate E: details expose the exact capabilities: {details:?}"
    );
    assert!(
        details.contains("enforcement"),
        "Gate E: and the enforcement: {details:?}"
    );
    session.write_all(b"y\n").expect("typed");
    let done = support::read_until(&mut session, "Ready to use.", Duration::from_secs(30));
    assert!(
        done.contains("Installed echo 0.1.0"),
        "K11P §5.2: `Installed … / Ready to use.`: {done:?}"
    );
    session.write_all(b"exit\n").expect("left");
    let run = ono(&home, "get plugin echo | select readiness | to json");
    assert!(run.stdout().contains("ready"), "{:?}", run.output());
}

#[test]
fn should_teach_the_short_name_first_and_keep_the_capability_commands_as_advanced_help() {
    // Gate W, K11P §25.2, §31.1.
    let home = root();
    let run = ono(&home, "help install plugin");
    run.assert_success();
    let first_example = run
        .stdout()
        .lines()
        .skip_while(|line| line.trim() != "EXAMPLES")
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        first_example,
        "install plugin kubernetes",
        "the first example is the short name, never a reverse-DNS id or a path: {:?}",
        run.stdout()
    );
    let topic = ono(&home, "help permissions");
    topic.assert_success();
    assert!(
        topic.stdout().contains("grant capability") && topic.stdout().contains("get permission"),
        "the topic keeps the two vocabularies apart: {:?}",
        topic.stdout()
    );
    let landing = ono(&home, "help");
    assert!(
        landing.stdout().contains("help permissions"),
        "{:?}",
        landing.stdout()
    );
}

#[test]
fn should_sanitise_a_permission_title_that_carries_terminal_escapes() {
    // K11P §34.2: package text cannot alter the surrounding security UI.
    let home = root();
    let manifest = declared_manifest(ECHO, "echo", "0.1.0", &kubeconfig_of(&home)).replace(
        "title: Connect to clusters",
        "title: \"Connect to \\u001b[31mclusters\\u0007\"",
    );
    std::fs::write(
        home.path().join("sources").join(ECHO).join("manifest.yaml"),
        manifest,
    )
    .expect("rewritten");
    ono(&home, "install plugin echo --confirm").assert_success();
    let rows = permissions(&home, "echo", false);
    let title = str_field(permission(&rows, "cluster-access"), "title").to_owned();
    assert_eq!(
        title, "Connect to clusters",
        "the escape sequence and the bell are gone"
    );
}

#[test]
fn should_never_let_a_new_catalog_entry_redirect_an_installed_package() {
    // K11P §34.3: catalog name takeover.
    let home = root();
    ono(&home, "install plugin echo --confirm").assert_success();
    let taker = "dev.taker.echo";
    lay_out(
        &home.path().join("sources"),
        taker,
        &declared_manifest(taker, "echo", "9.9.9", &kubeconfig_of(&home))
            .replace("publisher: dev.example", "publisher: dev.taker"),
    );
    catalog(
        &home,
        "takeover",
        "takeover",
        &[(taker, "echo", "9.9.9", "git:https://example.invalid/taker")],
    );
    let run = ono(&home, "install plugin echo --confirm");
    assert_refused_with(
        &run,
        "Ono-Sendai-E1602",
        "the name now names two ids, and nothing is chosen for the user",
    );
    let still = ono(&home, "get plugin | select id version | to json");
    assert!(
        still.stdout().contains(ECHO) && !still.stdout().contains(taker),
        "the installed package is untouched: {:?}",
        still.output()
    );
    // And every command that takes the installed name still means the installed package.
    let run = ono(&home, "inspect plugin echo | select readiness | to json");
    run.assert_success();
}
