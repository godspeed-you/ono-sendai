//! A loaded KUANG/11 package, mounted as an ordinary [`Provider`] (spec §31.23, ADR-0583).
//!
//! ADR-0582 made a contributed target typeable: `get <target>` reaches `provider.query` and the
//! records come back with the schema the package declared and the provenance the host stamped.
//! It reached exactly that far. Everything else the shell asks about a noun — `resolve`, and the
//! action and spatial paths built on it — asks the
//! [`ProviderRegistry`](ono_provider_api::ProviderRegistry), and the registry held no entry for a
//! contributed target at all.
//!
//! This module is that entry. It is deliberately the same shape as
//! [`ono_remote::RemoteProvider`]: one provider per target, wrapping a connection that was
//! established elsewhere, so that nothing above the registry can tell a contributed noun from a
//! built-in one. The supervisor stays out of it — it says itself that registry wiring is the
//! shell's integration step — and so the wrapper lives here, beside the module that loads
//! packages.

use std::sync::{Arc, Mutex, OnceLock};

use ono_core::ErrorCode;
use ono_kuang_supervisor::{LoadedPlugin, StreamEvent};
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Selector};
use ono_value::{ErrorValue, Schema, Value};

/// One target a loaded package contributes, mounted as a [`Provider`].
///
/// Its `id` is `plugin:<package.id>` — the same token the host stamps into every record the
/// package emits (spec §31.80). A provider whose id disagreed with the provenance of its own
/// records would make `inspect` and the registry two different answers to one question.
#[derive(Debug)]
pub struct PluginProvider {
    id: String,
    package_id: String,
    /// The instance answering right now. A reload replaces the instance behind an id without
    /// removing the target from the registry (spec §31.72: a reload is never a moment without
    /// the package), and [`ProviderRegistry`](ono_provider_api::ProviderRegistry) has no
    /// removal — so the handle is swapped rather than the registration.
    plugin: Mutex<Arc<LoadedPlugin>>,
    targets: [&'static str; 1],
}

impl PluginProvider {
    /// One provider per target `plugin` contributes, ready to be registered.
    ///
    /// A package that contributes no target yields none: `roles: [provider]` is a claim about
    /// what a package may do, and what it actually answers for is what it declared.
    #[must_use]
    pub fn of(plugin: &Arc<LoadedPlugin>) -> Vec<Arc<Self>> {
        plugin
            .targets()
            .iter()
            .map(|registered| Arc::new(Self::for_target(plugin, &registered.contribution.name)))
            .collect()
    }

    /// The provider for one named target of `plugin`.
    #[must_use]
    pub fn for_target(plugin: &Arc<LoadedPlugin>, target: &str) -> Self {
        Self {
            id: format!("plugin:{}", plugin.package_id()),
            package_id: plugin.package_id().to_owned(),
            plugin: Mutex::new(Arc::clone(plugin)),
            targets: [intern_target(target)],
        }
    }

    /// The package this provider answers from.
    #[must_use]
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    /// The one target this instance answers about.
    #[must_use]
    pub fn target(&self) -> &'static str {
        self.targets[0]
    }

    /// Points this provider at a freshly loaded instance of the same package.
    ///
    /// Reloading replaces the instance (spec §31.72); the registry entry is the same noun either
    /// way, and re-registering would leave the shut-down instance answering first.
    pub fn adopt(&self, plugin: Arc<LoadedPlugin>) {
        *lock(&self.plugin) = plugin;
    }

    /// The instance answering right now.
    fn plugin(&self) -> Arc<LoadedPlugin> {
        Arc::clone(&lock(&self.plugin))
    }

    /// Whether the instance answering now still contributes this target.
    fn contributes(&self) -> bool {
        self.plugin()
            .targets()
            .iter()
            .any(|registered| registered.contribution.name == self.target())
    }
}

