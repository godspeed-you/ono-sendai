//! Ono actions as causal anchors (v0.5 §17).
//!
//! §17.1: a shell "knows exactly which actions the operator requested", which is the one causal
//! fact external monitoring usually lacks. §17.5 attaches the price of keeping it: command
//! recording "MUST use semantic redaction", so [`RedactedCommandSummary`] is built from typed
//! arguments and the raw text never enters the type. There is no constructor that takes a
//! rendered command line.

use std::fmt;
use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::SpatialId;
use ono_value::Provenance;

use crate::id::ActionId;

/// What a redacted argument reads as in a persisted summary (§17.5).
pub const REDACTED: &str = "<secret:redacted>";

/// The words that make an argument name secret-shaped.
///
/// Over-redaction is the safe direction: §30.3 says "no secret values", and a `--keyfile` whose
/// path is hidden costs a reader one lookup, where a leaked token costs rather more.
const SECRET_WORDS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "credential",
    "apikey",
    "api-key",
    "api_key",
    "key",
    "auth",
];

/// Whether an argument name means its value must not be persisted.
fn is_secret_name(name: &str) -> bool {
    let lowered = name.trim_start_matches('-').to_ascii_lowercase();
    SECRET_WORDS.iter().any(|word| lowered.contains(word))
}

/// One argument on its way into a persisted action summary (§17.5).
///
/// The caller states what an argument *is*, and this module decides what survives. A caller that
/// only had the rendered text cannot express a secret, which is the point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redactable {
    /// An ordinary argument. Its own text is checked for a secret-shaped `name=value` form.
    Plain(Arc<str>),
    /// A named option and its value.
    Option {
        /// The option, as it was written — `--lines`.
        name: Arc<str>,
        /// Its value.
        value: Arc<str>,
    },
    /// A value the caller knows is secret. Only the name survives (§17.5).
    Secret {
        /// The option or parameter it was given for, where it had one.
        name: Option<Arc<str>>,
    },
}

impl Redactable {
    /// An ordinary argument.
    #[must_use]
    pub fn plain(text: &str) -> Self {
        Redactable::Plain(Arc::from(text))
    }

    /// A named option and its value.
    #[must_use]
    pub fn option(name: &str, value: &str) -> Self {
        Redactable::Option {
            name: Arc::from(name),
            value: Arc::from(value),
        }
    }

    /// A value the caller knows is secret. The value is taken and dropped, never stored.
    #[must_use]
    pub fn secret(name: &str, _value: &str) -> Self {
        Redactable::Secret {
            name: Some(Arc::from(name)),
        }
    }

    /// A secret with no name to show for it.
    #[must_use]
    pub fn anonymous_secret() -> Self {
        Redactable::Secret { name: None }
    }

    /// What this argument reads as once persisted.
    fn render(&self) -> String {
        match self {
            Redactable::Plain(text) => match text.split_once('=') {
                Some((name, _)) if is_secret_name(name) => format!("{name}={REDACTED}"),
                _ => text.to_string(),
            },
            Redactable::Option { name, value } => {
                if is_secret_name(name) {
                    format!("{name}={REDACTED}")
                } else {
                    format!("{name}={value}")
                }
            }
            Redactable::Secret { name } => match name {
                Some(name) => format!("{name}={REDACTED}"),
                None => REDACTED.to_owned(),
            },
        }
    }
}

/// A command as the ledger records it, with every secret-shaped argument replaced (§17.5, §30.3).
///
/// The raw text never enters this type: [`of`](Self::of) composes the summary from the verb, the
/// target and typed arguments, so there is no path by which an unredacted command line becomes
/// one of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactedCommandSummary(Arc<str>);

impl RedactedCommandSummary {
    /// The summary of a command, redacted as §17.5 requires.
    #[must_use]
    pub fn of(verb: &str, target: Option<&str>, arguments: &[Redactable]) -> Self {
        let mut rendered = String::from(verb);
        if let Some(target) = target {
            rendered.push(' ');
            rendered.push_str(target);
        }
        for argument in arguments {
            rendered.push(' ');
            rendered.push_str(&argument.render());
        }
        Self(rendered.into())
    }

    /// The summary as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether anything in the command was redacted, so a reader knows to expect a hole.
    #[must_use]
    pub fn is_redacted(&self) -> bool {
        self.0.contains(REDACTED)
    }
}

impl fmt::Display for RedactedCommandSummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What allowed an action to run (§17.2's `action.authorized`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationDecision {
    /// The policy allowed it without asking.
    Allowed,
    /// The operator was asked and agreed (v0.2 §11.4).
    Confirmed,
    /// The policy refused it.
    Denied,
    /// Nothing had to allow it — the operation is not a mutation.
    NotRequired,
}

