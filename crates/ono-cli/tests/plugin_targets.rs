//! A KUANG/11 package with `roles: [provider]` answers `get <target>` through the same registry
//! the built-in providers answer through.
//!
//! Spec §31.23 (target and schema contribution), §31.64 (contributed ids enter the real
//! registries with origin `plugin(...)`), §31.80 (the host stamps provenance). The supervisor
//! already speaks `provider.query` and the SDK already lets a package answer it; what these
//! tests hold is the shell's integration step, which is what turns a contributed target from a
//! protocol capability into something a user can type.
//!
//! The example package `dev.example.echo` contributes the target `echo-item` and answers a
//! provider query for it with three records.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;
use support::{echo_plugin_home, last_json_document as last_json, ono_with_plugins};

const ECHO: &str = "dev.example.echo";

/// The package's declaration of the target it answers for, readable before any of its code runs
/// (spec §31.64, §31.68).
///
/// A target, not a `get`-shaped command. The difference is the whole point: a command is invoked
/// and answers whatever it likes, while a target is a noun the provider machinery answers for —
/// with the schema it declared, provenance the host stamps, and a route into the spatial model.
const TARGETS: &str = r#"
targets:
  - name: echo-refusal
    schema: dev.example.echo.item/1
    summary: A target that refuses without emitting anything.
    identity_doc: It never answers, so nothing identifies an answer.
  - name: echo-item
    schema: dev.example.echo.item/1
    summary: Items the example package provides.
    identity_doc: Two observations are the same item when their `seq` matches.
  - name: echo-tick
    schema: dev.example.echo.item/1
    summary: Items emitted until the query is cancelled.
    identity_doc: Two observations are the same tick when their `seq` matches.
    answer: unbounded
"#;

#[test]
fn should_name_a_handshake_target_the_document_does_not_declare_at_load() {
    // ADR-0598: a target word comes from the on-disk document (spec §31.68). The example package
    // contributes `echo-place` and `echo-zone` at the handshake, and this document declares
    // neither, so they answer through the provider path and are not words — and `load plugin`
    // says so, rather than accepting the asymmetry silently.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}"));
    run.assert_success();
    let line = run
        .stdout()
        .lines()
        .find(|line| line.contains("answerable, not spellable"))
        .unwrap_or_else(|| {
            panic!(
                "the load names the unspellable targets, got {:?}",
                run.output()
            )
        });
    assert!(
        line.contains("echo-place") && line.contains("echo-zone") && !line.contains("echo-item"),
        "the handshake-only targets are named and the declared one is not, got {line}"
    );
}

#[test]
fn should_answer_get_for_a_contributed_target() {
    // The whole point of a provider package: the target it contributes is a noun the user types,
    // not a command namespace they have to learn (spec §31.23, §35.1 of the Kubernetes spec).
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    let items = items.as_sequence().expect("a sequence of records");
    assert_eq!(items.len(), 3, "the example provider answers three items");
}

#[test]
fn should_stamp_a_contributed_records_provenance_with_the_package() {
    // §31.80: the host stamps provenance; a package cannot claim another source. A record that
    // arrived from a plugin must say so wherever it is inspected.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | take 1 | inspect | to json"),
    );
    run.assert_success();
    assert!(
        run.stdout().contains(&format!("plugin:{ECHO}")),
        "inspect must attribute the record to the package that produced it, got {:?}",
        run.output()
    );
}

#[test]
fn should_not_answer_a_target_the_package_does_not_contribute() {
    // A package answers for what it declared and nothing else. An undeclared target is a
    // resolution failure, never an empty success.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-nonexistent | to json"),
    );
    assert_ne!(
        run.status().code(),
        0,
        "an undeclared target must fail rather than answer emptily, got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_a_contributed_target_without_an_explicit_load() {
    // §31.68: `installed manifest -> registry placeholders -> first invocation -> runtime load`.
    // The declaration is what makes the noun typeable; loading is what the first use pays for.
    // A user should not have to know which package answers `get echo-item` in order to ask.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(&home, "get echo-item | to json");
    run.assert_success();
    let items = last_json(&run);
    assert_eq!(
        items.as_sequence().expect("a sequence").len(),
        3,
        "the package loaded on first use and answered, got {:?}",
        run.output()
    );
}

