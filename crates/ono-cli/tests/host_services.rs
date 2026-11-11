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

mod support;

use ono_testkit::{Scratch, Shell, scratch};
use support::ono_with_packages_at_home as ono;

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

// --- relations.contribute (ADR-1014) ----------------------------------------------------------

/// The example package declaring the relation shape `process->process` named `feeds`.
fn relation_home(home: &Scratch) {
    plugin_home(home);
    home.write(
        "plugins/dev.example.echo/manifest.yaml",
        r#"format: kuang-package/1
package:
  id: dev.example.echo
  name: echo
  version: 0.1.0
  description: Emits what it is asked to emit, and knows one relation.
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
contributions:
  relations: ["process->process; name=feeds"]
capabilities:
  optional:
    - relation.write
    - relation.read
    - object.read
network:
  outbound: none
"#,
    );
}

/// Two processes of the test's own, told apart by name: the edge runs from a `sleep` to a `tail`.
fn two_processes() -> (ono_testkit::OwnedChild, ono_testkit::OwnedChild) {
    let spawn = |program: &str, arguments: &[&str]| {
        ono_testkit::OwnedChild::new(
            std::process::Command::new(program)
                .args(arguments)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .expect("a fixture process starts"),
        )
    };
    (
        spawn("sleep", &["120"]),
        spawn("tail", &["-f", "/dev/null"]),
    )
}

const RELATION_GRANTS: &str = "grant capability object.read --plugin dev.example.echo | count; \
     grant capability relation.read --plugin dev.example.echo | count; \
     load plugin dev.example.echo";

/// Where `follow feeds` from the `sleep` leaves the shell standing: the `look --json` that
/// follows it, which names the `tail` only if the contributed edge led there.
fn after_following(home: &Scratch, before: &str, from: u32) -> ono_testkit::Run {
    ono(
        home,
        &format!(
            "load plugin dev.example.echo; {before}; enter process {from}; \
             try {{ follow feeds }} catch e {{ $e | select name | to json }}; look --json"
        ),
    )
}

#[test]
fn should_hand_a_contributed_edge_back_through_relations_query_attributed_and_inferred() {
    // Spec §31.26: "Plugins MAY add graph edges", `provider` is set by the host, and "Inferred
    // relations MUST identify themselves as inferred". The host did not observe the edge, so even
    // a package's `exact` is not passed on as exact (spec v0.4 §36.2, ADR-1014).
    let home = scratch();
    relation_home(&home);
    let (from, to) = two_processes();
    let run = ono(
        &home,
        &format!(
            "{RELATION_GRANTS}; echo:relate --from {} --to {} --confidence exact --query true | \
             to json",
            from.id(),
            to.id()
        ),
    );
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("contributed 1"),
        "the host accepts the edge; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
    let edge = shown
        .lines()
        .flat_map(|line| line.split("\",\""))
        .find(|text| text.contains("process_to_process"))
        .unwrap_or_else(|| panic!("`relations.query` answers with the edge; stdout {shown:?}"));
    assert!(
        edge.contains(r#"\"provider\":\"dev.example.echo\""#),
        "the edge names the package that contributed it: {edge}"
    );
    assert!(
        edge.contains(r#"\"confidence\":\"inferred\""#),
        "an edge the host never observed is inferred, whatever the package claimed: {edge}"
    );
    assert!(
        edge.contains(&to.id().to_string()),
        "the edge leads to the process the package named: {edge}"
    );
}

#[test]
fn should_lead_follow_along_a_contributed_edge() {
    // The edges live in the spatial layer's contributed-relation registry (ADR-1006, ADR-1008),
    // so `look` counts them and `follow` walks them through the boundary every contributed
    // relation uses — the same merge `map` draws from.
    let home = scratch();
    relation_home(&home);
    let (from, to) = two_processes();
    let followed = after_following(
        &home,
        &format!("echo:relate --from {} --to {} | count", from.id(), to.id()),
        from.id(),
    );
    assert!(
        followed.stdout().contains(&format!("\"pid\":{}", to.id())),
        "`follow feeds` leads along the contributed edge to the `tail`; stdout {:?} stderr {:?}",
        followed.stdout(),
        followed.stderr()
    );
    let without = after_following(&home, "echo:emit | count", from.id());
    assert!(
        !without.stdout().contains(&format!("\"pid\":{}", to.id())),
        "with nothing contributed there is no edge to follow; stdout {:?}",
        without.stdout()
    );
}

#[test]
fn should_refuse_an_edge_that_names_its_own_provider() {
    // Spec §31.80, "plugin forges provenance": `provider` is the host's to set.
    let home = scratch();
    relation_home(&home);
    let (from, to) = two_processes();
    let edges = format!(
        r#"[{{\"from\": {{\"schema\": \"ono.process/1\", \"pid\": {f}}}, \"to\": {{\"schema\": \"ono.process/1\", \"pid\": {t}}}, \"relation\": \"dev.example.echo.process_to_process\", \"confidence\": \"exact\", \"provider\": \"linux.procfs\"}}]"#,
        f = from.id(),
        t = to.id()
    );
    let run = after_following(
        &home,
        &format!(
            "try {{ echo:relate --edges \"{edges}\" }} catch e {{ $e | select name | to json }}"
        ),
        from.id(),
    );
    assert!(
        run.stdout().contains("runtime.schema_violation"),
        "an edge that supplies its provider is refused; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        !run.stdout().contains(&format!("\"pid\":{}", to.id())),
        "nothing of the refused call can be followed; stdout {:?}",
        run.stdout()
    );
}

