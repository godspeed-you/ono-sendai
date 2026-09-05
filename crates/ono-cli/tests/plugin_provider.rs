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

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_cli::plugin_provider::PluginProvider;
use ono_kuang_protocol::{HealthState, Manifest};
use ono_kuang_supervisor::{LoadConfig, LoadedPlugin, Supervisor};
use ono_pipeline::StreamEvent;
use ono_provider_api::{Provider, ProviderRegistry, Query, Selector};
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
    std::fs::copy(example_binary(), package.join("runtime/echo"))
        .expect("the example plugin binary is built");
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
    let manifest =
        Manifest::parse(&echo_package_manifest(ECHO)).expect("the fixture manifest is valid");
    let private = scratch.path().join("state");
    std::fs::create_dir_all(&private).expect("the private directory");
    let mut config = LoadConfig::new(example_binary(), manifest);
    config.private_dir = Some(private);
    config.limits.queue_depth = queue_depth;
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

    let mut stream = registry
        .snapshot(&Query::target("echo-tick"))
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
