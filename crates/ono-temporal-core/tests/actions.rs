//! Ono actions as causal anchors: the record of v0.5 §17.4 and the semantic redaction of §17.5
//! and §30.3 — a secret-typed argument is persisted as `<secret:redacted>`, and the raw text
//! never enters the ledger.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use ono_temporal_core::{
    ActionId, ActionOutcome, ActionResultSummary, AuthorizationDecision, AuthorizationSummary,
    REDACTED, Redactable, RedactedCommandSummary,
};

use common::{instant, subject};

#[test]
fn should_replace_a_secret_argument_when_a_command_is_summarised() {
    let summary = RedactedCommandSummary::of(
        "set",
        Some("config-setting"),
        &[
            Redactable::plain("smtp.password"),
            Redactable::secret("value", "hunter2"),
        ],
    );
    assert!(
        !summary.as_str().contains("hunter2"),
        "§17.5: the raw value never enters the summary: {}",
        summary.as_str()
    );
    assert!(
        summary.as_str().contains(REDACTED),
        "the redaction is visible rather than silent: {}",
        summary.as_str()
    );
}

#[test]
fn should_replace_a_secret_shaped_option_when_a_command_is_summarised() {
    for (name, value) in [
        ("--password", "hunter2"),
        ("--token", "ghp_deadbeef"),
        ("--api-key", "sk-live-1234"),
        ("--passphrase", "correct horse"),
        ("--client-secret", "s3cr3t"),
        ("--credential", "abc"),
    ] {
        let summary = RedactedCommandSummary::of("run", None, &[Redactable::option(name, value)]);
        assert!(
            !summary.as_str().contains(value),
            "§30.3: `{name}` is secret-shaped, so its value must not be persisted: {}",
            summary.as_str()
        );
        assert!(
            summary.as_str().contains(name),
            "the option name survives, so the command still reads: {}",
            summary.as_str()
        );
    }
}

#[test]
fn should_replace_a_secret_shaped_inline_argument_when_a_command_is_summarised() {
    let summary = RedactedCommandSummary::of(
        "run",
        None,
        &[
            Redactable::plain("--password=hunter2"),
            Redactable::plain("--lines=20"),
        ],
    );
    assert!(!summary.as_str().contains("hunter2"));
    assert!(
        summary.as_str().contains("--lines=20"),
        "an ordinary option keeps its value: {}",
        summary.as_str()
    );
}

#[test]
fn should_keep_an_ordinary_command_readable_when_nothing_is_secret() {
    let summary =
        RedactedCommandSummary::of("restart", Some("service"), &[Redactable::plain("nginx")]);
    assert_eq!(summary.as_str(), "restart service nginx");
}

#[test]
fn should_mint_an_action_identity_before_execution_when_a_mutation_is_requested() {
    let requested_at = instant("2026-08-31T14:03:11Z");
    let first = ActionId::of("session-1", requested_at, "restart", Some(&subject(1827)));
    let second = ActionId::of("session-1", requested_at, "restart", Some(&subject(1827)));
    assert_eq!(first, second, "the same request is the same action");
    assert!(
        first.to_string().starts_with("ono:a"),
        "§17.3 renders an ActionId `ono:a<hex>`: {first}"
    );
    assert_eq!(ActionId::parse(&first.to_string()).as_ref(), Some(&first));
}

#[test]
fn should_record_the_external_transaction_when_an_authority_returns_its_own_job() {
    let action = common::action(Some("systemd:/org/freedesktop/systemd1/job/4821"));
    assert_eq!(
        action.external_transaction.as_deref(),
        Some("systemd:/org/freedesktop/systemd1/job/4821"),
        "§17.3: the ledger records the mapping to the authority's own id"
    );
}

#[test]
fn should_report_an_action_still_in_flight_as_unknown_rather_than_failed() {
    let action = common::action(None);
    assert!(
        action.result.is_none(),
        "§17.2: an action with no result yet has not failed"
    );
}

#[test]
fn should_name_the_authorisation_decision_when_an_action_is_recorded() {
    let authorization = AuthorizationSummary {
        decision: AuthorizationDecision::Confirmed,
        risk: "destructive".into(),
        capability: Some("system.service.manage".into()),
        reason: Some("the operator confirmed".into()),
    };
    assert_eq!(authorization.decision.as_str(), "confirmed");
    let result = ActionResultSummary {
        outcome: ActionOutcome::Succeeded,
        completed_at: instant("2026-08-31T14:03:13Z"),
        detail: None,
        error_code: None,
    };
    assert_eq!(result.outcome.as_str(), "succeeded");
}

#[test]
fn should_redact_a_secret_written_as_a_separate_word_when_a_command_is_summarised() {
    // §17.5 and §30.6: `--token abc` is two words and neither contains `=`, so a rule that only
    // understands `--token=abc` persists the second of them in clear. The sequence is what has to
    // be read, not each word alone.
    let summary = RedactedCommandSummary::of(
        "set",
        Some("credential"),
        &[
            Redactable::plain("registry"),
            Redactable::plain("--token"),
            Redactable::plain("hunter2"),
        ],
    );

    assert!(
        !summary.as_str().contains("hunter2"),
        "the value of a secret-named flag does not reach the ledger, got {:?}",
        summary.as_str()
    );
    assert!(
        summary.as_str().contains("--token"),
        "the flag stays, so the summary still says what was set: {:?}",
        summary.as_str()
    );
}

#[test]
fn should_keep_a_following_flag_when_a_secret_named_flag_carries_no_value() {
    // `--token --verbose` names no secret value; swallowing the next word would lose an argument
    // that was never one.
    let summary = RedactedCommandSummary::of(
        "set",
        None,
        &[Redactable::plain("--token"), Redactable::plain("--verbose")],
    );

    assert!(
        summary.as_str().contains("--verbose"),
        "a flag after a secret-named flag is not its value, got {:?}",
        summary.as_str()
    );
}

#[test]
fn should_redact_a_password_inside_a_url_when_it_is_the_target() {
    // §30.3: a connection string carries its password in the authority. Everything that
    // identifies the connection survives; the one part that must not be persisted does not.
    let summary = RedactedCommandSummary::of(
        "connect",
        Some("postgres://deploy:hunter2@db01:5432/app"),
        &[],
    );

    assert!(
        !summary.as_str().contains("hunter2"),
        "the password does not reach the ledger, got {:?}",
        summary.as_str()
    );
    assert!(
        summary.as_str().contains("db01") && summary.as_str().contains("deploy"),
        "the host and the user survive, so the record still names the connection: {:?}",
        summary.as_str()
    );
}
