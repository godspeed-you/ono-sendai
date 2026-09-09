//! Opaque secret handles for persisted plans (spec v0.6 §36.3).
//!
//! §36.3 is one sentence — *"Secrets MUST NOT be persisted in raw form when an opaque secret
//! handle can be used"* — and the shape of the answer is `ono-recorder`'s: a declared set of field
//! names, applied on the way into storage, replacing the value and keeping the name. Keeping the
//! name matters as much as dropping the value. A plan that reads `password = <withheld>` still
//! tells the operator that a password is being set, which is exactly the fact `explain` has to
//! show before an apply.
//!
//! The handle is a domain-separated digest of the argument name and the value, truncated. Two
//! consequences follow, and both are deliberate:
//!
//! - **Redaction is idempotent.** A handle is recognised by [`SecretRedaction::is_handle`] and
//!   passes through untouched, so the builder may redact before sealing and the store may redact
//!   again before writing without the two disagreeing.
//! - **The seal survives storage.** [`crate::builder::PlanBuilder`] redacts before it computes
//!   §4.4's digest, so the digest covers the handle. A plan read back out of the store still
//!   verifies against its own seal, which is what §63.2 asks to be checkable.
//!
//! A handle is one-way: nothing here recovers the value, and the store never held it. A holder of
//! the original secret can recompute the handle and confirm a match, which is all a plan view
//! needs.

use std::collections::BTreeSet;
use std::sync::Arc;

use ono_change_core::{Execution, value_text};
use ono_value::{ErrorValue, MapValue, RecordValue, Value};
use sha2::{Digest as _, Sha256};

/// The argument names whose values §36.3 keeps out of the store by default.
///
/// The list is by name because a provider action's arguments are the only shape a secret arrives
/// in here (§2.17 leaves nowhere else for one to hide), and a name is what a contract declares.
/// Over-redacting is the safe direction: a handle costs a reader nothing they were entitled to,
/// and a persisted credential costs rather more.
pub const SENSITIVE_ARGUMENTS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "access_token",
    "refresh_token",
    "api_key",
    "apikey",
    "private_key",
    "key_material",
    "credential",
    "credentials",
    "authorization",
    "auth_token",
    "pre_shared_key",
    "psk",
    "session_key",
];

/// The marker every opaque handle carries, so redaction can tell one from a value (§36.3).
pub const HANDLE_PREFIX: &str = "secret:sha256-";

/// How many hexadecimal characters of the digest a handle shows.
const HANDLE_WIDTH: usize = 16;

/// The domain separator that keeps a handle from colliding with any other digest in v0.6.
const HANDLE_TAG: &str = "ono.plan-secret";

/// Which argument names carry a secret, and what replaces one (§36.3).
#[derive(Debug, Clone)]
pub struct SecretRedaction {
    names: BTreeSet<Arc<str>>,
}

impl Default for SecretRedaction {
    /// [`SENSITIVE_ARGUMENTS`], which is what a caller that declares nothing gets.
    fn default() -> Self {
        Self::new()
    }
}