#[async_trait::async_trait]
impl Provider for PluginProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn targets(&self) -> &[&str] {
        &self.targets
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        self.plugin().schemas().to_vec()
    }

    fn capabilities(&self) -> Vec<Capability> {
        // Deliberately none. What a package may do is decided by the KUANG/11 policy broker at
        // every host call (spec §31.19), against the capability vocabulary of the package
        // manifest — not by the shell's provider capabilities, which say what a *built-in*
        // provider needs of the operating system. Restating the grants here would be a second
        // answer to a question that already has an authoritative one.
        Vec::new()
    }

    fn availability(&self) -> Availability {
        let plugin = self.plugin();
        if let Some(failure) = plugin.last_failure() {
            return Availability::unavailable(format!(
                "`{}` is not answering: {}",
                self.package_id,
                failure.message()
            ));
        }
        if !self.contributes() {
            return Availability::unavailable(format!(
                "the loaded instance of `{}` no longer contributes `{}`",
                self.package_id,
                self.target()
            ));
        }
        Availability::Available
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        if query.target_name() != self.target() {
            return Err(unclaimed(&self.package_id, query.target_name()));
        }
        let plugin = self.plugin();
        let target = self.target();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let mut invocation = match plugin.query(target, serde_json::Map::new()).await {
                    Ok(invocation) => invocation,
                    Err(error) => {
                        let _ = sink.fail(crate::kuang_host::wire_error_value(&error)).await;
                        return;
                    }
                };
                loop {
                    // Biased, so a cancelled pipeline stops the package rather than racing it
                    // for one more value. Spec §31.14 requires the cancel to be *delivered*:
                    // a package waiting for demand it will never be granted is a package that
                    // never finishes its handler, and dropping the invocation would leave it
                    // exactly there.
                    let event = tokio::select! {
                        biased;
                        () = sink.cancel_token().cancelled() => break,
                        event = invocation.next() => event,
                    };
                    let delivered = match event {
                        Some(StreamEvent::Value(value)) => sink.send(value).await,
                        Some(StreamEvent::Failed(error)) => {
                            sink.fail(crate::kuang_host::wire_error_value(&error)).await
                        }
                        None => return,
                    };
                    if delivered.is_err() {
                        break;
                    }
                }
                invocation.cancel().await;
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let stream = self.snapshot(&Query::target(self.target()))?;
        let collected = stream.collect().await;
        let mut refs = Vec::new();
        for value in collected.values() {
            if let Value::Record(record) = value
                && selector.matches(record)
                && let Some(reference) = ObjectRef::of(record)
            {
                refs.push(reference);
            }
        }
        // A read that failed outright is a failure to resolve; one that failed beside objects it
        // did resolve is the partial answer spec §16.5 asks for, and the objects stand.
        match collected.errors().first() {
            Some(error) if refs.is_empty() => Err(error.clone()),
            _ => Ok(refs),
        }
    }
}

/// The refusal for a target this provider does not answer for.
fn unclaimed(package_id: &str, target: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ResolveTargetNotFound,
        format!("`{package_id}` contributes no target named `{target}`"),
    )
    .with_help("a package answers for the targets it declared and for nothing else (spec §31.23)")
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Interns a target name, so a provider whose targets are known only once a package has been
/// loaded can still answer [`Provider::targets`], which returns borrowed names.
///
/// The trait's `&[&str]` is written for a built-in provider, whose vocabulary is a `&'static`
/// array in its own source. A package's is not known until its handshake, and a `String` field
/// cannot be lent out as `&[&str]` without either a self-reference or an allocation on every
/// call. So the name is leaked once, and the table hands the same `&'static str` back for every
/// later provider that names it: reloading a package a hundred times leaks nothing further, and
/// the total is bounded by how many distinct targets the installed packages declare rather than
/// by how often they are loaded. `ono_remote` makes the same trade for the same reason.
fn intern_target(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = lock(names);
    if let Some(existing) = guard.iter().find(|candidate| **candidate == name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    guard.push(leaked);
    leaked
}