impl AuthorizationDecision {
    /// Every decision.
    pub const ALL: &'static [AuthorizationDecision] = &[
        AuthorizationDecision::Allowed,
        AuthorizationDecision::Confirmed,
        AuthorizationDecision::Denied,
        AuthorizationDecision::NotRequired,
    ];

    /// The name `ono.action-event/1` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorizationDecision::Allowed => "allowed",
            AuthorizationDecision::Confirmed => "confirmed",
            AuthorizationDecision::Denied => "denied",
            AuthorizationDecision::NotRequired => "not_required",
        }
    }

    /// The decision with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|decision| decision.as_str() == name)
    }
}

/// Why an action was permitted to run (§17.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationSummary {
    /// What the gate decided.
    pub decision: AuthorizationDecision,
    /// The risk class the command declared (v0.2 §11.3).
    pub risk: Arc<str>,
    /// The capability that had to be granted, where one did.
    pub capability: Option<Arc<str>>,
    /// What a reader needs beside the decision — why a confirmation was asked for.
    pub reason: Option<Arc<str>>,
}

/// How an action ended (§17.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionOutcome {
    /// It did what was asked.
    Succeeded,
    /// It did not.
    Failed,
    /// It was interrupted before it finished (v0.2 §21).
    Cancelled,
}

impl ActionOutcome {
    /// Every outcome.
    pub const ALL: &'static [ActionOutcome] = &[
        ActionOutcome::Succeeded,
        ActionOutcome::Failed,
        ActionOutcome::Cancelled,
    ];

    /// The name `ono.action-event/1` spells.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ActionOutcome::Succeeded => "succeeded",
            ActionOutcome::Failed => "failed",
            ActionOutcome::Cancelled => "cancelled",
        }
    }

    /// The outcome with this name, or `None`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|outcome| outcome.as_str() == name)
    }

    /// The event kind this outcome closes the lifecycle with (§17.2).
    #[must_use]
    pub const fn event_kind(self) -> crate::event::EventKind {
        match self {
            ActionOutcome::Succeeded => crate::event::EventKind::ActionCompleted,
            ActionOutcome::Failed | ActionOutcome::Cancelled => {
                crate::event::EventKind::ActionFailed
            }
        }
    }
}

/// What came of an action (§17.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResultSummary {
    /// How it ended.
    pub outcome: ActionOutcome,
    /// When it ended.
    pub completed_at: Timestamp,
    /// What a reader needs beside the outcome.
    pub detail: Option<Arc<str>>,
    /// The structured error code where it failed (v0.2 §43).
    pub error_code: Option<Arc<str>>,
}

/// A mutation requested through Ono (§17.4).
#[derive(Debug, Clone, PartialEq)]
pub struct ActionEvent {
    /// The identity minted before execution (§17.3).
    pub action_id: ActionId,
    /// The command, redacted (§17.5).
    pub command: RedactedCommandSummary,
    /// The user identity that requested it.
    pub actor: Arc<str>,
    /// The session it came from.
    pub session_id: Arc<str>,
    /// When it was requested.
    pub requested_at: Timestamp,
    /// What it acts on. `None` for an action with no single spatial target.
    pub target: Option<SpatialId>,
    /// The operation, as the command registry spells it.
    pub operation: Arc<str>,
    /// What allowed it (§17.2).
    pub authorization: AuthorizationSummary,
    /// What came of it, once known. `None` while it is still in flight, which is not failure.
    pub result: Option<ActionResultSummary>,
    /// The external authority's own job or transaction id (§17.3).
    pub external_transaction: Option<Arc<str>>,
    /// Where the record came from (v0.2 §25.2).
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_treat_an_argument_name_as_secret_shaped_whatever_its_case_or_dashes() {
        assert!(is_secret_name("--PASSWORD"));
        assert!(is_secret_name("api_key"));
        assert!(is_secret_name("--client-secret"));
        assert!(!is_secret_name("--lines"));
        assert!(!is_secret_name("nginx"));
    }

    #[test]
    fn should_report_that_something_was_hidden_when_a_summary_carries_a_redaction() {
        let summary =
            RedactedCommandSummary::of("set", None, &[Redactable::secret("--token", "abc")]);
        assert!(summary.is_redacted());
        assert!(!RedactedCommandSummary::of("look", None, &[]).is_redacted());
    }

    #[test]
    fn should_close_the_lifecycle_with_a_failure_when_an_action_was_cancelled() {
        assert_eq!(
            ActionOutcome::Cancelled.event_kind(),
            crate::event::EventKind::ActionFailed
        );
        for outcome in ActionOutcome::ALL {
            assert_eq!(ActionOutcome::from_name(outcome.as_str()), Some(*outcome));
        }
        for decision in AuthorizationDecision::ALL {
            assert_eq!(
                AuthorizationDecision::from_name(decision.as_str()),
                Some(*decision)
            );
        }
    }
}
