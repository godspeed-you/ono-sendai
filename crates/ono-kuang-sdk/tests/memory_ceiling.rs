//! Which failure class a death at the memory ceiling belongs to (spec §31.15, §31.34).
//!
//! §31.34 lists `resource limit` and `trap/crash` as separate failure classes, and
//! `docs/contracts/kuang/errors.v1.yaml` gives each its own code: `Ono-Sendai-K11203`
//! (`runtime.memory_limit`) says *"The plugin instance exceeded its memory ceiling and was
//! terminated"*, `Ono-Sendai-K11201` (`runtime.trap`) says only that it crashed.
//!
//! No kernel signal distinguishes them. `RLIMIT_DATA` makes the allocation that would cross the
//! ceiling *fail*; what the package does then is the package's own business, and a Rust artifact
//! aborts. The host therefore sees `SIGABRT` in both cases and has to say which one it was from
//! the memory the instance held when it died. That is the contract this suite states:
//!
//! - an instance that traps with its memory at its effective ceiling died *because of* the
//!   ceiling, and is reported as `runtime.memory_limit`, carrying the declared and the effective
//!   ceiling the error contract promises;
//! - an instance that traps anywhere else is a crash, and stays `runtime.trap`.
//!
//! Both cases run the real example package under the real supervisor, so what is asserted is the
//! error a shell would show.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use ono_kuang_protocol::KuangErrorCode;
use ono_kuang_testhost::TestHost;
use serde_json::{Map as JsonMap, Value as Json};

const PACKAGE: &str = "dev.example.echo";

/// The example plugin binary, in the profile this test was built for.
///
/// `CARGO_BIN_EXE_*` is only set for the crate that declares the binary, so the path is derived
/// from this crate's manifest — the same way `ono-testkit` finds the shell.
fn example_plugin() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("target");
    path.push(if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    });
    path.push("kuang-example-plugin");
    assert!(
        path.is_file(),
        "the example plugin is not built at {}: run `cargo build --workspace` first",
        path.display()
    );
    path
}

/// The ceiling `support::example_manifest` declares, which host policy here does not lower, so it
/// is also the effective one.
const CEILING: u64 = 64 * 1024 * 1024;

#[tokio::test]
async fn should_name_the_ceiling_when_an_instance_traps_with_its_memory_at_the_ceiling() {
    // The package allocates far past its ceiling and touches every page, as fast as it can. The
    // pace matters: an instance that climbs to the ceiling inside one of the host's 100 ms
    // memory samples is exactly the instance whose *sampled* memory says nothing, and the class
    // of the death must not depend on whether a sampler happened to look in time.
    let mut arguments = JsonMap::new();
    arguments.insert("mib".to_owned(), Json::from(512));
    arguments.insert("pace-ms".to_owned(), Json::from(0));

    let plugin = TestHost::new(example_plugin(), &support::example_manifest())
        .load()
        .await
        .expect("the example package loads");
    let (_, result) = plugin
        .invoke(&format!("{PACKAGE}.command.hog"), arguments)
        .await
        .expect("the invocation reaches a running instance")
        .collect()
        .await;
    let failure = result
        .error
        .expect("an instance that runs out of its declared room ends its invocation");

    assert_eq!(
        failure.name,
        KuangErrorCode::RuntimeMemoryLimit.name(),
        "spec §31.34: an instance that died with its memory at its ceiling died of the ceiling, \
         and `resource limit` is a failure class of its own rather than an anonymous crash. \
         Got: {failure:?}"
    );
    assert_eq!(
        failure
            .metadata
            .get("resource_class")
            .and_then(Json::as_str),
        Some("memory"),
        "spec §31.34: the class names the resource, so a script need not read the sentence. \
         Got: {failure:?}"
    );
    assert_eq!(
        failure
            .metadata
            .get("declared_memory_max")
            .and_then(Json::as_u64),
        Some(CEILING),
        "errors.v1.yaml, K11203: \"The declared and effective ceilings are in the error \
         metadata\". Got: {failure:?}"
    );
    assert_eq!(
        failure
            .metadata
            .get("effective_memory_max")
            .and_then(Json::as_u64),
        Some(CEILING),
        "errors.v1.yaml, K11203: \"the effective one is the smaller of the package's declaration \
         and host policy\". Got: {failure:?}"
    );
}

#[tokio::test]
async fn should_still_name_a_trap_when_an_instance_dies_nowhere_near_its_ceiling() {
    // The same package, the same ceiling, a death that has nothing to do with it: the fixture
    // stops mid-invocation holding a few megabytes of its 64 MiB. Reading that as a memory limit
    // would be a story rather than a report, and would make K11203 mean "it died".
    let plugin = TestHost::new(example_plugin(), &support::example_manifest())
        .args(&["--misbehave=die"])
        .load()
        .await
        .expect("the fixture starts correctly and dies later");
    let (_, result) = plugin
        .invoke(&format!("{PACKAGE}.command.flood"), JsonMap::new())
        .await
        .expect("the invocation reaches a running instance")
        .collect()
        .await;
    let failure = result
        .error
        .expect("an instance that stops mid-invocation fails it");

    assert_eq!(
        failure.name,
        KuangErrorCode::RuntimeTrap.name(),
        "spec §31.34: `trap/crash` and `resource limit` are different classes, and a package \
         that broke no ceiling belongs to the first. Got: {failure:?}"
    );
    assert_eq!(
        failure.metadata.get("resource_class"),
        None,
        "a resource class belongs to a limit the host enforced, and nothing enforced anything \
         here: {failure:?}"
    );
}