#[test]
fn should_answer_a_contributed_target_alike_typed_aliased_or_called_from_a_function() {
    // Issue #130, v0.6.1 §6, §28: a function body resolves `get <target>` the way the prompt
    // does, including a target an installed package contributes and nobody has loaded yet. The
    // call is followed by a stage — `kprod | select name` in the report — because that is the
    // shape that failed with `resolve.command_not_found`. Each form runs in a session of its own,
    // so each one is the first use that has to load the package.
    let home = echo_plugin_home(ECHO, TARGETS);
    let answer = |form: &str, script: &str| {
        let run = ono_with_plugins(&home, script);
        assert!(
            run.status().is_success(),
            "a contributed target {form} answers without an explicit load, got {:?}",
            run.output()
        );
        last_json(&run)
    };

    // Without arguments: the report's exact failure, `… is declared but this build implements
    // nothing for it`.
    let typed = answer("typed", "get echo-item | select seq | to json");
    assert_eq!(typed.as_sequence().expect("a sequence").len(), 3);
    for (form, script) in [
        (
            "aliased",
            "alias items = get echo-item; items | select seq | to json",
        ),
        (
            "called from a function",
            "fn items() { get echo-item }; items | select seq | to json",
        ),
    ] {
        assert_eq!(
            answer(form, script),
            typed,
            "the target {form} answers exactly what it answers typed"
        );
    }

    // With an option the body takes from a parameter's default, as `kprod` does.
    let typed = answer("typed", "get echo-item --count 2 | select seq | to json");
    assert_eq!(
        typed.as_sequence().expect("a sequence").len(),
        2,
        "`--count 2` reaches the provider, got {typed:?}"
    );
    for (form, script) in [
        (
            "aliased",
            "alias items = get echo-item; items --count 2 | select seq | to json",
        ),
        (
            "called from a function",
            "fn items(n: Int = 2) { get echo-item --count $n }; items | select seq | to json",
        ),
        (
            "called from a function whose body is a pipeline",
            "fn items(n: Int = 2) { get echo-item --count $n | where seq > 0 }; items | select seq | to json",
        ),
    ] {
        assert_eq!(
            answer(form, script),
            typed,
            "the target {form} answers exactly what it answers typed"
        );
    }
}

#[test]
fn should_carry_the_schema_the_target_declared() {
    // A target names its schema in the declaration, and the records that arrive must be of it.
    // This is what separates a provider answer from a command that happens to be spelled `get`.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | take 1 | inspect | to json"),
    );
    run.assert_success();
    assert!(
        run.stdout().contains("dev.example.echo.item/1"),
        "the record must carry the declared schema, got {:?}",
        run.output()
    );
}

#[test]
fn should_compose_a_contributed_target_with_the_pipeline() {
    // A contributed target is not a special case: it is a stream of typed records, so `where`
    // filters it and `select` projects it exactly as for a built-in provider.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | where seq > 1 | select label | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    let items = items.as_sequence().expect("a sequence");
    assert_eq!(
        items.len(),
        2,
        "two of three items have seq > 1, got {:?}",
        run.output()
    );
}

#[test]
fn should_carry_a_targets_options_into_the_provider_query() {
    // A contributed target is invoked with words like any other stage, and those words are the
    // only way a user can say *which* of something they want — a context, a namespace, a kind.
    // The first version of this route passed an empty option map, so every target answered as if
    // it had been asked with no arguments at all: not visibly broken, just permanently unfiltered.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item --count 1 | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    assert_eq!(
        items.as_sequence().expect("a sequence").len(),
        1,
        "`--count 1` must reach the provider handler, got {:?}",
        run.output()
    );
}

#[test]
fn should_report_a_refusing_target_rather_than_an_empty_success() {
    // A provider handler can fail *without emitting anything* — no cluster named, no credential,
    // no route — and the host learns of it from the invocation result rather than from a stream
    // event. Reading only the stream, that refusal is indistinguishable from "there are none",
    // which is the exact confusion §21.4 exists to prevent and the one this whole provider model
    // is built to avoid. Zero records and a failure is not zero records.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-refusal | to json"),
    );
    assert_ne!(
        run.status().code(),
        0,
        "a refused query must fail, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("refuses"),
        "the refusal's own message must reach the user, got {:?}",
        run.output()
    );
}

// --- a contributed target declares its own options (spec §31.23, ADR-0587) ---------------------