#[test]
fn should_refuse_a_malformed_edge() {
    let home = scratch();
    relation_home(&home);
    let process = |pid: &str| format!(r#"{{\"schema\": \"ono.process/1\", \"pid\": {pid}}}"#);
    let relation = "dev.example.echo.process_to_process";
    for malformed in [
        // no confidence: ono.graph-edge/1 makes it required, never defaulted (§22.2)
        format!(
            r#"[{{\"from\": {}, \"to\": {}, \"relation\": \"{relation}\"}}]"#,
            process("1"),
            process("2")
        ),
        format!(
            r#"[{{\"from\": {}, \"to\": {}, \"relation\": \"{relation}\", \"confidence\": \"certain\"}}]"#,
            process("1"),
            process("2")
        ),
        // an end that is not an object id
        format!(
            r#"[{{\"from\": 1, \"to\": {}, \"relation\": \"{relation}\", \"confidence\": \"inferred\"}}]"#,
            process("2")
        ),
        // an end of another kind than the relation runs between
        format!(
            r#"[{{\"from\": {{\"schema\": \"ono.user/1\", \"uid\": 0}}, \"to\": {}, \"relation\": \"{relation}\", \"confidence\": \"inferred\"}}]"#,
            process("2")
        ),
        // a field ono.graph-edge/1 does not have
        format!(
            r#"[{{\"from\": {}, \"to\": {}, \"relation\": \"{relation}\", \"confidence\": \"inferred\", \"weight\": 3}}]"#,
            process("1"),
            process("2")
        ),
        r#"{\"not\": \"a list\"}"#.to_owned(),
    ] {
        let run = ono(
            &home,
            &format!(
                "load plugin dev.example.echo; try {{ echo:relate --edges \"{malformed}\" }} \
                 catch e {{ $e | select name | to json }}"
            ),
        );
        assert!(
            run.stdout().contains("runtime.schema_violation"),
            "{malformed} is not a list of the edges the contract declares; stdout {:?} stderr \
             {:?}",
            run.stdout(),
            run.stderr()
        );
    }
}

#[test]
fn should_refuse_an_edge_along_a_relation_the_package_does_not_contribute() {
    // With `relation.write` granted unbounded, the broker lets any relation name through; the
    // host still keeps a package's edges on the relations that package declared (ADR-1014).
    let home = scratch();
    relation_home(&home);
    let run = ono(
        &home,
        "grant capability relation.write --plugin dev.example.echo | count; \
         load plugin dev.example.echo; try { echo:relate --relation process.parent } catch e { \
         $e | select name | to json }",
    );
    assert!(
        run.stdout().contains("runtime.schema_violation"),
        "an edge along a core relation is refused; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_refuse_a_package_whose_relation_permission_is_denied() {
    let home = scratch();
    relation_home(&home);
    let run = ono(
        &home,
        "set permission dev.example.echo relation-write --decision deny | count; \
         load plugin dev.example.echo; try { echo:relate } catch e { $e | select name | to json }",
    );
    assert!(
        run.stdout().contains("capability.denied"),
        "a package denied `relation.write` contributes nothing; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_bound_what_one_package_contributes_and_say_so() {
    // A package cannot grow the store without limit: past the bound the whole call is refused
    // with a structured error, nothing of it is kept, and asserting an edge again does not count
    // twice (ADR-1014).
    let home = scratch();
    relation_home(&home);
    let run = ono(
        &home,
        "load plugin dev.example.echo; \
         try { echo:relate --from 1 --to 100000 --repeat 1025 } catch e { $e | select name | to json }; \
         echo:relate --from 1 --to 100000 --repeat 1024 | to json; \
         echo:relate --from 1 --to 100000 --repeat 1024 | to json; \
         try { echo:relate --from 1 --to 200000 } catch e { $e | select name | to json }",
    );
    let shown = run.stdout();
    let refusals = shown.matches("resource.item_limit").count();
    assert_eq!(
        refusals,
        2,
        "1025 edges at once, and one edge beyond a full store, are refused; stdout {shown:?} \
         stderr {:?}",
        run.stderr()
    );
    assert_eq!(
        shown.matches("contributed 1024").count(),
        2,
        "the bound itself is accepted, and asserting the same edges again replaces them; stdout \
         {shown:?}"
    );
}

#[test]
fn should_refuse_an_edge_larger_than_the_bound_and_keep_nothing_of_its_call() {
    // The store is bounded in bytes as well as in edges: before, a package could keep 1024 edges
    // whose `metadata` filled each frame, and grew the shell past its own `memory_max` (v0.6.5
    // re-review F1). An edge beyond the per-edge bound refuses its whole call with a structured
    // error, and an edge within it is kept (ADR-1031).
    let home = scratch();
    relation_home(&home);
    let process = |pid: &str| format!(r#"{{\"schema\": \"ono.process/1\", \"pid\": {pid}}}"#);
    let relation = "dev.example.echo.process_to_process";
    let edge = |to: &str, bytes: usize| {
        format!(
            r#"{{\"from\": {}, \"to\": {}, \"relation\": \"{relation}\", \"confidence\": \"inferred\", \"metadata\": {{\"blob\": \"{}\"}}}}"#,
            process("1"),
            process(to),
            "x".repeat(bytes)
        )
    };
    let run = ono(
        &home,
        &format!(
            "load plugin dev.example.echo; \
             try {{ echo:relate --edges \"[{}, {}]\" }} catch e {{ $e | select name | to json }}; \
             echo:relate --edges \"[{}]\" | to json",
            edge("2", 16),
            edge("3", 8192),
            edge("4", 1024)
        ),
    );
    let shown = run.stdout();
    assert_eq!(
        shown.matches("resource.item_limit").count(),
        1,
        "a call carrying an edge beyond the bound is refused whole; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
    assert!(
        shown.contains("contributed 1"),
        "an edge within the bound is kept; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
}

#[test]
fn should_withdraw_a_packages_edges_when_it_is_unloaded() {
    let home = scratch();
    relation_home(&home);
    let (from, to) = two_processes();
    let run = after_following(
        &home,
        &format!(
            "echo:relate --from {} --to {} | count; unload plugin dev.example.echo | count",
            from.id(),
            to.id()
        ),
        from.id(),
    );
    assert!(
        run.stdout().contains(&format!("\"pid\":{}", from.id())),
        "the shell still stands at the `sleep`; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        !run.stdout().contains(&format!("\"pid\":{}", to.id())),
        "an unloaded package's edges leave with it; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

// --- secrets.request (ADR-1016) ---------------------------------------------------------------

#[test]
fn should_refuse_a_granted_secret_request_and_say_why_this_shell_keeps_no_store() {
    // There is no store for the operator to put a secret in, and no host call this build serves
    // would consume a handle, so a handle would name nothing usable. The refusal says so rather
    // than issuing one (ADR-1016).
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        r#"grant capability secret.use --plugin dev.example.echo --scope "secrets=api-token" | count; load plugin dev.example.echo; try { echo:secret --name api-token } catch e { $e | select name message | to json }"#,
    );
    let shown = run.stdout();
    assert!(
        shown.contains("provider.unavailable"),
        "no handle is issued; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
    assert!(
        shown.contains("keeps no secret store") && shown.contains("ADR-1016"),
        "the refusal says why, and where the decision is recorded; stdout {shown:?}"
    );
}

#[test]
fn should_never_answer_a_secret_request_from_the_environment() {
    // Spec §31.20: a package gains no bulk access to environment variables or secret stores
    // merely by running inside a shell. A variable that happens to share the secret's name is
    // not a store.
    let home = scratch();
    plugin_home(&home);
    let run = Shell::new()
        .args([
            "-c",
            r#"grant capability secret.use --plugin dev.example.echo --scope "secrets=api-token" | count; load plugin dev.example.echo; try { echo:secret --name api-token } catch e { $e | select name | to json }; get audit --plugin dev.example.echo | to json"#,
        ])
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
        .env("API_TOKEN", "hunter2-from-the-environment")
        .env("api-token", "hunter2-from-the-environment")
        .env("ONO_SECRET_API_TOKEN", "hunter2-from-the-environment")
        .cwd(home.path())
        .run();
    assert!(
        run.stdout().contains("provider.unavailable") && !run.stdout().contains("handle:"),
        "a variable named like the secret issues no handle; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
    assert!(
        !run.stdout().contains("hunter2") && !run.stderr().contains("hunter2"),
        "and its value appears nowhere; stdout {:?} stderr {:?}",
        run.stdout(),
        run.stderr()
    );
}

#[test]
fn should_refuse_a_secret_outside_the_granted_names_and_record_the_name_asked_for() {
    // capabilities.v1.yaml: "A request outside the list is `capability.scope_violation` and is
    // audited with the name that was asked for." No enumeration: the package learns nothing
    // about names it was not granted.
    let home = scratch();
    plugin_home(&home);
    let run = ono(
        &home,
        r#"grant capability secret.use --plugin dev.example.echo --scope "secrets=api-token" | count; load plugin dev.example.echo; try { echo:secret --name db-password } catch e { $e | select name | to json }; get audit --plugin dev.example.echo | where action == "secrets.request" | select result target | to json"#,
    );
    let shown = run.stdout();
    assert!(
        shown.contains("capability.scope_violation"),
        "a name outside the grant is refused at the broker; stdout {shown:?} stderr {:?}",
        run.stderr()
    );
    assert!(
        shown.contains("db-password"),
        "and the trail names what was asked for; stdout {shown:?}"
    );
}
