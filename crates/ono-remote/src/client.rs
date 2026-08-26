//! The local end of a link: a remote machine mounted as ordinary providers (spec §21.4).
//!
//! Spec §21's promise is that a linked machine is not a different kind of thing to talk to:
//! `get process` against it is the same command, the same records, the same schema, the same
//! partial-failure semantics as at home. The way this crate keeps that promise is to make the
//! remote end *be* a [`Provider`]: [`RemoteLink::connect`] performs the handshake and the trust
//! decision, and hands back one [`RemoteProvider`] per negotiated target, ready to be
//! registered into the same [`ProviderRegistry`](ono_provider_api::ProviderRegistry) the local
//! providers live in. Everything above the registry — the evaluator, the pipeline, the renderer
//! — works unchanged, because nothing above the registry can tell.
//!
//! What does change is provenance: every arriving record is re-tagged with the host it came
//! from (spec §25.2, [`crate::retag`]), so `inspect` and the remote-context prompt of spec
//! §14.4 can always say where an object lives.

use std::sync::{Arc, Mutex, OnceLock};

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_protocol::{
    ActRequest, ClientConfig, Link, Negotiated, ProviderDescriptor, RemoteMessage, RemoteQuery,
    Transport,
};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, EventStream, ObjectRef, Provider, Query,
    Selector,
};
use ono_value::{ErrorValue, Schema, Value};

use crate::retag::{retag_event, retag_record, retag_value};

/// An established link to a remote machine, and the providers it negotiated.
///
/// Dropping the link does not tear down providers that were already registered elsewhere: each
/// [`RemoteProvider`] keeps the connection alive for as long as it is mounted.
#[derive(Debug)]
pub struct RemoteLink {
    link: Arc<Link>,
    host: Arc<str>,
    providers: Vec<Arc<RemoteProvider>>,
}

impl RemoteLink {
    /// Performs the handshake and the trust decision over `transport`, then derives one
    /// mountable provider per target the remote negotiated.
    ///
    /// # Errors
    ///
    /// Exactly [`Link::connect`]'s: `remote.host_key_changed` (E0603) for a peer presenting a
    /// key other than the pinned one — never a prompt (ADR-0015 T5/T6) — `safety.policy_denied`
    /// (E0702) when the trust policy will not accept the peer, `remote.protocol_mismatch`
    /// (E0602) and `remote.unreachable` (E0601) for a peer that cannot be spoken to.
    pub async fn connect<T: Transport>(
        transport: T,
        config: ClientConfig,
    ) -> Result<Self, ErrorValue> {
        let host: Arc<str> = Arc::from(config.host());
        let schemas = Arc::clone(config.schemas());
        let link = Arc::new(Link::connect(transport, config).await?);

        // The schemas the remote will send, as this side defines them; decoding already
        // guarantees an arriving record satisfies a schema this side knows.
        let negotiated_schemas: Vec<Arc<Schema>> = schemas
            .schemas()
            .filter(|schema| {
                link.negotiated()
                    .schemas()
                    .iter()
                    .any(|id| id == &schema.id().to_string())
            })
            .map(Arc::clone)
            .collect();

        let mut providers = Vec::new();
        for descriptor in link.negotiated().providers() {
            for target in descriptor.targets() {
                providers.push(Arc::new(RemoteProvider::new(
                    Arc::clone(&link),
                    Arc::clone(&host),
                    descriptor,
                    target,
                    negotiated_schemas.clone(),
                )));
            }
        }
        Ok(Self {
            link,
            host,
            providers,
        })
    }

    /// The host this link is to, as the user named it.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Everything the handshake settled (spec §21.2).
    #[must_use]
    pub fn negotiated(&self) -> &Negotiated {
        self.link.negotiated()
    }

    /// One mountable provider per negotiated target, the visibly unavailable ones included
    /// (spec §21.3: a reduced capability set must be visible, not silent).
    #[must_use]
    pub fn providers(&self) -> &[Arc<RemoteProvider>] {
        &self.providers
    }

    /// Registers every negotiated provider into `registry`, after which `get <target>` reaches
    /// the remote machine through the ordinary provider path.
    pub fn register_into(&self, registry: &mut ono_provider_api::ProviderRegistry) {
        for provider in &self.providers {
            registry.register(Arc::clone(provider) as Arc<dyn Provider>);
        }
    }

    /// Performs one action on the remote machine and waits for its structured outcome
    /// (spec §11.5).
    ///
    /// This is the action path for now: the mounted [`RemoteProvider`] deliberately refuses
    /// [`Provider::act`], because [`Action`] does not expose its arguments for enumeration and
    /// forwarding an action without them would silently drop, say, the signal of a `stop`.
    ///
    /// # Errors
    ///
    /// Whatever the remote refused with, or `remote.unreachable` when the link failed first.
    pub async fn act(&self, request: &ActRequest) -> Result<ActionOutcome, ErrorValue> {
        self.link.act(request).await
    }

    /// The underlying protocol link, for callers that need raw streams.
    #[must_use]
    pub fn protocol(&self) -> &Link {
        &self.link
    }
}

