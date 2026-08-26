//! The SSH fallback transport of Phase H, proven without a network or real ssh.
//!
//! Spec §21.4 draws the agent as `LOCAL ONO → typed RPC → REMOTE ono-agent`. In production the
//! byte pipe between them is `ssh <host> ono --agent`; here it is a local child process running
//! the same agent loop over its stdin/stdout, which is exactly the property the transport
//! depends on — the transport cannot tell, and that is the point (AGENTS.md §11: fake the
//! outside world, not your own layers).

mod common;

use common::fixture::fixture_schema_id;
use common::{client_config, within};
use ono_pipeline::StreamEvent;
use ono_provider_api::{Provider, Query};
use ono_remote::{SshTarget, SubprocessTransport, ssh_command};
use ono_value::{Link, Value};

/// The fixture agent, spawned the way `ssh <host> ono --agent` will be.
#[allow(
    clippy::expect_used,
    reason = "a test helper states its precondition the way a #[test] body does"
)]
fn fixture_agent() -> SubprocessTransport {
    let command = tokio::process::Command::new(env!("CARGO_BIN_EXE_ono-remote-fixture-agent"));
    SubprocessTransport::spawn(command).expect("the fixture agent starts")
}

#[tokio::test]
async fn should_run_a_query_against_an_agent_in_a_child_process() {
    let link = within(ono_remote::RemoteLink::connect(
        fixture_agent(),
        client_config(),
    ))
    .await
    .expect("the handshake crosses the process boundary");

    let provider = link
        .providers()
        .iter()
        .find(|provider| provider.targets() == ["process"])
        .cloned()
        .expect("the child announces its `process` target");
    let mut stream = provider
        .snapshot(&Query::target("process"))
        .expect("the query opens");

    let mut pids = Vec::new();
    within(async {
        while let Some(event) = stream.recv().await {
            if let StreamEvent::Value(Value::Record(record)) = event {
                assert_eq!(record.schema_id(), &fixture_schema_id());
                assert_eq!(
                    record.provenance().link(),
                    &Link::Remote("remhost".into()),
                    "a record from the child says which host it came from, exactly as over a \
                     network transport"
                );
                pids.push(record.get("pid").cloned());
            }
        }
    })
    .await;

    assert_eq!(
        pids,
        [
            Some(Value::Int(1)),
            Some(Value::Int(2)),
            Some(Value::Int(3))
        ],
        "the records cross the process boundary intact and in order"
    );
}

#[tokio::test]
async fn should_end_the_child_agent_when_the_link_is_dropped() {
    let transport = fixture_agent();
    let exited = transport.exited();
    let link = within(ono_remote::RemoteLink::connect(transport, client_config()))
        .await
        .expect("the handshake crosses the process boundary");

    drop(link);

    let status = within(exited).await;
    assert!(
        status.is_some_and(|status| status.success()),
        "hanging up makes the agent see end-of-input and exit cleanly: {status:?}"
    );
}

#[test]
fn should_spell_the_real_ssh_invocation_through_one_substitutable_function() {
    let command = ssh_command(
        &SshTarget::new("prod-db")
            .with_user("deploy")
            .with_port(2222),
    );
    let std_command = command.as_std();

    assert_eq!(std_command.get_program(), "ssh");
    let args: Vec<String> = std_command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        args,
        [
            "-o",
            "BatchMode=yes",
            "-T",
            "-p",
            "2222",
            "-l",
            "deploy",
            "--",
            "prod-db",
            "ono",
            "--agent",
        ],
        "the agent end is `ono --agent` (spec §21.4); BatchMode because a refusal is never an \
         interactive prompt (ADR-0015); `--` so a host name cannot become an option"
    );
}
