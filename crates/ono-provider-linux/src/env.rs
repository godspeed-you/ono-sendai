//! The `env` target: the session's variables as records rather than as `NAME=value` lines.
//!
//! The session owns the environment — `let`, `set env` and the inherited block all live in the
//! evaluator's scopes — so this provider is *given* the bindings rather than reading the shell
//! process's own. That is what lets `get env` answer for the scope the user is in rather than
//! for whatever `execve` happened to hand the binary. The session publishes its current
//! bindings before each pipeline runs ([`EnvProvider::publish`]), so what `set env` bound a
//! statement ago is what `get env` lists now.

use std::sync::{Arc, Mutex, PoisonError};

use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::common::provenance;
use crate::schemas;

/// The provider's stable id, as it appears in every record's provenance.
pub const PROVIDER_ID: &str = "ono.session";

/// Where a variable's current value came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvSource {
    /// Inherited from the process that started the shell.
    Inherited,
    /// Set by a configuration file.
    Config,
    /// Set on the command line that started the shell.
    Invocation,
    /// Set during the session, by `let` or `set env`.
    Shell,
}

impl EnvSource {
    /// The name `docs/contracts/schemas/env-var.v1.yaml` gives the source.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            EnvSource::Inherited => "inherited",
            EnvSource::Config => "config",
            EnvSource::Invocation => "invocation",
            EnvSource::Shell => "shell",
        }
    }
}

/// One variable the session holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvBinding {
    /// The variable's name.
    pub name: String,
    /// Its value, verbatim and unsplit.
    pub value: String,
    /// Whether child processes inherit it.
    pub exported: bool,
    /// Where the current value came from.
    pub source: EnvSource,
}

impl EnvBinding {
    /// A variable inherited from the process that started the shell.
    #[must_use]
    pub fn inherited(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            exported: true,
            source: EnvSource::Inherited,
        }
    }

    /// A binding created during the session, exported or not (ADR-0009's `let`).
    #[must_use]
    pub fn shell(name: impl Into<String>, value: impl Into<String>, exported: bool) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            exported,
            source: EnvSource::Shell,
        }
    }
}

/// The session's environment.
///
/// ```
/// use ono_provider_linux::{EnvBinding, EnvProvider};
/// use ono_provider_api::Provider;
///
/// let provider = EnvProvider::new([EnvBinding::inherited("PATH", "/usr/bin")]);
/// assert_eq!(provider.targets(), ["env"]);
/// ```
#[derive(Debug, Default)]
pub struct EnvProvider {
    bindings: Mutex<Vec<EnvBinding>>,
}

impl EnvProvider {
    /// A provider over exactly these bindings.
    ///
    /// They are kept in name order, so `get env` is the same list every time it is asked — the
    /// determinism spec §50 requires of redirected output.
    #[must_use]
    pub fn new(bindings: impl IntoIterator<Item = EnvBinding>) -> Self {
        let provider = Self::default();
        provider.publish(bindings);
        provider
    }

    /// Replaces the bindings with what the session holds now.
    pub fn publish(&self, bindings: impl IntoIterator<Item = EnvBinding>) {
        let mut bindings: Vec<EnvBinding> = bindings.into_iter().collect();
        bindings.sort_by(|left, right| left.name.cmp(&right.name));
        *self.bindings.lock().unwrap_or_else(PoisonError::into_inner) = bindings;
    }

    /// The bindings the provider answers with, in name order.
    #[must_use]
    pub fn bindings(&self) -> Vec<EnvBinding> {
        self.bindings
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn record(binding: &EnvBinding, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
        Ok(RecordValue::builder(
            Arc::clone(schema),
            provenance(PROVIDER_ID, schema.id(), "the session's environment"),
        )
        .set("name", Value::string(&binding.name))?
        .set("value", Value::string(&binding.value))?
        .set("exported", Value::Bool(binding.exported))?
        .set("source", Value::string(binding.source.as_str()))?
        .build())
    }

    fn selected(&self, query: &Query) -> Vec<EnvBinding> {
        let wanted = query
            .selectors()
            .iter()
            .find_map(|selector| match selector {
                Selector::Field { name, value } if name == "name" => value.as_str().ok(),
                _ => None,
            });
        let exported = query
            .option_value("exported")
            .and_then(|value| value.as_bool().ok());
        self.bindings()
            .into_iter()
            .filter(|binding| wanted.is_none_or(|name| binding.name == name))
            .filter(|binding| exported.is_none_or(|wanted| binding.exported == wanted))
            .take(query.max().unwrap_or(usize::MAX))
            .collect()
    }
}

#[async_trait::async_trait]
impl Provider for EnvProvider {
    fn id(&self) -> &str {
        PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["env"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        schemas::require(&schemas::env_var_id())
            .into_iter()
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        // `env.set` is deliberately absent: setting a variable changes the session's scope, which
        // the evaluator owns. A provider handed a snapshot of the environment cannot honour it,
        // and claiming the capability would make `set env` fail somewhere less obvious.
        vec![Capability::new("env.read", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let schema = schemas::require(&schemas::env_var_id())?;
        let selected = self.selected(query);
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                for binding in selected {
                    match Self::record(&binding, &schema) {
                        Ok(record) => {
                            if sink.send(record.into_value()).await.is_err() {
                                return;
                            }
                        }
                        Err(error) => {
                            if sink.fail(error).await.is_err() {
                                return;
                            }
                        }
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let schema = schemas::require(&schemas::env_var_id())?;
        let mut found = Vec::new();
        for binding in self.bindings() {
            let record = Self::record(&binding, &schema)?;
            if selector.matches(&record)
                && let Some(reference) = ObjectRef::of(&record)
            {
                found.push(reference);
            }
        }
        Ok(found)
    }
}
