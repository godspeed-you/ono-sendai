//! Just-in-time consent: the seam through which the broker asks a person when the need is
//! concrete (K11P §14, ADR-0603).
//!
//! The policy module says why the supervisor never prompted: a library has no prompt to offer.
//! It still has none. What it has is a [`ConsentSource`] the host supplies, called only when an
//! evaluation is `Denied(Default)` — never over a system or operator deny — for a capability the
//! package's permission layer decides just in time. The default answers `Deny`, which is what a
//! non-interactive host must answer too (K11P §3 invariant 11); the shell's source renders the
//! question and reads the answer; a test's source answers what the test wrote down.

use std::sync::{Arc, Mutex};

use ono_kuang_protocol::{Capability, KuangError, KuangErrorCode, PermissionDescriptor};
use serde_json::{Map as JsonMap, Value as Json};

/// What the broker is about to allow or refuse, with everything a person needs to decide it.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsentRequest {
    /// The package asking.
    pub package: String,
    /// The capability the call needs.
    pub capability: Capability,
    /// The permission that governs it — its id, title, purpose and kind — from the package's
    /// permission layer (ADR-0600).
    pub permission: PermissionDescriptor,
    /// The host call being made, e.g. `process.exec`.
    pub action: String,
    /// The invocation the call belongs to, as the audit trail labels it.
    pub invocation: String,
    /// The concrete values the call will use, by scope key: `programs` → the resolved path,
    /// `hosts` → the host, and so on. What an `always` answer is scoped to.
    pub uses: Vec<(String, String)>,
    /// The program's arguments, for a `process.exec` request, so the prompt can show the
    /// command line (K11P §14.2). Secret-looking arguments are the prompt's to redact.
    pub arguments: Vec<String>,
    /// The scope a `session` or `always` answer carries: the narrowest enforceable one, built
    /// by the broker from `uses` (K11P §14.3). A source may narrow it further, never widen it.
    pub scope: Option<JsonMap<String, Json>>,
}

impl ConsentRequest {
    /// The program a `process.exec` request names, when it does.
    #[must_use]
    pub fn program(&self) -> Option<&str> {
        self.uses
            .iter()
            .find(|(key, _)| key == "programs")
            .map(|(_, value)| value.as_str())
    }

    /// The scope word K11P §14 shows: `/usr/bin/aws` for a program, `host:port` for a
    /// connection, or the uses joined.
    #[must_use]
    pub fn subject(&self) -> String {
        if let Some(program) = self.program() {
            return program.to_owned();
        }
        self.uses
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// How long a `yes` lasts (K11P §14.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentDuration {
    /// This one evaluation. Nothing is recorded as a grant.
    Once,
    /// A session grant, scoped to the exact value in use.
    Session,
    /// A persistent grant, scoped the same way, plus a decision record.
    Always,
}

impl ConsentDuration {
    /// The word the audit trail and the grant record carry.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            ConsentDuration::Once => "once",
            ConsentDuration::Session => "session",
            ConsentDuration::Always => "always",
        }
    }
}

/// The answer a source gives.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsentAnswer {
    /// The call proceeds, for this long, in this scope. A scope wider than the request's is
    /// narrowed back to the request's by the broker; a source cannot widen what was asked.
    Allow {
        /// How long.
        duration: ConsentDuration,
        /// The scope the grant carries. `None` takes the request's.
        scope: Option<JsonMap<String, Json>>,
    },
    /// The call is refused with this error — `permission.denied` where a person said no,
    /// `permission.required` where nobody could be asked (K11P §14.5, §20.3).
    Deny(KuangError),
}

