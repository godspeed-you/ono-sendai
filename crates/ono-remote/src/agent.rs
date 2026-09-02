//! The remote end of a link: spec §21.4's `ono-agent`, as a library.
//!
//! Spec §21.4 sketches the agent as "a small remote agent [that] can expose native provider
//! calls and typed streams over a versioned protocol". The protocol half already exists in
//! `ono-protocol`; this module supplies the other half — a [`RemoteService`] that answers with
//! a real [`ProviderRegistry`], and the negotiation material derived from it: which providers
//! this machine has, which targets they answer, which schemas they produce, what they must be
//! allowed to do, and whether they can answer here at all (spec §21.2, §35.3).
//!
//! [`agent_main`] is the entry the `ono --agent` flag will call: the same loop over stdin and
//! stdout, because in agent mode those *are* the wire — `ssh <host> ono --agent` hands them to
//! the caller as the byte pipe (spec §21.4). Nothing else may be written to stdout in that
//! mode; diagnostics belong on stderr, which ssh carries separately.

use std::process::ExitCode;
use std::sync::Arc;

use ono_adapter::OutputDemand;

use ono_core::ErrorCode;
use ono_pipeline::StreamEvent;
use ono_protocol::{
    ActRequest, AdaptRequest, Audit, Identity, Limits, NoAudit, PeerAuthorization,
    ProviderDescriptor, RemoteQuery, RemoteService, ServerAuthorization, ServerConfig,
    StreamResponder, Transport,
};
use ono_provider_api::{ActionOutcome, Availability, ProviderRegistry};
use ono_value::{ErrorValue, SchemaRegistry, Value};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::transport::StdioTransport;

/// What an agent serves, and as whom.
///
/// ```
/// use std::sync::Arc;
/// use ono_protocol::Identity;
/// use ono_provider_api::ProviderRegistry;
/// use ono_remote::AgentConfig;
///
/// let registry = Arc::new(ProviderRegistry::new());
/// let config = AgentConfig::new(registry).with_identity(Identity::new("deploy"));
/// ```
#[derive(Debug, Clone)]
pub struct AgentConfig {
    registry: Arc<ProviderRegistry>,
    identity: Identity,
    limits: Limits,
    adapters: Option<Arc<ono_adapter::Registry>>,
    authorization: ServerAuthorization,
    action_capabilities: Vec<(String, String, String)>,
    audit: Audit,
    source_address: Option<String>,
}