impl SecretRedaction {
    /// The default declared set of [`SENSITIVE_ARGUMENTS`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            names: SENSITIVE_ARGUMENTS.iter().map(|name| Arc::from(*name)).collect(),
        }
    }

    /// A redaction that declares nothing, for a caller assembling its own set.
    ///
    /// This is the only way to obtain an empty set, and it is spelled out rather than reached by
    /// default: §36.3 is a prohibition, and the value a caller gets by saying nothing must be the
    /// safe one.
    #[must_use]
    pub fn declaring_nothing() -> Self {
        Self {
            names: BTreeSet::new(),
        }
    }

    /// The same redaction, also treating `name` as sensitive.
    #[must_use]
    pub fn declaring(mut self, name: &str) -> Self {
        self.names.insert(Arc::from(normalise(name).as_str()));
        self
    }

    /// Whether an argument called `name` carries a secret.
    ///
    /// Comparison is over the normalised name, so `--API-KEY`, `api_key` and `api-key` are one
    /// declaration rather than three that a contract has to remember to spell all of.
    #[must_use]
    pub fn is_sensitive(&self, name: &str) -> bool {
        self.names.contains(normalise(name).as_str())
    }

    /// Every name this redaction treats as sensitive, in a stable order.
    #[must_use]
    pub fn declared(&self) -> Vec<&str> {
        self.names.iter().map(AsRef::as_ref).collect()
    }

    /// Whether `text` is already an opaque handle.
    ///
    /// This is what makes redaction idempotent, and idempotence is what lets the builder and the
    /// store both apply it without the second pass changing what the first sealed.
    #[must_use]
    pub fn is_handle(text: &str) -> bool {
        text.strip_prefix(HANDLE_PREFIX)
            .is_some_and(|body| body.len() == HANDLE_WIDTH && body.bytes().all(|b| b.is_ascii_hexdigit()))
    }

    /// The opaque handle that stands in for `value` given under `name` (§36.3).
    #[must_use]
    pub fn handle(name: &str, value: &Value) -> Value {
        if let Value::String(text) = value
            && Self::is_handle(text)
        {
            return value.clone();
        }
        let mut hasher = Sha256::new();
        hasher.update(HANDLE_TAG.as_bytes());
        hasher.update([0x1f]);
        hasher.update(normalise(name).as_bytes());
        hasher.update([0x1f]);
        hasher.update(value_text(value).as_bytes());
        let bytes = hasher.finalize();
        let mut handle = String::with_capacity(HANDLE_PREFIX.len() + HANDLE_WIDTH);
        handle.push_str(HANDLE_PREFIX);
        for byte in bytes.iter().take(HANDLE_WIDTH.div_ceil(2)) {
            use std::fmt::Write as _;
            let _ = write!(handle, "{byte:02x}");
        }
        handle.truncate(HANDLE_PREFIX.len() + HANDLE_WIDTH);
        Value::String(Arc::from(handle.as_str()))
    }

    /// `arguments` with every declared secret replaced by its handle.
    #[must_use]
    pub fn arguments(&self, arguments: &[(Arc<str>, Value)]) -> Vec<(Arc<str>, Value)> {
        arguments
            .iter()
            .map(|(name, value)| {
                if self.is_sensitive(name) {
                    (Arc::clone(name), Self::handle(name, value))
                } else {
                    (Arc::clone(name), value.clone())
                }
            })
            .collect()
    }

    /// `argv` with every declared secret replaced by its handle (§12.3, §36.3).
    ///
    /// An argument vector has no names, so the names are the words themselves: `--password=x` and
    /// the pair `--password x` are the two forms a program takes one in, and both are covered. A
    /// bare positional word is left alone, because nothing declares what it means and replacing it
    /// would remove the operator's ability to read what the plan will run.
    #[must_use]
    pub fn argv(&self, argv: &[Arc<str>]) -> Vec<Arc<str>> {
        let mut redacted: Vec<Arc<str>> = Vec::with_capacity(argv.len());
        let mut pending: Option<Arc<str>> = None;
        for word in argv {
            if let Some(name) = pending.take() {
                redacted.push(handle_text(&name, word));
                continue;
            }
            match word.split_once('=') {
                Some((name, value)) if self.is_sensitive(name) => {
                    redacted.push(Arc::from(
                        format!("{name}={}", handle_text(name, value)).as_str(),
                    ));
                }
                _ => {
                    if word.starts_with('-') && self.is_sensitive(word) {
                        pending = Some(Arc::clone(word));
                    }
                    redacted.push(Arc::clone(word));
                }
            }
        }
        redacted
    }

    /// `execution` with every declared secret replaced by its handle.
    #[must_use]
    pub fn execution(&self, execution: &Execution) -> Execution {
        match execution {
            Execution::ProviderAction {
                provider,
                operation,
                arguments,
            } => Execution::ProviderAction {
                provider: Arc::clone(provider),
                operation: Arc::clone(operation),
                arguments: self.arguments(arguments),
            },
            Execution::RecoveryOperation {
                provider,
                capability,
                arguments,
            } => Execution::RecoveryOperation {
                provider: Arc::clone(provider),
                capability: Arc::clone(capability),
                arguments: self.arguments(arguments),
            },
            Execution::Program { program, argv } => Execution::Program {
                program: Arc::clone(program),
                argv: self.argv(argv),
            },
            Execution::Opaque {
                description,
                program,
                argv,
            } => Execution::Opaque {
                description: Arc::clone(description),
                program: program.clone(),
                argv: self.argv(argv),
            },
        }
    }

    /// One `ono.plan-action/1` record with its execution redacted, for the store's write path.
    ///
    /// The store redacts the record rather than the plan because a sealed plan is immutable
    /// (§4.4): there is no way to hand back a changed one, and the record is the thing about to be
    /// written. Everything outside `execution` is copied through, including the digest the seal
    /// was taken over.
    ///
    /// # Errors
    ///
    /// Returns `change.plan_store_corrupt` when the record does not carry the fields
    /// `ono.plan-action/1` declares, which means the contract in this build and the record in hand
    /// disagree.
    pub fn action_record(&self, record: &RecordValue) -> Result<RecordValue, ErrorValue> {
        let redacted = record
            .get("execution")
            .map(|execution| self.execution_value(execution));
        rebuild_record(record, "execution", redacted)
    }

    /// One encoded execution map with its `arguments` and `argv` redacted.
    fn execution_value(&self, value: &Value) -> Value {
        let Value::Map(map) = value else {
            return value.clone();
        };
        let mut next = MapValue::new();
        for (key, item) in map.iter() {
            let replacement = match key {
                "arguments" => self.argument_list(item),
                "argv" => self.argv_list(item),
                _ => item.clone(),
            };
            next.insert(Arc::from(key), replacement);
        }
        Value::Map(Arc::new(next))
    }

    /// The `arguments` list of an encoded execution: `{name, value}` maps, in order.
    fn argument_list(&self, value: &Value) -> Value {
        let Value::List(items) = value else {
            return value.clone();
        };
        Value::list(items.iter().map(|item| {
            let Value::Map(pair) = item else {
                return item.clone();
            };
            let Some(Value::String(name)) = pair.get("name") else {
                return item.clone();
            };
            if !self.is_sensitive(name) {
                return item.clone();
            }
            let mut next = MapValue::new();
            for (key, held) in pair.iter() {
                let replacement = if key == "value" {
                    Self::handle(name, held)
                } else {
                    held.clone()
                };
                next.insert(Arc::from(key), replacement);
            }
            Value::Map(Arc::new(next))
        }))
    }

    /// The `argv` list of an encoded execution: a list of words.
    fn argv_list(&self, value: &Value) -> Value {
        let Value::List(items) = value else {
            return value.clone();
        };
        let words: Vec<Arc<str>> = items
            .iter()
            .map(|item| match item {
                Value::String(text) => Arc::clone(text),
                other => Arc::from(value_text(other).as_str()),
            })
            .collect();
        Value::list(self.argv(&words).into_iter().map(Value::String))
    }
}

