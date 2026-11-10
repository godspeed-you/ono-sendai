//! A loaded KUANG/11 package is a [`Provider`] in the shell's own `ProviderRegistry`.
//!
//! ADR-0582 wired the read path: `get <contributed target>` reaches `provider.query` and comes
//! back with records carrying the declared schema and host-stamped provenance. It reached no
//! further — the registry held no entry for a contributed target, so everything that asks the
//! registry rather than the command path could not see one.
//!
//! ADR-0583 closes that: every target a loaded package contributes becomes one registered
//! `Provider`, so `for_target`, `provider_for`, `snapshot` and `resolve` answer for it the way
//! they answer for a built-in provider or for a mounted remote one.
//!
//! Two layers of proof, because the boundary has two sides. The shell side is the real `ono`
//! binary: `help get <target>` prints the providers registered for a command's target (spec
//! §15.2), so it says out loud whether the registry holds one. The provider side is the
//! `Provider` contract itself, driven against a genuinely loaded instance of the SDK's example
//! package — records, refusals and cancellation are outcomes of that contract, and nothing
//! smaller than a running package proves them.

// The core build of #127 leaves this tier out (ADR-0910, ADR-0913).
#![cfg(feature = "kuang")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_cli::plugin_provider::PluginProvider;
use ono_kuang_protocol::Capability;
use ono_kuang_protocol::{HealthState, Manifest};
use ono_kuang_supervisor::{LoadConfig, LoadedPlugin, Policy, Supervisor};
use ono_pipeline::StreamEvent;
use ono_provider_api::{Action, ObjectId, Provider, ProviderRegistry, Query, Selector};
use ono_value::Value;

mod support;
use support::{echo_package_manifest, ono_with_plugins};

const ECHO: &str = "dev.example.echo";
const ITEM_SCHEMA: &str = "dev.example.echo.item/1";

const TARGETS: &str = r#"
targets:
  - name: echo-item
    schema: dev.example.echo.item/1
    summary: Items the example package provides.
    identity_doc: Two observations are the same item when their `seq` matches.
"#;

fn example_binary() -> PathBuf {
    ono_testkit::ono_binary()
        .parent()
        .expect("the target directory")
        .join("kuang-example-plugin")
}

fn lay_out_package(root: &Path, id: &str) {
    let package = root.join(id);
    std::fs::create_dir_all(package.join("runtime")).expect("the runtime directory");
    std::fs::write(package.join("manifest.yaml"), echo_package_manifest(id)).expect("the manifest");
    std::fs::create_dir_all(package.join("contributions")).expect("the contributions directory");
    std::fs::write(package.join("contributions/targets.yaml"), TARGETS).expect("the document");
    ono_testkit::executable_copy(example_binary(), &package.join("runtime/echo"));
}

fn plugin_home() -> ono_testkit::Scratch {
    let scratch = ono_testkit::scratch();
    lay_out_package(&scratch.path().join("plugins"), ECHO);
    scratch
}

/// A genuinely loaded instance of the example package, with the credit window the test wants.
///
/// `queue_depth` is the credit a stream starts with (spec §31.15). One means the package is
/// always waiting for demand after the first value, which is the state in which cancellation is
/// the only thing that can end its handler.
async fn load(scratch: &ono_testkit::Scratch, queue_depth: u32) -> Arc<LoadedPlugin> {
    load_under(scratch, queue_depth, Policy::deny_all()).await
}

/// The same, under the capability policy `policy` the broker enforces.
async fn load_under(
    scratch: &ono_testkit::Scratch,
    queue_depth: u32,
    policy: Policy,
) -> Arc<LoadedPlugin> {
    let manifest =
        Manifest::parse(&echo_package_manifest(ECHO)).expect("the fixture manifest is valid");
    let private = scratch.path().join("state");
    std::fs::create_dir_all(&private).expect("the private directory");
    let mut config = LoadConfig::new(example_binary(), manifest);
    config.private_dir = Some(private);
    config.limits.queue_depth = queue_depth;
    config.policy = policy;
    Arc::new(
        Supervisor::load(config)
            .await
            .expect("the example package loads"),
    )
}

/// The registry a shell would hold once that package is loaded.
fn registry_of(plugin: &Arc<LoadedPlugin>) -> ProviderRegistry {
    let mut registry = ProviderRegistry::new();
    for provider in PluginProvider::of(plugin) {
        registry.register(provider as Arc<dyn Provider>);
    }
    registry
}

