//! The privacy floor (v0.5 §10.6, §17.5, §30.3, §30.4, §30.5).
//!
//! §10.6's second list is the one that matters here, and it is a prohibition rather than a
//! preference: the recorder MUST NOT persist stdout or stderr bodies, file contents, environment
//! dumps, secrets, secret-bearing command-line arguments, packet payloads or unlimited metric
//! samples. §30.1 says why the default is conservative — "temporal retention increases privacy
//! risk because harmless current-state facts become a behavioral history when persisted".
//!
//! Two mechanisms, and no third:
//!
//! - **A withheld field is nulled, never trimmed.** A field on `WITHHELD_FIELDS` becomes
//!   [`Value::Null`], which `RecordValue::access` reads back as `Unknown` — "nobody wrote this
//!   down", which is exactly true, rather than an empty string, which would read as a value.
//! - **Everything else passes through capture-group redaction.** `ono_history::policy` is the
//!   precedent, and it is reused rather than reimplemented: `--password=` survives so the command
//!   stays readable, and the value does not. Running it over the whole record through
//!   `Value::map_text` means a secret is redacted wherever it is, including inside a list, a map
//!   or a nested record.
//!
//! Argv is the one field with a switch. §30.4 keeps the executable, the name and the identity
//! fields and drops the raw argument vector unless `temporal.record.process_argv` says otherwise;
//! turning it on changes what the ledger *contains*, and the secret patterns still apply.

use std::sync::Arc;

use ono_history::Policy;
use ono_value::{RecordValue, Value};

/// The fields §10.6, §30.3 and §30.5 forbid the default recorder from persisting.
///
/// The list is by field name because a provider record is the only shape these arrive in, and a
/// name is what a schema declares. Over-withholding is the safe direction: a null costs a reader
/// one live query, and a persisted secret costs rather more.
pub const WITHHELD_FIELDS: &[&str] = &[
    // §10.6, §30.3: arbitrary stdout/stderr bodies and command outputs.
    "stdout",
    "stderr",
    "output",
    // §10.6, §30.3: complete file contents, and partial ones.
    "content",
    "contents",
    // §10.6, §30.3: shell environment dumps.
    "env",
    "environ",
    "environment",
    // §10.6, §30.5: raw network packet payloads.
    "payload",
    "packet",
    "packets",
    "body",
];

/// The fields that carry a process's raw argument vector (§30.4).
pub const ARGV_FIELDS: &[&str] = &["command", "cmdline", "argv", "args"];

/// What survives persistence, and what does not (§10.6).
#[derive(Debug, Clone)]
pub struct Redaction {
    policy: Policy,
    process_argv: bool,
}

impl Default for Redaction {
    /// §30.4: `temporal.record.process_argv` is off.
    fn default() -> Self {
        Self::new(false)
    }
}

impl Redaction {
    /// The floor, with argv persistence as `temporal.record.process_argv` states it (§30.4).
    #[must_use]
    pub fn new(process_argv: bool) -> Self {
        Self {
            policy: Policy::default(),
            process_argv,
        }
    }

    /// The floor with a redaction policy of the caller's own beside the default patterns.
    #[must_use]
    pub fn with_policy(mut self, policy: Policy) -> Self {
        self.policy = policy;
        self
    }

    /// Whether `temporal.record.process_argv` is on (§30.4).
    #[must_use]
    pub const fn persists_argv(&self) -> bool {
        self.process_argv
    }

    /// Whether §10.6 forbids persisting a field of this name.
    #[must_use]
    pub fn withholds(&self, field: &str) -> bool {
        let name = field.to_ascii_lowercase();
        WITHHELD_FIELDS.contains(&name.as_str())
            || (!self.process_argv && ARGV_FIELDS.contains(&name.as_str()))
    }

    /// The record as the ledger may hold it (§10.6, §30.3, §30.4).
    ///
    /// Every field the collection policy forbids becomes null, and every text anywhere in what
    /// remains passes the secret patterns. The schema, the identity and the provenance are
    /// untouched: a redacted record is still the same object, observed by the same source.
    #[must_use]
    pub fn record(&self, record: &RecordValue) -> RecordValue {
        let scrubbed = record.map_text(&|text| self.rewrite(text));
        let mut builder =
            RecordValue::builder(Arc::clone(scrubbed.schema()), scrubbed.provenance().clone());
        for field in scrubbed.schema().fields() {
            let name = field.name();
            let value = if self.withholds(name) {
                Value::Null
            } else {
                scrubbed.get(name).cloned().unwrap_or(Value::Null)
            };
            builder = match builder.clone().set(name, value) {
                Ok(next) => next,
                Err(_) => builder,
            };
        }
        for (key, value) in scrubbed.extra().iter() {
            if self.withholds(key) {
                builder = builder.set_extra(key, Value::Null);
            } else {
                builder = builder.set_extra(key, value.clone());
            }
        }
        builder.build()
    }

    /// One value as the ledger may hold it, for a payload that is not a record.
    #[must_use]
    pub fn value(&self, value: &Value) -> Value {
        value.map_text(&|text| self.rewrite(text))
    }

    /// The replacement for one string, or `None` where it carries no secret.
    fn rewrite(&self, text: &str) -> Option<Arc<str>> {
        let redacted = self.policy.redact(text);
        (redacted != text).then(|| Arc::from(redacted.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_withhold_a_body_and_keep_a_name_when_the_policy_is_asked() {
        let redaction = Redaction::default();
        assert!(redaction.withholds("stdout"));
        assert!(redaction.withholds("Environment"));
        assert!(redaction.withholds("command"));
        assert!(!redaction.withholds("name"));
        assert!(!Redaction::new(true).withholds("command"));
    }

    #[test]
    fn should_replace_only_the_value_when_a_secret_shaped_option_is_rewritten() {
        let redaction = Redaction::default();
        let rewritten = redaction.rewrite("psql --password=hunter2");
        let text = rewritten.expect("a secret-shaped option is rewritten");
        assert!(text.contains("--password"));
        assert!(!text.contains("hunter2"));
        assert!(redaction.rewrite("get process").is_none());
    }
}