/// The handle for `value` written as text, for an argument vector.
fn handle_text(name: &str, value: &str) -> Arc<str> {
    match SecretRedaction::handle(name, &Value::string(value)) {
        Value::String(handle) => handle,
        other => Arc::from(value_text(&other).as_str()),
    }
}

/// One record with a single field replaced, keeping schema, provenance and extensions.
///
/// `pub(crate)` because the store needs the same surgery to lay §41.2's persisted action statuses
/// over a stored plan, and two copies of it would eventually disagree about extensions.
pub(crate) fn rebuild_record(
    record: &RecordValue,
    field: &str,
    replacement: Option<Value>,
) -> Result<RecordValue, ErrorValue> {
    let Some(replacement) = replacement else {
        return Ok(record.clone());
    };
    let mut builder = RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for declared in record.schema().fields() {
        let name = declared.name();
        let value = if name == field {
            replacement.clone()
        } else {
            record.get(name).cloned().unwrap_or(Value::Null)
        };
        builder = builder.set(name, value).map_err(|error| {
            ono_change_core::error::record_malformed(
                name,
                &format!("{} does not declare it: {error}", record.schema_id()),
            )
        })?;
    }
    for (key, value) in record.extra().iter() {
        builder = builder.set_extra(key, value.clone());
    }
    Ok(builder.build())
}