// --- the shell side: the registry the session holds -------------------------------------------

#[test]
fn should_register_a_loaded_packages_target_as_a_provider() {
    // Spec §15.2: a command's help page names the providers registered for its target. That row
    // is the shell saying which provider would answer, so it is where registration is visible
    // without inventing a surface for the test.
    let home = plugin_home();
    let run = ono_with_plugins(&home, &format!("load plugin {ECHO}; help get echo-item"));
    run.assert_success();
    assert!(
        run.stdout().contains(&format!("plugin:{ECHO}")),
        "a loaded package must answer for its contributed target through the provider registry, \
         got {:?}",
        run.output()
    );
}

#[test]
fn should_not_claim_a_contributed_target_before_the_package_is_loaded() {
    // §31.68's order is `installed manifest -> registry placeholders -> first invocation ->
    // runtime load`. The command placeholder exists without the package running; the *provider*
    // cannot, because there is no instance to query. The help page must say so rather than
    // promise an answerer that is not there.
    let home = plugin_home();
    let run = ono_with_plugins(&home, "help get echo-item");
    run.assert_success();
    assert!(
        run.stdout().contains("none registered"),
        "an unloaded package has no provider yet, and the page must say so, got {:?}",
        run.output()
    );
}

// --- the provider side: the `Provider` contract ------------------------------------------------