/// The same package, with the target declaring the option a user actually types.
///
/// `get k8s-pod --context prod` was the motivating case: a target answers *about* something, and
/// which something is the whole question. Without a declaration the word reaches the provider —
/// ADR-0582 saw to that — but nothing tells the user it exists, nothing types it, and nothing
/// applies a default. The declared default here is 1 while the package's own fallback is 3, so
/// only a default the host applied can produce one record.
const OPTION_TARGETS: &str = r#"
targets:
  - name: echo-precondition
    schema: dev.example.echo.item/1
    summary: A target that refuses because a precondition of its own is unmet.
    identity_doc: It never answers, so nothing identifies an answer.
  - name: echo-item
    schema: dev.example.echo.item/1
    summary: Items the example package provides.
    identity_doc: Two observations are the same item when their `seq` matches.
    options:
      - name: count
        type: int
        doc: How many items to answer with.
        default: 1
"#;

#[test]
fn should_show_a_contributed_targets_declared_option_in_its_help_page() {
    let home = echo_plugin_home(ECHO, OPTION_TARGETS);
    let run = ono_with_plugins(&home, "help get echo-item");
    run.assert_success();
    let shown = run.stdout();
    assert!(
        shown.contains("--count") && shown.contains("How many items to answer with."),
        "spec §31.23: a target's declared option is documented, got {shown:?}"
    );
}

#[test]
fn should_offer_a_contributed_targets_declared_option_when_completing() {
    let home = echo_plugin_home(ECHO, OPTION_TARGETS);
    let plugins = home.path().join("plugins");
    let mut shell = support::interactive_shell_with_plugins(&home, &plugins);
    let _ = support::read_until(&mut shell, "> ", std::time::Duration::from_secs(10));

    shell
        .write_all(b"get echo-item --co\t")
        .expect("the completion request");
    let seen = support::read_until(&mut shell, "--count", std::time::Duration::from_secs(10));
    assert!(
        seen.contains("--count"),
        "spec §31.86: a target's declared option completes; saw:\n{seen}"
    );

    shell.write_all(b"\x03").expect("abandon the line");
    shell.write_all(b"exit\n").expect("input");
    let _ = shell.wait();
}

#[test]
fn should_apply_a_contributed_targets_declared_default_when_the_option_is_absent() {
    let home = echo_plugin_home(ECHO, OPTION_TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-item | to json"),
    );
    run.assert_success();
    assert_eq!(
        last_json(&run).as_sequence().expect("a sequence").len(),
        1,
        "the declared default reaches the provider query, got {:?}",
        run.output()
    );
}

#[test]
fn should_name_a_packages_own_refusal_as_its_own_rather_than_a_host_policy() {
    // §31.79 had no code for the refusal a provider makes on a rule of its own, so a package
    // that declined because one of *its* preconditions was unmet had to borrow one that says
    // something untrue: `safety.policy_denied` claims a configured host policy, and there is no
    // configuration; `provider.unavailable` claims the external system did not answer, and it
    // was never asked; `provider.unsupported` claims an inability, and the package is perfectly
    // able. `contribution.refused` is the package speaking for itself.
    let home = echo_plugin_home(ECHO, OPTION_TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-precondition | to json"),
    );
    assert_ne!(
        run.status().code(),
        0,
        "a refused query must fail, got {:?}",
        run.output()
    );
    assert!(
        run.stderr().contains("Ono-Sendai-K11901"),
        "the package's own refusal carries its own code, got {:?}",
        run.output()
    );
}

#[test]
fn should_not_collect_an_answer_the_package_says_does_not_end() {
    // ADR-0588. `echo-tick` emits until it is cancelled. Before a target could declare that, the
    // host had one behaviour for every contributed answer — read it to the end — so a package
    // whose answer has no end never returned to the prompt at all. Declared unbounded, it is a
    // stream, and a stage that takes a prefix of a stream finishes.
    //
    // The assertion that matters is that this command *returns*. The row count is the second
    // thing: a run that hung would never get to compare it.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(
        &home,
        &format!("load plugin {ECHO}; get echo-tick | take 2 | to json"),
    );
    run.assert_success();
    let items = last_json(&run);
    let items = items.as_sequence().expect("a sequence of records");
    assert_eq!(
        items.len(),
        2,
        "a prefix of an endless answer is two records, and the endless part stops"
    );
}

#[test]
fn should_refuse_to_render_an_endless_answer_where_nobody_is_watching_it() {
    // The other half, and it is inherited rather than new: shell specification §18.3 shows a
    // live stream in place at a terminal, and a redirected run has nowhere to show one. The
    // refusal belongs to the pipeline and reaches a contributed target now that one can be
    // unbounded — which is the point of declaring it rather than writing a special case for
    // packages.
    let home = echo_plugin_home(ECHO, TARGETS);
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}; get echo-tick"));
    assert!(
        !run.status().is_success(),
        "an endless answer with no representation is refused, got {:?}",
        run.output()
    );
}