impl ConsentAnswer {
    /// The refusal a source gives where nobody can be asked (K11P §20.3, Gate J), with the
    /// remedy the metadata must carry.
    #[must_use]
    pub fn required(request: &ConsentRequest, remedy: &str) -> Self {
        let permission = &request.permission;
        let mut error = KuangError::new(
            KuangErrorCode::PermissionRequired,
            format!(
                "{} needs permission to {} ({}), and nobody can be asked here",
                package_display(&request.package),
                lowercase_first(&permission.title),
                request.subject()
            ),
        )
        .with_help(format!(
            "a script never waits for a prompt and never assumes consent (K11P §20.3); decide it \
             deliberately: {remedy}"
        ))
        .with_metadata("plugin", Json::String(request.package.clone()))
        .with_metadata("permission", Json::String(permission.id.clone()))
        .with_metadata(
            "capability",
            Json::String(request.capability.id().to_owned()),
        )
        .with_metadata("remedy", Json::String(remedy.to_owned()));
        let mut requested = JsonMap::new();
        for (key, value) in &request.uses {
            requested.insert(key.clone(), Json::String(value.clone()));
        }
        if let Some(program) = request.program() {
            requested.insert("program".to_owned(), Json::String(program.to_owned()));
        }
        if !request.arguments.is_empty() {
            requested.insert(
                "arguments".to_owned(),
                Json::Array(
                    request
                        .arguments
                        .iter()
                        .map(|argument| Json::String(argument.clone()))
                        .collect(),
                ),
            );
        }
        error = error.with_metadata("requested_scope", Json::Object(requested));
        if let Some(purpose) = &permission.purpose {
            error = error.with_metadata("reason", Json::String(purpose.clone()));
        }
        ConsentAnswer::Deny(error)
    }

    /// The refusal a source gives where a person said no (K11P §14.5).
    #[must_use]
    pub fn denied(request: &ConsentRequest, remedy: &str) -> Self {
        let permission = &request.permission;
        ConsentAnswer::Deny(
            KuangError::new(
                KuangErrorCode::PermissionDenied,
                format!(
                    "{} cannot {} ({}) because permission to {} is denied",
                    package_display(&request.package),
                    lowercase_first(&permission.title),
                    request.subject(),
                    lowercase_first(&permission.title)
                ),
            )
            .with_help(format!(
                "`get permission {}` shows the decision; {remedy}",
                request.package
            ))
            .with_metadata("plugin", Json::String(request.package.clone()))
            .with_metadata("permission", Json::String(permission.id.clone()))
            .with_metadata(
                "capability",
                Json::String(request.capability.id().to_owned()),
            )
            .with_metadata("operation", Json::String(request.action.clone()))
            .with_metadata("remedy", Json::String(remedy.to_owned())),
        )
    }
}

/// Who answers a just-in-time request.
pub trait ConsentSource: Send + Sync + std::fmt::Debug {
    /// Decides one request. May block on a person; the broker calls it off the runtime's
    /// worker threads.
    fn consent(&self, request: &ConsentRequest) -> ConsentAnswer;
}

/// A source that can ask nobody: every request is `permission.required` (K11P §20.3).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoConsent;

impl ConsentSource for NoConsent {
    fn consent(&self, request: &ConsentRequest) -> ConsentAnswer {
        ConsentAnswer::required(
            request,
            &format!(
                "`grant capability {} --plugin {}{} --duration always`",
                request.capability.id(),
                request.package,
                scope_words(request.scope.as_ref())
            ),
        )
    }
}

/// A source that answers what it was told to, in order, and remembers what it was asked —
/// the fake outside world a test hands the real broker.
#[derive(Debug, Default)]
pub struct ScriptedConsent {
    answers: Mutex<std::collections::VecDeque<ConsentAnswer>>,
    asked: Mutex<Vec<ConsentRequest>>,
}

impl ScriptedConsent {
    /// A source with these answers queued; once they run out, it answers `permission.required`.
    #[must_use]
    pub fn answering(answers: impl IntoIterator<Item = ConsentAnswer>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            asked: Mutex::new(Vec::new()),
        })
    }

    /// Every request so far, in order.
    #[must_use]
    pub fn asked(&self) -> Vec<ConsentRequest> {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl ConsentSource for ScriptedConsent {
    fn consent(&self, request: &ConsentRequest) -> ConsentAnswer {
        self.asked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.clone());
        self.answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
            .unwrap_or_else(|| NoConsent.consent(request))
    }
}

/// `--scope key=value` words for a remedy line.
#[must_use]
pub fn scope_words(scope: Option<&JsonMap<String, Json>>) -> String {
    let Some(scope) = scope else {
        return String::new();
    };
    scope
        .iter()
        .map(|(key, value)| {
            let values: Vec<String> = match value {
                Json::Array(items) => items
                    .iter()
                    .map(|item| match item {
                        Json::String(text) => text.clone(),
                        other => other.to_string(),
                    })
                    .collect(),
                Json::String(text) => vec![text.clone()],
                other => vec![other.to_string()],
            };
            format!(" --scope {key}={}", values.join(","))
        })
        .collect()
}

fn package_display(id: &str) -> String {
    id.rsplit('.').next().unwrap_or(id).to_owned()
}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