impl AgentConfig {
    /// An agent serving `registry`.
    ///
    /// The identity defaults to the `USER` environment variable, because that is who the agent
    /// process runs as; the CLI replaces it with the real resolved identity when it wires
    /// `--agent` (spec §21.2: "identity and privilege").
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_owned());
        Self {
            registry,
            identity: Identity::new(user),
            limits: Limits::default(),
            adapters: None,
            authorization: ServerAuthorization::CarriedByTransport,
            action_capabilities: Vec::new(),
            audit: Arc::new(NoAudit),
            source_address: None,
        }
    }

    /// Where this agent's audit events go (v0.4.1 §14.1).
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Where the connection this configuration serves came from (§14.2's `source_address`).
    #[must_use]
    pub fn with_source_address(mut self, address: impl Into<String>) -> Self {
        self.source_address = Some(address.into());
        self
    }

    /// Who decides which clients this agent serves (v0.4.1 §9.2).
    ///
    /// A listening agent passes its `authorized_clients` store; the stdio agent of §4.3 leaves
    /// the default, because the carrier that ran it already decided who may.
    #[must_use]
    pub fn with_authorization(mut self, authorization: ServerAuthorization) -> Self {
        self.authorization = authorization;
        self
    }

    /// Declares which capability an action on `target` spelled `operation` needs (§9.5, §56.4).
    ///
    /// Provider capabilities stay the canonical authorization unit, so this is a restatement of
    /// the command registry rather than a second taxonomy: the CLI fills it from the same
    /// `provider_capability` field `docs/spec/commands/` already declares.
    #[must_use]
    pub fn with_action_capability(
        mut self,
        target: impl Into<String>,
        operation: impl Into<String>,
        capability: impl Into<String>,
    ) -> Self {
        self.action_capabilities
            .push((target.into(), operation.into(), capability.into()));
        self
    }

    /// The adapters this agent negotiates and runs on its own side (spec v0.3 §1.54).
    #[must_use]
    pub fn with_adapters(mut self, adapters: Arc<ono_adapter::Registry>) -> Self {
        self.adapters = Some(adapters);
        self
    }

    /// Who the agent answers as (spec §21.5: least privilege, and visibly so).
    #[must_use]
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// The bounds the agent enforces on its caller (ADR-0015 T7).
    #[must_use]
    pub fn with_limits(mut self, limits: Limits) -> Self {
        self.limits = limits;
        self
    }

    /// The negotiation material spec §21.2 asks for, derived from the registry rather than
    /// written down twice: every provider with its targets, capabilities and availability, and
    /// every schema any of them produces, on top of the built-in ones.
    fn server_config(&self) -> ServerConfig {
        let mut schemas = SchemaRegistry::new();
        for schema in ono_value::builtin_schemas().schemas() {
            // A duplicate id is already registered, which is the outcome wanted.
            let _ = schemas.register((**schema).clone());
        }
        for schema in self.registry.schemas() {
            let _ = schemas.register((*schema).clone());
        }

        let mut config = ServerConfig::new()
            .with_identity(self.identity.clone())
            .with_schemas(Arc::new(schemas))
            .with_limits(self.limits.clone())
            .with_authorization(self.authorization.clone())
            .with_audit(Arc::clone(&self.audit));
        if let Some(address) = &self.source_address {
            config = config.with_source_address(address);
        }
        for (target, operation, capability) in &self.action_capabilities {
            config = config.with_action_capability(target, operation, capability);
        }
        for provider in self.registry.providers() {
            let mut descriptor = ProviderDescriptor::new(provider.id())
                .with_targets(provider.targets().iter().copied());
            for capability in provider.capabilities() {
                descriptor = descriptor.with_capability(&capability);
            }
            if let Availability::Unavailable(reason) = provider.availability() {
                descriptor = descriptor.unavailable(reason);
            }
            config = config.with_provider(descriptor);
        }
        config
    }
}

/// Answers one link from `transport` with the registry in `config`, until the caller hangs up.
///
/// A caller disconnecting is the normal end of a session and returns `Ok(())`; so does a caller
/// that shares no protocol version, which is refused inside the handshake (spec §21.2).
///
/// # Errors
///
/// Returns `remote.protocol_mismatch` when the caller is not speaking this protocol, and
/// `remote.unreachable` when the transport fails or ends mid-frame.
pub async fn serve_registry<T: Transport>(
    transport: T,
    config: AgentConfig,
) -> Result<(), ErrorValue> {
    let server = config.server_config();
    let service = RegistryService {
        registry: Arc::clone(&config.registry),
        adapters: config.adapters.clone(),
        action_capabilities: config.action_capabilities.clone(),
    };
    ono_protocol::serve(transport, server, service).await
}

/// The agent process entry: serve the registry over this process's stdin and stdout.
///
/// This is what `ono --agent` runs (spec §21.4). The exit status follows ADR-0008: `0` when the
/// session ended — however unimpressive the caller's manners — and `1` when the agent itself
/// failed, with the structured error rendered to stderr, which ssh carries back to the user
/// separately from the wire.
pub async fn agent_main<R, W>(input: R, output: W, config: AgentConfig) -> ExitCode
where
    R: AsyncRead + Send + Unpin + 'static,
    W: AsyncWrite + Send + Unpin + 'static,
{
    match serve_registry(StdioTransport::new(input, output), config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ono --agent: {}", error.render_full());
            ExitCode::FAILURE
        }
    }
}

/// A [`RemoteService`] that answers with a local [`ProviderRegistry`].
#[derive(Debug)]
struct RegistryService {
    registry: Arc<ProviderRegistry>,
    adapters: Option<Arc<ono_adapter::Registry>>,
    action_capabilities: Vec<(String, String, String)>,
}

