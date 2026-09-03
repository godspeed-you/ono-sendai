//! What a one-shot session pays before an external program runs (spec §34).
//!
//! `ono -c 'echo ready'` is the cold-start benchmark of `cargo xtask perf`. Nothing in that
//! pipeline is native, so nothing in it needs the provider registry: no async runtime, no
//! D-Bus connection to systemd and logind, no netlink socket. The session builds those on first
//! use, and an external-only pipeline is not a use.

#![allow(
    clippy::expect_used,
    reason = "a test states its preconditions directly"
)]

use ono_cli::report::Reporter;
use ono_cli::session::Session;
use ono_render::Presentation;

fn run(script: &str) -> Session {
    let mut session = Session::new(false);
    let reporter = Reporter::new(Presentation::Plain);
    let status = ono_cli::repl::run_source(&mut session, script, &reporter);
    assert!(
        status.is_success(),
        "{script:?} should succeed, got {status:?}"
    );
    session
}

#[test]
fn should_not_start_the_runtime_for_a_pipeline_of_external_programs() {
    let session = run("true | true");
    assert!(
        session.runtime_handle().is_none(),
        "spec §34: an external-only pipeline needs no provider, so the runtime that providers \
         run on is never built for it"
    );
}

#[test]
fn should_start_the_runtime_once_a_native_stage_is_in_the_pipeline() {
    let session = run("get dir . | count");
    assert!(
        session.runtime_handle().is_some(),
        "a native stage is planned against the providers, which live on the runtime"
    );
}