#[tokio::test]
async fn should_answer_a_contributed_target_through_the_registry() {
    // The registry answers for the contributed noun exactly as it does for a built-in one, and
    // what comes back is the package's records: the schema the target declared (§31.23) and the
    // provenance the host stamped (§31.80), which a package cannot forge.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 16).await;
    let registry = registry_of(&plugin);

    let stream = registry
        .snapshot(&Query::target("echo-item"))
        .expect("the registry answers for a contributed target");
    let values = stream.collect().await.into_values();
    assert_eq!(values.len(), 3, "the example provider answers three items");

    let Value::Record(record) = &values[0] else {
        panic!(
            "a contributed target answers with records, got {:?}",
            values[0]
        );
    };
    assert_eq!(
        record.schema().id().to_string(),
        ITEM_SCHEMA,
        "the records must carry the schema the target declared"
    );
    assert_eq!(
        record.provenance().provider(),
        format!("plugin:{ECHO}"),
        "the host stamps provenance; a package cannot claim another source (§31.80)"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_declare_the_schema_the_contributed_target_answers_with() {
    // A registered provider says what it produces, because that is what the pipeline, `find` and
    // completion read before anything is queried.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 16).await;
    let registry = registry_of(&plugin);
    assert!(
        registry
            .schemas()
            .iter()
            .any(|schema| schema.id().to_string() == ITEM_SCHEMA),
        "the contributed schema must be one of the registry's, got {:?}",
        registry
            .schemas()
            .iter()
            .map(|schema| schema.id().to_string())
            .collect::<Vec<_>>()
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_a_target_the_package_does_not_contribute() {
    // A package answers for what it declared and nothing else. Claiming every noun because one
    // package happens to be loaded would make the registry's answer meaningless.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 16).await;
    let registry = registry_of(&plugin);

    let error = registry
        .provider_for("echo-nonexistent")
        .expect_err("an undeclared target is not answered");
    assert_eq!(
        error.code(),
        ono_core::ErrorCode::ResolveTargetNotFound,
        "an undeclared target is a resolution failure, got {error:?}"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_resolve_an_object_of_a_contributed_target() {
    // `resolve` is what `enter`, `follow` and every action path go through. A contributed target
    // that only answers `get` is a stream; one the registry can resolve is an object.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 16).await;
    let registry = registry_of(&plugin);

    let refs = registry
        .resolve("echo-item", &Selector::field("seq", Value::Int(2)))
        .await
        .expect("the contributed target resolves");
    assert_eq!(refs.len(), 1, "one item has seq 2, got {refs:?}");
    assert_eq!(
        refs[0].id().schema().to_string(),
        ITEM_SCHEMA,
        "the resolved object carries the contributed schema"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_cancel_the_packages_stream_when_the_query_is_cancelled() {
    // Spec §31.14: "cancellation is delivered to the plugin, not inferred from a stalled
    // stream". `echo-tick` emits until it is told to stop, and with a credit window of one it is
    // blocked waiting for demand the moment the consumer stops taking values. The SDK reports
    // `busy` for as long as a handler is running and `ready` once it has returned, so the
    // package coming back to `ready` — and answering the next query — is the delivery. An
    // invocation that had only been dropped would leave it blocked in that handler for good.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 1).await;
    let registry = registry_of(&plugin);

    // A limit, because `echo-tick` declares that its answer does not end (ADR-0588) and a
    // snapshot of such an answer is refused rather than read forever. The number is far larger
    // than this test will ever take: what is being proven is the cancellation, and a limit the
    // stream could reach would prove it by arriving at the end instead.
    let mut stream = registry
        .snapshot(&Query::target("echo-tick").limit(1_000_000))
        .expect("the endless target is answered");
    let first = tokio::time::timeout(std::time::Duration::from_secs(10), stream.recv())
        .await
        .expect("the first value arrives");
    assert!(
        matches!(first, Some(StreamEvent::Value(_))),
        "the endless target streams values, got {first:?}"
    );
    assert_eq!(
        probe(&plugin).await,
        HealthState::Busy,
        "the package is inside its provider handler while the query is open"
    );

    stream.cancel_token().cancel();
    drop(stream);

    let returned = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        while probe(&plugin).await != HealthState::Ready {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        returned.is_ok(),
        "a cancelled query must reach the package, so that its handler returns instead of \
         waiting for demand that will never come"
    );

    let answered = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        registry
            .snapshot(&Query::target("echo-item"))
            .expect("the package still answers")
            .collect()
            .await
            .into_values()
    })
    .await
    .expect("a cancelled query leaves the package able to answer the next one");
    assert_eq!(
        answered.len(),
        3,
        "the package answers the next query, which it could not do if the cancelled stream had \
         only been dropped"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

/// What the package says about itself right now (spec §31.35).
async fn probe(plugin: &Arc<LoadedPlugin>) -> HealthState {
    plugin
        .probe()
        .await
        .expect("the loaded package answers a health probe")
        .state
}

#[tokio::test]
async fn should_refuse_a_snapshot_of_an_answer_the_package_says_does_not_end() {
    // ADR-0588. `snapshot` is collected by `resolve` above, so an unbounded answer read here
    // would hang the registry rather than answer it — and hanging is the one outcome that is
    // never an answer. The refusal names the declaration, because the caller cannot see it.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 1).await;
    let registry = registry_of(&plugin);

    let refused = registry
        .snapshot(&Query::target("echo-tick"))
        .expect_err("an answer that does not end has no snapshot");

    assert!(
        refused.message().contains("does not end"),
        "the refusal says why, got {refused:?}"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_carry_the_querys_own_narrowing_to_the_package() {
    // The empty argument map this path used to pass was not a decision: every contributed target
    // answered a `resolve`, an `enter` or a re-read of a place as though nothing had been asked
    // of it. An unfiltered answer is not a visible failure, which is what made it worth fixing
    // rather than documenting. `echo-item` honours `--count`, so the count is the proof.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 1).await;
    let registry = registry_of(&plugin);

    let collected = registry
        .snapshot(&Query::target("echo-item").option("count", Value::Int(1)))
        .expect("the target is answered")
        .collect()
        .await;

    assert_eq!(
        collected.values().len(),
        1,
        "the package received `count`, so it emitted one item rather than all three"
    );
    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- acting on a contributed object (#224 (6), ADR-1012) ----------------------------------------

/// The identity of the place `uid` names, read from the package's own answer, as anything that
/// acts on an object first observes it.
async fn place(registry: &ProviderRegistry, uid: &str) -> ObjectId {
    let values = registry
        .snapshot(&Query::target("echo-place"))
        .expect("the registry answers for the places")
        .collect()
        .await
        .into_values();
    values
        .iter()
        .find_map(|value| match value {
            Value::Record(record) if record.get("uid") == Some(&Value::string(uid)) => {
                ObjectId::of(record)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("the package answers for `{uid}`, got {values:?}"))
}

fn granted_mutation() -> Policy {
    Policy::deny_all().grant(Capability::ProviderMutate, None)
}

#[tokio::test]
async fn should_carry_out_an_action_on_a_contributed_object_when_the_package_holds_provider_mutate()
{
    // The generic provider boundary every mutation in the shell goes through reaches a contributed
    // object: the package's declared `set` action on `echo-place`, under the `provider.mutate`
    // grant the broker checks before any package code runs, audited as loudly as a denial
    // (spec §31.37, ADR-0594).
    let scratch = ono_testkit::scratch();
    let plugin = load_under(&scratch, 16, granted_mutation()).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let outcome = registry
        .act(&Action::new("echo-place", "set", object).with("state", Value::string("ready")))
        .await
        .expect("a package holding provider.mutate can be asked to act");
    assert!(
        outcome.is_success(),
        "the package carried the action out, got {outcome:?}"
    );
    assert!(outcome.changed(), "a real action reports a change");
    assert!(
        plugin
            .audit()
            .iter()
            .any(|event| event.capability == "provider.mutate"
                && event.result == ono_kuang_protocol::AuditResult::Success),
        "the permitted mutation is in the trail"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_an_action_on_a_contributed_object_when_provider_mutate_is_not_granted() {
    // No grant, no attempt: the refusal is the broker's structured denial, an error rather than
    // a per-object outcome, because the provider could not attempt the action at all.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 16).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let refusal = registry
        .act(&Action::new("echo-place", "set", object).with("state", Value::string("ready")))
        .await
        .expect_err("an action under an ungranted provider.mutate is refused");
    assert_eq!(
        refusal.code().name(),
        "capability.denied",
        "the broker's denial, got {refusal:?}"
    );
    assert!(
        refusal.message().contains("provider.mutate"),
        "the denial names the capability, got {refusal:?}"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_report_an_action_the_system_refused_as_a_failed_outcome_for_that_object() {
    // Attempted and failed is an outcome, not an error (spec §11.5, §16.5): a bulk action has to
    // be able to say which object failed and why. The package's refusal names the object it was
    // handed, which is the proof that it was handed that object.
    let scratch = ono_testkit::scratch();
    let plugin = load_under(&scratch, 16, granted_mutation()).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let outcome = registry
        .act(&Action::new("echo-place", "set", object).with("state", Value::string("rejected")))
        .await
        .expect("an attempted action answers with an outcome");
    assert!(!outcome.is_success(), "the action failed, got {outcome:?}");
    let error = outcome.error().expect("a failed outcome carries its error");
    assert!(
        error
            .message()
            .contains("refused state `rejected` for `u-3`"),
        "the package's own reason, about the object it was handed, got {error:?}"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_dry_run_an_action_on_a_contributed_object_without_reporting_a_change() {
    let scratch = ono_testkit::scratch();
    let plugin = load_under(&scratch, 16, granted_mutation()).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let outcome = registry
        .act(
            &Action::new("echo-place", "set", object)
                .with("state", Value::string("ready"))
                .as_dry_run(),
        )
        .await
        .expect("the action declares a dry run");
    assert!(outcome.is_success(), "got {outcome:?}");
    assert!(!outcome.changed(), "a dry run changes nothing");

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_an_operation_the_package_declares_no_action_for() {
    // A package acts with the actions it declared and no others. `kill` is a word the generic
    // vocabulary has and this package does not answer to.
    let scratch = ono_testkit::scratch();
    let plugin = load_under(&scratch, 16, granted_mutation()).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let refusal = registry
        .act(&Action::new("echo-place", "kill", object))
        .await
        .expect_err("an undeclared operation is refused before anything runs");
    assert_eq!(
        refusal.code().name(),
        "provider.unsupported",
        "got {refusal:?}"
    );
    assert!(
        refusal.message().contains("kill"),
        "the refusal names the operation, got {refusal:?}"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- watching a contributed target (#224 (6), ADR-1013) -----------------------------------------

#[tokio::test]
async fn should_refuse_to_subscribe_to_a_contributed_target_and_say_why() {
    // The registry's `subscribe` is what `watch` and the live map ask. A contributed target has no
    // event vocabulary to answer with, and the refusal says that rather than the generic "cannot
    // watch for changes" a provider that simply never implemented it gives.
    let scratch = ono_testkit::scratch();
    let plugin = load(&scratch, 16).await;
    let registry = registry_of(&plugin);

    let Err(refusal) = registry.subscribe(&Query::target("echo-item")) else {
        panic!("a contributed target cannot be subscribed to");
    };
    assert_eq!(
        refusal.code().name(),
        "provider.unsupported",
        "got {refusal:?}"
    );
    assert!(
        refusal.message().contains("no event vocabulary"),
        "the refusal says why, got {refusal:?}"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

// --- an action's own risk, confirmation and defaults (#224 review R-F7/S-F4, ADR-1024) ----------

/// The example package in its `--destructive-action` mode, under `policy`.
async fn load_destructive_action(
    scratch: &ono_testkit::Scratch,
    policy: Policy,
) -> Arc<LoadedPlugin> {
    let manifest =
        Manifest::parse(&echo_package_manifest(ECHO)).expect("the fixture manifest is valid");
    let private = scratch.path().join("state");
    std::fs::create_dir_all(&private).expect("the private directory");
    let mut config = LoadConfig::new(example_binary(), manifest);
    config.args = vec!["--destructive-action".to_owned()];
    config.private_dir = Some(private);
    config.policy = policy;
    Arc::new(
        Supervisor::load(config)
            .await
            .expect("the example package loads"),
    )
}

#[tokio::test]
async fn should_refuse_a_destructive_action_on_a_contributed_object_that_was_not_confirmed() {
    // The action a package declared `destructive` is confirmed by the gate a destructive command
    // is (ADR-1001), whichever door reaches it: through the provider boundary as well as typed.
    let scratch = ono_testkit::scratch();
    let plugin = load_destructive_action(&scratch, granted_mutation()).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let refusal = registry
        .act(&Action::new("echo-place", "remove", object.clone()))
        .await
        .expect_err("an unconfirmed destructive action is not attempted");
    assert_eq!(
        refusal.code().name(),
        "safety.confirmation_required",
        "got {refusal:?}"
    );
    assert!(
        !plugin
            .audit()
            .iter()
            .any(|event| event.capability == "provider.mutate"),
        "nothing of the package ran: the broker never saw the invocation"
    );

    let confirmed = registry
        .act(&Action::new("echo-place", "remove", object).with("confirm", Value::Bool(true)))
        .await
        .expect("a confirmed destructive action is attempted");
    assert!(confirmed.is_success(), "got {confirmed:?}");

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_apply_an_actions_declared_defaults_and_honour_its_declared_dry_run_option() {
    // `dry_run` (underscore, as the Kubernetes provider spells it) defaults to true. The host
    // applies the default, so an action that does not say otherwise is a dry run and reports no
    // change; an action asked as a dry run sets the option the package declared, whatever its
    // spelling; `dry_run false` acts for real.
    let scratch = ono_testkit::scratch();
    let plugin = load_destructive_action(&scratch, granted_mutation()).await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;
    let confirmed =
        || Action::new("echo-place", "remove", object.clone()).with("confirm", Value::Bool(true));

    let defaulted = registry.act(&confirmed()).await.expect("attempted");
    assert!(
        defaulted.is_success() && !defaulted.changed(),
        "the declared default `dry_run: true` arrived, so nothing changed, got {defaulted:?}"
    );
    let dry = registry
        .act(&confirmed().as_dry_run())
        .await
        .expect("the action declares a dry-run option, in its own spelling");
    assert!(dry.is_success() && !dry.changed(), "got {dry:?}");
    let real = registry
        .act(&confirmed().with("dry_run", Value::Bool(false)))
        .await
        .expect("attempted");
    assert!(
        real.is_success() && real.changed(),
        "`dry_run false` is the real change, got {real:?}"
    );

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}

#[tokio::test]
async fn should_refuse_an_action_whose_hello_understates_its_risk() {
    // The hello declares `restart echo-place` `mutate` while it needs `filesystem.write`, which is
    // `destructive`. The registry refuses such a declaration (ADR-1000), and so does an action
    // that resolves to it (ADR-1002), even with every capability granted.
    let scratch = ono_testkit::scratch();
    let plugin = load_destructive_action(
        &scratch,
        granted_mutation().grant(Capability::FilesystemWrite, None),
    )
    .await;
    let registry = registry_of(&plugin);
    let object = place(&registry, "u-3").await;

    let refusal = registry
        .act(&Action::new("echo-place", "restart", object).with("confirm", Value::Bool(true)))
        .await
        .expect_err("an understated action is refused before it is attempted");
    assert_eq!(refusal.code().name(), "package.invalid", "got {refusal:?}");

    plugin
        .shutdown(ono_kuang_protocol::ShutdownReason::Unload)
        .await;
}
