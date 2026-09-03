//! What `ono --agent --listen` can be asked to do, and the one thing it cannot (v0.4.1 spec §7.4,
//! §11.1, §65.1; issue #39).
//!
//! > The normal direct listening-agent mode MUST NOT provide a flag that disables client
//! > authentication. (§7.4)
//!
//! §65.1 names the mistake this prevents — "using encryption while accepting any client
//! certificate/no certificate and then calling the session authenticated is forbidden" — and the
//! ordinary way a project makes it is not by choosing to, but by growing a flag for one awkward
//! deployment and never taking it away. So the check is written down before there is anything to
//! check: every spelling of that flag anyone might reach for is a usage error, and the only
//! listening form is the authenticated one.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_cli::invocation::Invocation;
use ono_testkit::Shell;

/// The flags a person would try if they wanted an agent that authenticates nobody.
///
/// Not an exhaustive list of strings — nothing could be — but the ones every neighbouring tool
/// spells it with, which is where a request for one would arrive from.
const WOULD_DISABLE_AUTHENTICATION: &[&str] = &[
    "--no-client-auth",
    "--no-client-cert",
    "--no-auth",
    "--no-mutual-auth",
    "--no-verify",
    "--no-tls",
    "--insecure",
    "--anonymous",
    "--allow-anonymous",
    "--allow-unauthenticated",
    "--unauthenticated",
    "--disable-client-auth",
    "--trust-any-client",
    "--legacy",
];

fn parse<const N: usize>(arguments: [&str; N]) -> Invocation {
    Invocation::from_args(arguments.map(String::from))
}

#[test]
fn should_have_no_flag_that_turns_client_authentication_off_for_a_listening_agent() {
    for flag in WOULD_DISABLE_AUTHENTICATION {
        let parsed = parse(["--agent", "--listen", "127.0.0.1:0", flag]);

        assert!(
            matches!(parsed, Invocation::Usage(_)),
            "v0.4.1 §7.4: the normal listening-agent mode MUST NOT provide a flag that disables \
             client authentication, and `{flag}` parsed as {parsed:?} instead of a usage error"
        );
    }
}

#[test]
fn should_offer_exactly_one_listening_form_and_it_authenticates_its_clients() {
    let parsed = parse(["--agent", "--listen", "127.0.0.1:7734"]);

    let Invocation::Agent(_, options) = parsed else {
        panic!("`--agent --listen <address>` is how a listening agent is started");
    };
    assert_eq!(
        options.listen.as_deref(),
        Some("127.0.0.1:7734"),
        "the address is the only thing `--listen` carries"
    );
    // `--host-key` points the agent at a different identity file; it cannot say *not* to have
    // one, which is what makes §7.4 hold by construction rather than by review.
    assert_eq!(options.host_key, None);
    assert!(!options.print_host_key);
}

#[test]
fn should_report_a_usage_error_rather_than_listening_when_asked_to_authenticate_nobody() {
    for flag in WOULD_DISABLE_AUTHENTICATION {
        let run = Shell::new()
            .args(["--agent", "--listen", "127.0.0.1:0", flag])
            .run();

        assert!(
            !run.status().is_success(),
            "`ono --agent --listen … {flag}` must not start an agent, got {:?}",
            run.stdout()
        );
        assert!(
            !run.stderr().contains("listening on"),
            "nothing may be bound on the way to refusing the flag, got {:?}",
            run.stderr()
        );
    }
}