impl RegistryService {
    /// The capability an action needs, as this side declares it.
    fn action_capability(&self, target: &str, operation: &str) -> Option<&str> {
        self.action_capabilities
            .iter()
            .find(|(declared_target, declared_operation, _)| {
                declared_target == target && declared_operation == operation
            })
            .map(|(_, _, capability)| capability.as_str())
    }
}

#[async_trait::async_trait]
impl RemoteService for RegistryService {
    async fn query(
        &self,
        peer: &PeerAuthorization,
        query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        // Asked again, here, from the same context the protocol loop asked it with. §10.2 is
        // explicit that negotiation filtering "is not sufficient by itself"; two checks on two
        // sides of the crate boundary are what makes a missed one a bug rather than a breach.
        peer.require_observe(&format!("get {}", query.target_name()))?;
        let mut stream = self.registry.snapshot(&query.to_query())?;
        // The provider may honour the limit or ignore it (its documented liberty); the caller's
        // bound is enforced here either way, so an endless remote target with a limit ends.
        let limit = query.max().unwrap_or(usize::MAX);
        let mut sent = 0;
        loop {
            // Biased, so a caller that cancelled is heard before the next value is taken out of
            // a producer that may never pause on its own.
            let event = tokio::select! {
                biased;
                () = responder.cancel_token().cancelled() => break,
                event = stream.recv() => match event {
                    Some(event) => event,
                    None => break,
                },
            };
            let delivered = match event {
                StreamEvent::Value(value) => {
                    sent += 1;
                    responder.send(value).await
                }
                StreamEvent::Failure(error) => responder.fail(error).await,
            };
            if delivered.is_err() || sent >= limit {
                break;
            }
        }
        stream.cancel_token().cancel();
        Ok(())
    }