/// A name reduced to the form the declared set is written in: lowercase, undashed, unprefixed.
fn normalise(name: &str) -> String {
    name.trim_start_matches('-')
        .to_ascii_lowercase()
        .replace('-', "_")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_replace_a_declared_secret_with_a_handle_and_keep_the_argument_name() {
        let redaction = SecretRedaction::new();
        let arguments = vec![
            (Arc::from("user"), Value::string("alice")),
            (Arc::from("password"), Value::string("hunter2")),
        ];
        let redacted = redaction.arguments(&arguments);
        assert_eq!(
            redacted[0].1,
            Value::string("alice"),
            "§36.3 redacts secrets, and an ordinary argument is not one"
        );
        assert_eq!(
            redacted[1].0.as_ref(),
            "password",
            "the name stays so a plan view can still say a password is being set (§36.3)"
        );
        assert!(
            !matches!(&redacted[1].1, Value::String(text) if text.contains("hunter2")),
            "§36.3: the raw secret MUST NOT be persisted"
        );
    }

    #[test]
    fn should_leave_an_already_redacted_argument_alone_when_redaction_runs_twice() {
        let redaction = SecretRedaction::new();
        let once = redaction.arguments(&[(Arc::from("token"), Value::string("abc"))]);
        let twice = redaction.arguments(&once);
        assert_eq!(
            once, twice,
            "redaction must be idempotent, or the builder's seal and the store's write disagree"
        );
    }

    #[test]
    fn should_give_the_same_secret_the_same_handle_and_different_secrets_different_ones() {
        let first = SecretRedaction::handle("password", &Value::string("hunter2"));
        let same = SecretRedaction::handle("password", &Value::string("hunter2"));
        let other = SecretRedaction::handle("password", &Value::string("hunter3"));
        assert_eq!(
            first, same,
            "§4.4's digest covers the handle, so an unchanged secret must not re-seal the plan"
        );
        assert_ne!(
            first, other,
            "two different secrets are two different plans (§4.4)"
        );
    }

    #[test]
    fn should_recognise_its_own_handle_and_nothing_else() {
        let Value::String(handle) = SecretRedaction::handle("password", &Value::string("x")) else {
            panic!("a handle is text");
        };
        assert!(SecretRedaction::is_handle(&handle));
        assert!(!SecretRedaction::is_handle("hunter2"));
        assert!(!SecretRedaction::is_handle("secret:sha256-"));
        assert!(!SecretRedaction::is_handle("secret:sha256-zzzzzzzzzzzzzzzz"));
    }

    #[test]
    fn should_treat_one_declaration_as_covering_every_spelling_of_the_name() {
        let redaction = SecretRedaction::new();
        for spelling in ["api_key", "API-KEY", "--api-key", "Api_Key"] {
            assert!(
                redaction.is_sensitive(spelling),
                "`{spelling}` is the same declared argument (§36.3)"
            );
        }
        assert!(!redaction.is_sensitive("path"));
    }

    #[test]
    fn should_declare_nothing_only_when_a_caller_asks_for_it() {
        assert!(SecretRedaction::default().is_sensitive("password"));
        assert!(
            !SecretRedaction::declaring_nothing().is_sensitive("password"),
            "the empty set is reachable only by asking (§36.3)"
        );
        assert!(
            SecretRedaction::declaring_nothing()
                .declaring("vault_pin")
                .is_sensitive("vault_pin")
        );
    }

    #[test]
    fn should_redact_an_inline_option_value_in_an_argument_vector() {
        let redaction = SecretRedaction::new();
        let argv: Vec<Arc<str>> = ["--user=alice", "--password=hunter2"]
            .iter()
            .map(|word| Arc::from(*word))
            .collect();
        let redacted = redaction.argv(&argv);
        assert_eq!(redacted[0].as_ref(), "--user=alice");
        assert!(
            redacted[1].starts_with("--password="),
            "the option name survives so the plan stays readable, got `{}`",
            redacted[1]
        );
        assert!(
            !redacted[1].contains("hunter2"),
            "§36.3: a secret-bearing argument MUST NOT reach the store"
        );
    }

    #[test]
    fn should_redact_the_word_after_a_secret_bearing_option() {
        let redaction = SecretRedaction::new();
        let argv: Vec<Arc<str>> = ["--token", "s3cr3t", "--verbose"]
            .iter()
            .map(|word| Arc::from(*word))
            .collect();
        let redacted = redaction.argv(&argv);
        assert_eq!(redacted[0].as_ref(), "--token");
        assert!(
            SecretRedaction::is_handle(&redacted[1]),
            "`--token s3cr3t` hides the secret in the next word (§36.3), got `{}`",
            redacted[1]
        );
        assert_eq!(redacted[2].as_ref(), "--verbose");
    }

    #[test]
    fn should_leave_a_positional_word_alone() {
        let redaction = SecretRedaction::new();
        let argv: Vec<Arc<str>> = ["restart", "nginx.service"]
            .iter()
            .map(|word| Arc::from(*word))
            .collect();
        assert_eq!(
            redaction.argv(&argv),
            argv,
            "nothing declares what a positional word means, and §2.17 keeps the plan readable"
        );
    }

    #[test]
    fn should_redact_the_arguments_of_every_kind_of_execution() {
        let redaction = SecretRedaction::new();
        let provider = Execution::ProviderAction {
            provider: Arc::from("linux.users"),
            operation: Arc::from("ono.user.set-password"),
            arguments: vec![(Arc::from("password"), Value::string("hunter2"))],
        };
        let recovery = Execution::RecoveryOperation {
            provider: Arc::from("ono.recovery.restic"),
            capability: Arc::from("recovery.prepare"),
            arguments: vec![(Arc::from("passphrase"), Value::string("hunter2"))],
        };
        for execution in [provider, recovery] {
            let redacted = redaction.execution(&execution);
            assert!(
                !redacted.digest_text().contains("hunter2"),
                "§36.3 covers every structured execution, not one of them"
            );
        }
    }

    #[test]
    fn should_keep_the_provider_and_operation_of_a_redacted_execution() {
        let redaction = SecretRedaction::new();
        let redacted = redaction.execution(&Execution::ProviderAction {
            provider: Arc::from("linux.users"),
            operation: Arc::from("ono.user.set-password"),
            arguments: vec![(Arc::from("password"), Value::string("hunter2"))],
        });
        assert_eq!(
            redacted.actor(),
            "linux.users",
            "redaction removes a value, and §4.4 still seals the provider it goes to"
        );
    }
}