/// One remote target, mounted as an ordinary [`Provider`].
///
/// Its `id` is the *remote* provider's id — `linux.procfs` stays `linux.procfs` — because that
/// is who really produced the records, and provenance must keep saying so (spec §25.2). Which
/// machine it ran on is the provenance link, not the provider name.
#[derive(Debug)]
pub struct RemoteProvider {
    link: Arc<Link>,
    host: Arc<str>,
    id: String,
    targets: [&'static str; 1],
    schemas: Vec<Arc<Schema>>,
    capabilities: Vec<Capability>,
    availability: Availability,
}

impl RemoteProvider {
    fn new(
        link: Arc<Link>,
        host: Arc<str>,
        descriptor: &ProviderDescriptor,
        target: &str,
        schemas: Vec<Arc<Schema>>,
    ) -> Self {
        let availability = match descriptor.unavailable_reason() {
            None => Availability::Available,
            Some(reason) => Availability::unavailable(reason),
        };
        Self {
            link,
            host,
            id: descriptor.id().to_owned(),
            targets: [intern_target(target)],
            schemas,
            capabilities: descriptor
                .capabilities()
                .iter()
                .map(ono_protocol::CapabilityDescriptor::to_capability)
                .collect(),
            availability,
        }
    }

    /// The one target this instance answers about.
    fn target(&self) -> &'static str {
        self.targets[0]
    }
}

#[async_trait::async_trait]
impl Provider for RemoteProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn targets(&self) -> &[&str] {
        &self.targets
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        self.schemas.clone()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities.clone()
    }

    fn availability(&self) -> Availability {
        self.availability.clone()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let remote = self.link.query(&RemoteQuery::from_query(query))?;
        let host = Arc::clone(&self.host);
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let mut remote = remote;
                loop {
                    // Biased, so cancelling the pipeline stops the remote producer rather than
                    // racing it for one more value; dropping `remote` sends the cancel.
                    let message = tokio::select! {
                        biased;
                        () = sink.cancel_token().cancelled() => break,
                        message = remote.recv() => message,
                    };
                    let delivered = match message {
                        Some(RemoteMessage::Value(value)) => {
                            sink.send(retag_value(value, &host)).await
                        }
                        Some(RemoteMessage::Failure(error)) => sink.fail(error).await,
                        Some(RemoteMessage::Event(_)) => Ok(()),
                        None => break,
                    };
                    if delivered.is_err() {
                        break;
                    }
                }
            },
        ))
    }

    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        let remote = self.link.subscribe(&RemoteQuery::from_query(query))?;
        let host = Arc::clone(&self.host);
        Ok(EventStream::spawn(PipelineConfig::new(), move |sink| {
            async move {
                let mut remote = remote;
                while let Some(message) = remote.recv().await {
                    // An event stream has no error channel (spec §31.14), so a remote refusal —
                    // a provider that cannot watch — arrives as an immediately ending stream;
                    // callers that need the refusal itself use the protocol link directly.
                    if let RemoteMessage::Event(event) = message
                        && sink.send(retag_event(event, &host)).await.is_err()
                    {
                        break;
                    }
                }
            }
        }))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let mut remote = self
            .link
            .query(&RemoteQuery::target(self.target()).with(selector.clone()))?;
        let mut refs = Vec::new();
        let mut failure = None;
        while let Some(message) = remote.recv().await {
            match message {
                RemoteMessage::Value(Value::Record(record)) => {
                    let retagged = retag_record(&record, &self.host);
                    if let Some(reference) = ObjectRef::of(&retagged) {
                        refs.push(reference);
                    }
                }
                RemoteMessage::Failure(error) => failure = Some(error),
                RemoteMessage::Value(_) | RemoteMessage::Event(_) => {}
            }
        }
        match failure {
            // A failure beside resolved objects is a partial answer; the objects stand.
            Some(error) if refs.is_empty() => Err(error),
            _ => Ok(refs),
        }
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let _ = action;
        // `Action` exposes its arguments only by name, so a faithful `ActRequest` cannot be
        // built from one: forwarding would silently drop, say, the signal of a `stop`, and a
        // dropped argument on a mutation is exactly the kind of lie ADR-0015 exists to prevent.
        Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            format!(
                "actions on {} are not forwarded through the mounted provider yet",
                self.host
            ),
        )
        .with_help(
            "perform the action through the link itself (`RemoteLink::act`), which carries \
             every argument explicitly",
        ))
    }
}

/// Interns a target name, so a provider whose targets are only known at negotiation time can
/// still answer [`Provider::targets`], which returns borrowed names.
///
/// The table leaks one copy of each *distinct* name for the life of the process. Target names
/// come from a small, closed vocabulary (`docs/spec/targets.yaml`, plus what plugins add), so
/// the leak is bounded by the vocabulary, not by how often links are opened.
fn intern_target(name: &str) -> &'static str {
    static NAMES: OnceLock<Mutex<Vec<&'static str>>> = OnceLock::new();
    let names = NAMES.get_or_init(|| Mutex::new(Vec::new()));
    let mut guard = match names.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(existing) = guard.iter().find(|candidate| **candidate == name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    guard.push(leaked);
    leaked
}
