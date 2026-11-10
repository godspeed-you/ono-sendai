//! The KUANG/11 host services that write: `history.append`, `relations.contribute`, and
//! `secrets.request` (spec §31.12, §31.20, §31.26; issue #203; ADR-1014 … ADR-1016) — each
//! through the real binary and the example package, at the protocol boundary a package uses.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0925).
#![cfg(feature = "kuang")]
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::{Scratch, Shell, scratch};

/// The example package under `<scratch>/plugins`, asking for what the services need.
fn plugin_home(home: &Scratch) {
    home.write(
        "plugins/dev.example.echo/manifest.yaml",
        r#"format: kuang-package/1
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
    - history.read
    - history.write
    - secret.use: {secrets: ["api-token"]}
network:
  outbound: none
"#,
    );
    let binary = ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin");
    let entry = home.path().join("plugins/dev.example.echo/runtime/echo");
    std::fs::create_dir_all(entry.parent().expect("a parent")).expect("the runtime directory");
    ono_testkit::executable_copy(&binary, &entry);
}

fn ono(home: &Scratch, script: &str) -> ono_testkit::Run {
    Shell::new()
        .args(["-c", script])
        .env("HOME", home.path().display().to_string())
        .env(
            "XDG_CONFIG_HOME",
            home.path().join("config").display().to_string(),
        )
        .env(
            "XDG_STATE_HOME",
            home.path().join("state").display().to_string(),
        )
        .env(
            "ONO_PLUGIN_PATH",
            home.path().join("plugins").display().to_string(),
        )
        .cwd(home.path())
        .run()
}

/// The grants a history test runs under.
const HISTORY_GRANTS: &str = "grant capability history.write --plugin dev.example.echo | count; \
     grant capability history.read --plugin dev.example.echo | count; \
     load plugin dev.example.echo";

// --- history.append (ADR-1015) ----------------------------------------------------------------

#[test]
fn should_keep_a_packages_entry_attributed_to_the_package_and_hand_it_to_a_reader() {
    // Spec §31.12: a package may append to history, and the host sets the attribution (§31.7
    // `history.write: {scope: plugin}`). A reader sees the entry and whose it is.
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        &format!(
            "{HISTORY_GRANTS}; echo:history-append --command \"synced 3 objects\" | to json; \
             echo:history --full true | to json"
        ),
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("appended"),
        "the host accepts the entry; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
    assert!(
        shown.contains("synced 3 objects") && shown.contains(r#"\"package\":\"dev.example.echo\""#),
        "the entry reaches a reader attributed to the package that wrote it; stdout {shown:?}"
    );
}

#[test]
fn should_keep_a_packages_entry_out_of_the_operators_recall() {
    // A package's entry is not something the operator typed: it must never be offered at the
    // prompt as a command to run again, nor crowd the operator's own history out (ADR-1015).
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        &format!("{HISTORY_GRANTS}; echo:history-append --command \"rm -rf ~/work\" | to json"),
    );
    run.assert_success();
    let operators =
        std::fs::read_to_string(home.path().join("state/ono/history.jsonl")).unwrap_or_default();
    assert!(
        !operators.contains("rm -rf"),
        "the operator's history, which the prompt recalls from, holds no package entry; got \
         {operators:?}"
    );
    // ... and the entry is still kept, across a restart, for a reader with `history.read`.
    let later = ono(
        &home,
        "grant capability history.read --plugin dev.example.echo | count; \
         load plugin dev.example.echo; echo:history --full true | to json",
    );
    assert!(
        later.stdout().contains("rm -rf ~/work"),
        "the package's entry outlives the session that wrote it; stdout {:?} stderr {:?}",
        later.stdout(),
        later.stderr()
    );
}

#[test]
fn should_refuse_an_entry_that_names_its_own_author() {
    // The contract: "Attribution to the package is set by the host and cannot be supplied." A
    // package naming the operator, another package or itself is refused, and nothing is kept.
    let home = scratch();
    plugin_home(&home);
    for forged in [
        r#"{\"command\": \"sudo reboot\", \"package\": \"dev.other.pkg\"}"#,
        r#"{\"command\": \"sudo reboot\", \"package\": null}"#,
        r#"{\"command\": \"sudo reboot\", \"session\": \"s1\"}"#,
    ] {
        let run = ono(
            &home,
            &format!(
                "{HISTORY_GRANTS}; try {{ echo:history-append --entry \"{forged}\" }} catch e \
                 {{ $e | select name | to json }}; echo:history --full true | to json"
            ),
        );
        let shown = run.stdout();
        assert!(
            shown.contains("runtime.schema_violation"),
            "an entry that supplies {forged} is refused; stdout {shown:?} stderr {:?}",
            run.stderr()
        );
        assert!(
            !shown.contains("sudo reboot"),
            "nothing of a refused entry is kept; stdout {shown:?}"
        );
    }
}

#[test]
fn should_refuse_a_malformed_entry() {
    let home = scratch();
    plugin_home(&home);
    for malformed in [
        r#"{\"status\": 0}"#,
        r#"{\"command\": \"\"}"#,
        r#"{\"command\": \"ok\", \"status\": \"fine\"}"#,
        r#"{\"command\": \"ok\", \"status\": 300}"#,
        r#"[\"not\", \"a record\"]"#,
    ] {
        let run = ono(
            &home,
            &format!(
                "{HISTORY_GRANTS}; try {{ echo:history-append --entry \"{malformed}\" }} catch e \
                 {{ $e | select name | to json }}"
            ),
        );
        assert!(
            run.stdout().contains("runtime.schema_violation"),
            "the entry {malformed} is not the record the contract declares; stdout {:?} stderr \
             {:?}",
            run.stdout(),
            run.stderr()
        );
    }
}

#[test]
fn should_redact_a_secret_in_a_packages_entry_before_it_is_kept() {
    // Spec §17.5 and ADR-0015 T8 hold for a package's entry as for the operator's.
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        &format!(
            "{HISTORY_GRANTS}; echo:history-append --command \"login --token=hunter2 prod\" | \
             count; echo:history --full true | to json"
        ),
    );
    run.assert_success();
    assert!(
        run.stdout().contains("prod") && !run.stdout().contains("hunter2"),
        "the secret is gone and the rest kept; stdout {:?}",
        run.stdout()
    );
    let mut written = String::new();
    for file in walk(&home.path().join("state")) {
        written.push_str(&std::fs::read_to_string(file).unwrap_or_default());
    }
    assert!(
        !written.contains("hunter2"),
        "no file the shell keeps holds the secret: {written:?}"
    );
}

#[test]
fn should_refuse_a_package_whose_history_permission_is_denied() {
    // `history.write` is extension-local authority, included without a question (K11P §7.1) —
    // which is exactly why the operator's decision to deny it must hold.
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        "set permission dev.example.echo history-write --decision deny | count; \
         load plugin dev.example.echo; try { echo:history-append --command \"unasked\" } catch e \
         { $e | select name | to json }; get audit --plugin dev.example.echo | where capability == \
         \"history.write\" | select result | to json",
    );
    let shown = run.stdout();
    assert!(
        shown.contains("capability.denied"),
        "a package denied `history.write` is refused; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
    assert!(
        shown.contains(r#"{"result":"denied"}"#),
        "and the refusal is in the trail; stdout {shown:?}"
    );
    let later = ono(
        &home,
        "grant capability history.read --plugin dev.example.echo | count; \
         load plugin dev.example.echo; echo:history --full true | to json",
    );
    assert!(
        !later.stdout().contains("unasked"),
        "nothing was kept; stdout {:?}",
        later.stdout()
    );
}

/// Every file under `root`.
fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}