    async fn adapt(
        &self,
        peer: &PeerAuthorization,
        request: AdaptRequest,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        if matches!(peer, PeerAuthorization::Policy(_)) {
            peer.require_action(None, &format!("adapt {}", request.argv().join(" ")))?;
        }
        let Some(adapters) = &self.adapters else {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "this agent has no adapters",
            ));
        };
        let Some(program) = request.argv().first() else {
            return Err(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                "nothing to adapt",
            ));
        };
        let Some(path) = find_on_path(program) else {
            return Err(ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{program}` is not on this host's PATH"),
            ));
        };
        let demand = match request.demand() {
            "structured" => OutputDemand::Structured { schema: None },
            "interactive" => OutputDemand::Interactive,
            _ => OutputDemand::RawBytes,
        };
        let negotiation = adapters.negotiate(&path, request.argv(), &demand);
        if request.is_explain_only() {
            let mut map = ono_value::MapValue::new();
            map.insert("adapted".into(), Value::Bool(negotiation.plan().is_some()));
            map.insert(
                "state".into(),
                Value::string(&negotiation.describe(&demand)),
            );
            map.insert(
                "argv".into(),
                negotiation.plan().map_or(Value::Null, |plan| {
                    Value::list(plan.argv().iter().map(|word| Value::string(word)))
                }),
            );
            let _ = responder.send(Value::Map(Arc::new(map))).await;
            return Ok(());
        }
        if let Some(error) = negotiation.refusal(&demand, &path, request.argv()) {
            return Err(error);
        }
        let Some(plan) = negotiation.plan().cloned() else {
            return Err(ErrorValue::new(
                ErrorCode::AdapterNotAvailable,
                format!(
                    "no adapter on this host gives `{}` structured output",
                    request.argv().join(" ")
                ),
            )
            .with_metadata("invocation", Value::string(&request.argv().join(" ")))
            .with_metadata("raw_fallback_safe", Value::Bool(true)));
        };
        run_plan(plan, request.argv().to_vec(), responder).await
    }

    async fn subscribe(
        &self,
        peer: &PeerAuthorization,
        query: RemoteQuery,
        responder: &StreamResponder,
    ) -> Result<(), ErrorValue> {
        peer.require_observe(&format!("watch {}", query.target_name()))?;
        let mut events = self.registry.subscribe(&query.to_query())?;
        loop {
            let event = tokio::select! {
                biased;
                () = responder.cancel_token().cancelled() => break,
                event = events.recv() => match event {
                    Some(event) => event,
                    None => break,
                },
            };
            if responder.send_event(event).await.is_err() {
                break;
            }
        }
        events.cancel();
        Ok(())
    }

    async fn act(
        &self,
        peer: &PeerAuthorization,
        request: ActRequest,
    ) -> Result<ActionOutcome, ErrorValue> {
        if matches!(peer, PeerAuthorization::Policy(_)) {
            peer.require_action(
                self.action_capability(request.target_name(), request.operation()),
                &format!("{} {}", request.operation(), request.target_name()),
            )?;
        }
        self.registry.act(&request.to_action()).await
    }
}

/// The first executable named `program` on this process's `PATH`, or the path itself.
fn find_on_path(program: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt as _;
    let executable = |path: &std::path::Path| {
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    };
    if program.contains('/') {
        let path = std::path::PathBuf::from(program);
        return executable(&path).then_some(path);
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(program))
        .find(|candidate| executable(candidate))
}

/// Runs an adapter plan on this side and streams what it decodes (ADR-0066): the child is
/// spawned with the plan's argv and environment, a reader thread decodes its stdout, values
/// and failures go to the responder as they arrive, and a non-zero exit is reported after them.
async fn run_plan(
    plan: ono_adapter::AdapterPlan,
    user_invocation: Vec<String>,
    responder: &StreamResponder,
) -> Result<(), ErrorValue> {
    let trace = ono_adapter::Trace {
        executable: plan.executable().to_path_buf(),
        version: plan.version().cloned(),
        user_invocation,
        actual_invocation: plan.argv().to_vec(),
        host: None,
    };
    let mut decoding =
        ono_adapter::Decoding::for_plan(plan.clone(), trace, ono_value::builtin_schemas())?;
    let mut command = std::process::Command::new(plan.executable());
    command
        .args(plan.argv().iter().skip(1))
        .envs(plan.env())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit());
    let mut child = command.spawn().map_err(|error| {
        ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            format!("running {}: {error}", plan.executable().display()),
        )
    })?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err(ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the child's stdout could not be captured",
        ));
    };
    let (sender, mut receiver) = tokio::sync::mpsc::channel::<Result<Value, ErrorValue>>(256);
    let reader = std::thread::spawn(move || {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            match std::io::Read::read(&mut stdout, &mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => {
                    for outcome in decoding.feed(&buffer[..count]) {
                        if sender.blocking_send(outcome).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        for outcome in decoding.finish() {
            if sender.blocking_send(outcome).is_err() {
                return;
            }
        }
    });
    let mut cancelled = false;
    loop {
        let outcome = tokio::select! {
            biased;
            () = responder.cancel_token().cancelled() => {
                cancelled = true;
                break;
            }
            outcome = receiver.recv() => match outcome {
                Some(outcome) => outcome,
                None => break,
            },
        };
        let delivered = match outcome {
            Ok(value) => responder.send(value).await,
            Err(error) => responder.fail(error).await,
        };
        if delivered.is_err() {
            cancelled = true;
            break;
        }
    }
    if cancelled {
        let _ = child.kill();
    }
    let status = tokio::task::spawn_blocking(move || child.wait())
        .await
        .map_err(|_| {
            ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                "the child could not be waited for",
            )
        })?
        .map_err(|error| ErrorValue::new(ErrorCode::IoPermissionDenied, error.to_string()))?;
    let _ = reader.join();
    if !cancelled && !status.success() {
        let program = plan
            .executable()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(ErrorValue::new(
            ErrorCode::ExternalExitNonzero,
            format!(
                "{program} exited with status {}",
                status.code().unwrap_or(-1)
            ),
        ));
    }
    Ok(())
}
