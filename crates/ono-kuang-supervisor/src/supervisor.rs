//! Spawning, handshake, and the per-instance actor that brokers every host call.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ono_kuang_protocol::{
    AuditLogParams, AuditResult, CancelParams, CancelReason, CheckAnswer, CheckParams, CloseParams,
    CommandContribution, DemandParams, EmitParams, EmitResult, Enforcement, Envelope,
    FilesystemReadParams, FilesystemReadResult, FrameError, FrameLimits, HOST_API, Hello,
    InitParams, InitResult, InvokeParams, InvokeResult, InvokeStatus, KuangError, KuangErrorCode,
    Lease, Lifecycle, Manifest, OverflowPolicy, PACKAGE_FORMAT, PluginContract, PluginState,
    ProbeResult, QueryParams, RequestOnceParams, ShutdownParams, ShutdownReason, StateGetResult,
    StateKeyParams, StateSetParams, TargetContribution, VersionRange, WireError, decode_payload,
    method,
};
use ono_value::{SchemaRegistry, Value, from_json, to_json};
use serde_json::{Map as JsonMap, Value as Json, json};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};

use crate::negotiate::{HostLimits, negotiate};
use crate::policy::{Evaluation, Policy, ScopeUse, denial_error};
use crate::state::StateStore;
use crate::trail::{AuditTrail, HostClock};

/// The platform tuple of the running host, in the manifest's vocabulary (`linux-amd64`).
#[must_use]
pub fn host_platform() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// Everything a load needs (spec §31.63). The manifest arrives already parsed — and therefore
/// already validated — because [`Manifest::parse`] is the only way to build one.
#[derive(Debug)]
pub struct LoadConfig {
    /// The runtime artifact to spawn.
    pub program: PathBuf,
    /// Arguments for the artifact.
    pub args: Vec<String>,
    /// The validated manifest.
    pub manifest: Manifest,
    /// The capability policy the broker enforces.
    pub policy: Policy,
    /// The host's resource ceilings.
    pub limits: HostLimits,
    /// The host clock. Fixed under the test host (spec §31.73).
    pub clock: HostClock,
    /// The platform tuple checked against `compatibility.platforms`.
    pub platform: String,
}

impl LoadConfig {
    /// A config with default limits, the system clock and the running host's platform.
    #[must_use]
    pub fn new(program: impl Into<PathBuf>, manifest: Manifest) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            manifest,
            policy: Policy::deny_all(),
            limits: HostLimits::default(),
            clock: HostClock::System,
            platform: host_platform(),
        }
    }
}

/// A contributed command as the host registers it: the contract-shaped entry plus the
/// provider attribution the host sets (spec §31.64 — origin is never a manifest field).
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredCommand {
    /// The contribution, exactly as the package declared it.
    pub contribution: CommandContribution,
    /// `plugin:<package.id>`, set by the host at registration.
    pub provider: String,
}

/// A contributed target as the host registers it.
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredTarget {
    /// The contribution, exactly as the package declared it.
    pub contribution: TargetContribution,
    /// `plugin:<package.id>`, set by the host at registration.
    pub provider: String,
}

/// One delivery on an invocation's output stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A typed value, decoded from the lossless encoding and provenance-stamped by the host.
    Value(Value),
    /// A per-element or terminal failure. A stream can carry an error and continue
    /// (spec §16.5); a terminal one is followed by the end of the stream.
    Failed(WireError),
}

/// The supervisor: loads plugin instances (spec §31.11).
#[derive(Debug)]
pub struct Supervisor;

impl Supervisor {
    /// Loads a plugin: negotiates capabilities, spawns the artifact, drives the handshake, and
    /// returns the handle the shell wires into its registries.
    ///
    /// # Errors
    ///
    /// - `package.incompatible` before anything is spawned, when the host or platform is
    ///   outside the manifest's ranges;
    /// - `load.capability_denied` before anything is spawned, when a required capability has no
    ///   grant — the package stays enabled and unloaded, and no package code has run;
    /// - `package.invalid` when the hello does not match the manifest, or a contribution breaks
    ///   a namespacing rule;
    /// - `runtime.protocol_violation` / `runtime.trap` when the instance misframes or dies
    ///   during the handshake.
    pub async fn load(config: LoadConfig) -> Result<LoadedPlugin, KuangError> {
        let LoadConfig {
            program,
            args,
            manifest,
            policy,
            limits,
            clock,
            platform,
        } = config;
        manifest.check_host(HOST_API, &platform)?;
        // Negotiation before code: a denied required capability means nothing is spawned.
        let contract = negotiate(&manifest, &policy, &limits)?;
        let frame_limits = FrameLimits {
            max_frame: contract.limits.max_frame,
        };
        let mut child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| {
                KuangError::new(
                    KuangErrorCode::LoadRuntimeUnavailable,
                    format!("cannot start `{}`: {error}", program.display()),
                )
            })?;
        let stdin = child.stdin.take().ok_or_else(broken_pipes)?;
        let stdout = child.stdout.take().ok_or_else(broken_pipes)?;
        let (frame_tx, frame_rx) = mpsc::channel(64);
        tokio::spawn(read_frames(stdout, frame_limits, frame_tx));

        let deadline = Duration::from_millis(contract.limits.call_deadline_ms);
        let handshake = Handshake {
            child: &mut child,
            frames: frame_rx,
            stdin,
            manifest: &manifest,
            contract: &contract,
            frame_limits,
            deadline,
        };
        let (frames, stdin, hello, init) = handshake.run().await?;

        let mut schemas = ono_value::builtin_schemas().clone();
        let mut commands = Vec::new();
        let mut targets = Vec::new();
        let package_id = manifest.package.id.clone();
        let provider = format!("plugin:{package_id}");
        for schema in &hello.contributions.schemas {
            ono_kuang_protocol::validate_contributed_id(&package_id, "schema", &schema.id)?;
            let built = schema.to_schema()?;
            schemas.register(built).map_err(|error| {
                KuangError::new(
                    KuangErrorCode::PackageInvalid,
                    format!("schema `{}` was refused: {error}", schema.id),
                )
            })?;
        }
        for command in &hello.contributions.commands {
            validate_command_contribution(&package_id, command)?;
            commands.push(RegisteredCommand {
                contribution: command.clone(),
                provider: provider.clone(),
            });
        }
        for target in &hello.contributions.targets {
            if !target.schema.starts_with(&format!("{package_id}."))
                && !target.schema.starts_with("ono.")
            {
                return Err(KuangError::new(
                    KuangErrorCode::PackageInvalid,
                    format!(
                        "target `{}` names schema `{}`, which is neither contributed nor core",
                        target.name, target.schema
                    ),
                ));
            }
            targets.push(RegisteredTarget {
                contribution: target.clone(),
                provider: provider.clone(),
            });
        }

        let mut lifecycle = Lifecycle::installed();
        let enabled = lifecycle.enable();
        let loaded = lifecycle.load(contract.degraded);
        debug_assert!(enabled.is_ok() && loaded.is_ok());
        let shared = Arc::new(Mutex::new(Shared {
            lifecycle,
            logs: Vec::new(),
            plugin_events: Vec::new(),
            last_failure: None,
        }));
        let audit = AuditTrail::for_source(&package_id);
        let (msg_tx, msg_rx) = mpsc::channel(64);
        let actor = Actor {
            child,
            stdin,
            frames,
            msgs: msg_rx,
            shared: Arc::clone(&shared),
            audit: audit.clone(),
            policy,
            clock,
            contract: contract.clone(),
            state: StateStore::new(contract.limits.state_quota),
            package_id: package_id.clone(),
            commands: commands.clone(),
            targets: targets.clone(),
            seq: 2,
            pending: HashMap::new(),
            streams: HashMap::new(),
            next_handle: 1,
            invocation_counter: 0,
            invocations: HashMap::new(),
            frame_limits,
            shutting_down: false,
            schemas,
            msg_sender: msg_tx.clone(),
        };
        tokio::spawn(actor.run());
        Ok(LoadedPlugin {
            package_id,
            shared,
            contract,
            disabled_features: init.disabled_features,
            commands,
            targets,
            audit,
            to_actor: msg_tx,
        })
    }
}

fn broken_pipes() -> KuangError {
    KuangError::new(
        KuangErrorCode::LoadRuntimeUnavailable,
        "the spawned instance has no stdio pipes",
    )
}

fn protocol_violation(detail: impl std::fmt::Display) -> KuangError {
    KuangError::new(
        KuangErrorCode::RuntimeProtocolViolation,
        format!("the plugin violated the negotiated protocol: {detail}"),
    )
    .with_help("this is a defect in the package, not in your pipeline; the instance is quarantined")
}

fn validate_command_contribution(
    package_id: &str,
    command: &CommandContribution,
) -> Result<(), KuangError> {
    ono_kuang_protocol::validate_contributed_id(package_id, "command", &command.id)?;
    let expected_prefix = format!("{package_id}.command.");
    if !command.id.starts_with(&expected_prefix) {
        return Err(KuangError::new(
            KuangErrorCode::PackageInvalid,
            format!(
                "command id `{}` is not `<package.id>.command.<kebab-name>` (spec §31.5)",
                command.id
            ),
        ));
    }
    for capability in &command.capabilities {
        let _: ono_kuang_protocol::Capability = capability.parse()?;
    }
    Ok(())
}

// --- handshake ---------------------------------------------------------------------------------

struct Handshake<'a> {
    child: &'a mut Child,
    frames: mpsc::Receiver<Result<Envelope, FrameError>>,
    stdin: ChildStdin,
    manifest: &'a Manifest,
    contract: &'a PluginContract,
    frame_limits: FrameLimits,
    deadline: Duration,
}

impl Handshake<'_> {
    async fn run(
        mut self,
    ) -> Result<
        (
            mpsc::Receiver<Result<Envelope, FrameError>>,
            ChildStdin,
            Hello,
            InitResult,
        ),
        KuangError,
    > {
        let result = self.drive().await;
        match result {
            Ok((hello, init)) => Ok((self.frames, self.stdin, hello, init)),
            Err(error) => {
                let _ = self.child.kill().await;
                Err(error)
            }
        }
    }

    async fn next_envelope(&mut self) -> Result<Envelope, KuangError> {
        let frame = tokio::time::timeout(self.deadline, self.frames.recv())
            .await
            .map_err(|_| {
                KuangError::new(
                    KuangErrorCode::RuntimeTimeout,
                    "the instance did not answer within the handshake deadline",
                )
            })?;
        match frame {
            Some(Ok(envelope)) => Ok(envelope),
            Some(Err(error)) => Err(protocol_violation(error)),
            None => Err(KuangError::new(
                KuangErrorCode::RuntimeTrap,
                "the instance exited during the handshake",
            )),
        }
    }

    async fn drive(&mut self) -> Result<(Hello, InitResult), KuangError> {
        let invalid = |detail: String| KuangError::new(KuangErrorCode::PackageInvalid, detail);
        let Envelope::Hello(hello) = self.next_envelope().await? else {
            return Err(protocol_violation("the first frame was not a hello"));
        };
        if hello.format != PACKAGE_FORMAT {
            return Err(invalid(format!(
                "the instance speaks `{}`, this host reads `{PACKAGE_FORMAT}`",
                hello.format
            )));
        }
        if hello.package != self.manifest.package.id
            || hello.version != self.manifest.package.version
        {
            return Err(invalid(format!(
                "the instance identifies as `{}@{}`, the manifest says `{}@{}`",
                hello.package,
                hello.version,
                self.manifest.package.id,
                self.manifest.package.version
            )));
        }
        let range: VersionRange = hello.kuang_api.parse()?;
        if !range.contains(HOST_API) {
            return Err(KuangError::new(
                KuangErrorCode::PackageIncompatible,
                format!(
                    "the instance speaks kuang-host `{}`, this host is `{HOST_API}`",
                    hello.kuang_api
                ),
            ));
        }
        let init_request = Envelope::Request {
            seq: 1,
            method: method::LIFECYCLE_INIT.to_owned(),
            params: serde_json::to_value(InitParams {
                contract: self.contract.clone(),
            })
            .unwrap_or(Json::Null),
        };
        write_envelope(&mut self.stdin, &init_request, self.frame_limits).await?;
        let init: InitResult = match self.next_envelope().await? {
            Envelope::Response {
                seq: 1,
                result,
                error,
            } => {
                if let Some(error) = error {
                    return Err(error.to_kuang_error());
                }
                serde_json::from_value(result.unwrap_or(Json::Null)).map_err(protocol_violation)?
            }
            _ => return Err(protocol_violation("expected the answer to lifecycle.init")),
        };
        if !init.ready {
            let detail = init.error.as_ref().map_or_else(
                || "the instance reported not ready".to_owned(),
                WireError::to_string,
            );
            return Err(KuangError::new(KuangErrorCode::RuntimeTrap, detail));
        }
        Ok((hello, init))
    }
}

async fn write_envelope(
    stdin: &mut ChildStdin,
    envelope: &Envelope,
    limits: FrameLimits,
) -> Result<(), KuangError> {
    let frame = ono_kuang_protocol::encode_frame(envelope, limits)
        .map_err(|error| protocol_violation(format!("host-side encoding failed: {error}")))?;
    stdin.write_all(&frame).await.map_err(|error| {
        KuangError::new(
            KuangErrorCode::RuntimeTrap,
            format!("the instance's stdin closed: {error}"),
        )
    })?;
    stdin.flush().await.map_err(|error| {
        KuangError::new(
            KuangErrorCode::RuntimeTrap,
            format!("the instance's stdin closed: {error}"),
        )
    })
}

async fn read_frames(
    mut stdout: ChildStdout,
    limits: FrameLimits,
    tx: mpsc::Sender<Result<Envelope, FrameError>>,
) {
    loop {
        let mut header = [0u8; 4];
        match stdout.read_exact(&mut header).await {
            Ok(_) => {}
            Err(_) => return,
        }
        let declared = u32::from_be_bytes(header);
        if declared > limits.max_frame {
            let _ = tx
                .send(Err(FrameError::TooLarge {
                    declared,
                    ceiling: limits.max_frame,
                }))
                .await;
            return;
        }
        let mut payload = vec![0u8; declared as usize];
        if stdout.read_exact(&mut payload).await.is_err() {
            let _ = tx
                .send(Err(FrameError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "the frame ended mid-payload",
                ))))
                .await;
            return;
        }
        match decode_payload(&payload) {
            Ok(envelope) => {
                if tx.send(Ok(envelope)).await.is_err() {
                    return;
                }
            }
            Err(error) => {
                let _ = tx.send(Err(error)).await;
                return;
            }
        }
    }
}

// --- the handle --------------------------------------------------------------------------------

#[derive(Debug)]
struct Shared {
    lifecycle: Lifecycle,
    logs: Vec<AuditLogParams>,
    plugin_events: Vec<Json>,
    last_failure: Option<KuangError>,
}

fn lock<'a, T>(mutex: &'a Mutex<T>) -> std::sync::MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// A loaded plugin instance: the typed surface the shell wires into its registries and drives
/// invocations through.
#[derive(Debug)]
pub struct LoadedPlugin {
    package_id: String,
    shared: Arc<Mutex<Shared>>,
    contract: PluginContract,
    disabled_features: Vec<String>,
    commands: Vec<RegisteredCommand>,
    targets: Vec<RegisteredTarget>,
    audit: AuditTrail,
    to_actor: mpsc::Sender<ActorMsg>,
}

impl LoadedPlugin {
    /// The package id this instance runs.
    #[must_use]
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    /// The flat lifecycle state `get plugin` shows (spec §31.8).
    #[must_use]
    pub fn state(&self) -> PluginState {
        lock(&self.shared).lifecycle.state()
    }

    /// Why the instance was quarantined, when it was.
    #[must_use]
    pub fn quarantine_reason(&self) -> Option<String> {
        lock(&self.shared)
            .lifecycle
            .quarantine_reason()
            .map(str::to_owned)
    }

    /// The failure that ended the instance, when one did.
    #[must_use]
    pub fn last_failure(&self) -> Option<KuangError> {
        lock(&self.shared).last_failure.clone()
    }

    /// The negotiated contract of spec §31.63.
    #[must_use]
    pub const fn contract(&self) -> &PluginContract {
        &self.contract
    }

    /// The features the plugin disabled in answer to denied optional capabilities.
    #[must_use]
    pub fn disabled_features(&self) -> &[String] {
        &self.disabled_features
    }

    /// The contributed commands, contract-shaped and provider-stamped, ready for registration.
    #[must_use]
    pub fn commands(&self) -> &[RegisteredCommand] {
        &self.commands
    }

    /// The contributed targets.
    #[must_use]
    pub fn targets(&self) -> &[RegisteredTarget] {
        &self.targets
    }

    /// A snapshot of the audit trail (spec §31.37).
    #[must_use]
    pub fn audit(&self) -> Vec<ono_kuang_protocol::AuditEvent> {
        self.audit.snapshot()
    }

    /// The structured log records the plugin emitted (spec §31.33).
    #[must_use]
    pub fn logs(&self) -> Vec<AuditLogParams> {
        lock(&self.shared).logs.clone()
    }

    /// Runs a contributed command (spec §31.22, §31.29).
    ///
    /// # Errors
    ///
    /// `resolve.command_not_found` for a command the package did not contribute;
    /// `capability.denied` when a capability the command declares is not granted (the check is
    /// at invocation, not only at load); the instance's failure when it is gone.
    pub async fn invoke(
        &self,
        command: &str,
        arguments: JsonMap<String, Json>,
    ) -> Result<RunningInvocation, WireError> {
        let (respond, receive) = oneshot::channel();
        self.to_actor
            .send(ActorMsg::Invoke {
                command: command.to_owned(),
                arguments,
                respond,
            })
            .await
            .map_err(|_| self.gone())?;
        receive.await.map_err(|_| self.gone())?
    }

    /// Queries a contributed target: the provider path of spec §31.23, forwarded over the
    /// protocol, answered as a value stream.
    ///
    /// # Errors
    ///
    /// As for [`Self::invoke`], with `resolve.target_not_found` for an unknown target.
    pub async fn query(
        &self,
        target: &str,
        options: JsonMap<String, Json>,
    ) -> Result<RunningInvocation, WireError> {
        let (respond, receive) = oneshot::channel();
        self.to_actor
            .send(ActorMsg::Query {
                target: target.to_owned(),
                options,
                respond,
            })
            .await
            .map_err(|_| self.gone())?;
        receive.await.map_err(|_| self.gone())?
    }

    /// Probes the instance's health (spec §31.35).
    ///
    /// # Errors
    ///
    /// The instance's failure when it is gone or answers outside the contract.
    pub async fn probe(&self) -> Result<ProbeResult, WireError> {
        let (respond, receive) = oneshot::channel();
        self.to_actor
            .send(ActorMsg::Probe { respond })
            .await
            .map_err(|_| self.gone())?;
        receive.await.map_err(|_| self.gone())?
    }

    /// Replaces the policy the broker evaluates from now on — a grant made or revoked at runtime
    /// (spec §31.18, §31.19). The next capability check, at the next call, sees it; nothing
    /// already granted to a running invocation is interrupted.
    pub async fn update_policy(&self, policy: Policy) {
        let (respond, receive) = oneshot::channel();
        if self
            .to_actor
            .send(ActorMsg::SetPolicy { policy, respond })
            .await
            .is_ok()
        {
            let _ = receive.await;
        }
    }

    /// Shuts the instance down: `lifecycle.shutdown` with a drain deadline, then termination
    /// (spec §31.8's unload).
    pub async fn shutdown(&self, reason: ShutdownReason) {
        let (respond, receive) = oneshot::channel();
        if self
            .to_actor
            .send(ActorMsg::Shutdown { reason, respond })
            .await
            .is_ok()
        {
            let _ = receive.await;
        }
    }

    fn gone(&self) -> WireError {
        self.last_failure().map_or_else(
            || {
                KuangError::new(
                    KuangErrorCode::RuntimeTrap,
                    "the plugin instance is no longer running",
                )
                .into()
            },
            WireError::from,
        )
    }
}

/// One running invocation: the value stream and the completion status.
#[derive(Debug)]
pub struct RunningInvocation {
    handle: u64,
    values: mpsc::UnboundedReceiver<StreamEvent>,
    result: oneshot::Receiver<InvokeResult>,
    to_actor: mpsc::Sender<ActorMsg>,
}

impl RunningInvocation {
    /// The next stream delivery, or `None` when the stream has ended. Taking a value grants the
    /// producer one more credit — consumption is what creates demand (ADR-0022 §8).
    pub async fn next(&mut self) -> Option<StreamEvent> {
        let event = self.values.recv().await;
        if event.is_some() {
            let _ = self
                .to_actor
                .send(ActorMsg::Demand {
                    handle: self.handle,
                    credit: 1,
                })
                .await;
        }
        event
    }

    /// Cancels the invocation's output stream. Cancellation is delivered to the plugin, not
    /// inferred from a stalled stream (spec §31.14).
    pub async fn cancel(&self) {
        let _ = self
            .to_actor
            .send(ActorMsg::CancelStream {
                handle: self.handle,
            })
            .await;
    }

    /// Waits for the plugin's completion answer, without consuming remaining values.
    pub async fn finish(self) -> InvokeResult {
        self.result.await.unwrap_or(InvokeResult {
            status: InvokeStatus::Failed,
            error: Some(
                KuangError::new(
                    KuangErrorCode::RuntimeTrap,
                    "the instance ended before answering the invocation",
                )
                .into(),
            ),
        })
    }

    /// Drains the stream, then waits for completion.
    pub async fn collect(mut self) -> (Vec<StreamEvent>, InvokeResult) {
        let mut events = Vec::new();
        while let Some(event) = self.next().await {
            events.push(event);
        }
        let result = self.finish().await;
        (events, result)
    }
}

// --- the actor ---------------------------------------------------------------------------------

enum ActorMsg {
    Invoke {
        command: String,
        arguments: JsonMap<String, Json>,
        respond: oneshot::Sender<Result<RunningInvocation, WireError>>,
    },
    Query {
        target: String,
        options: JsonMap<String, Json>,
        respond: oneshot::Sender<Result<RunningInvocation, WireError>>,
    },
    Demand {
        handle: u64,
        credit: u32,
    },
    CancelStream {
        handle: u64,
    },
    Probe {
        respond: oneshot::Sender<Result<ProbeResult, WireError>>,
    },
    Shutdown {
        reason: ShutdownReason,
        respond: oneshot::Sender<()>,
    },
    SetPolicy {
        policy: Policy,
        respond: oneshot::Sender<()>,
    },
}

enum Pending {
    Invocation {
        result: oneshot::Sender<InvokeResult>,
        output: u64,
        invocation: u64,
    },
    Probe(oneshot::Sender<Result<ProbeResult, WireError>>),
    Shutdown(oneshot::Sender<()>),
    FireAndForget,
}

struct OutStream {
    tx: mpsc::UnboundedSender<StreamEvent>,
    credit: u32,
    cancelled: bool,
    expected: Expected,
}

enum Expected {
    Schema(ono_value::SchemaId),
    Type(ono_value::FieldType),
    Any,
}

struct Actor {
    child: Child,
    stdin: ChildStdin,
    frames: mpsc::Receiver<Result<Envelope, FrameError>>,
    msgs: mpsc::Receiver<ActorMsg>,
    shared: Arc<Mutex<Shared>>,
    audit: AuditTrail,
    policy: Policy,
    clock: HostClock,
    contract: PluginContract,
    state: StateStore,
    package_id: String,
    commands: Vec<RegisteredCommand>,
    targets: Vec<RegisteredTarget>,
    seq: u64,
    pending: HashMap<u64, Pending>,
    streams: HashMap<u64, OutStream>,
    next_handle: u64,
    invocation_counter: u64,
    invocations: HashMap<u64, String>,
    frame_limits: FrameLimits,
    shutting_down: bool,
    schemas: SchemaRegistry,
    msg_sender: mpsc::Sender<ActorMsg>,
}

enum LoopStep {
    Continue,
    Stop,
}

impl Actor {
    async fn run(mut self) {
        loop {
            tokio::select! {
                frame = self.frames.recv() => match frame {
                    Some(Ok(envelope)) => match self.handle_envelope(envelope).await {
                        Ok(LoopStep::Continue) => {}
                        Ok(LoopStep::Stop) => break,
                        Err(violation) => {
                            self.quarantine(violation).await;
                            break;
                        }
                    },
                    Some(Err(frame_error)) => {
                        self.quarantine(protocol_violation(frame_error)).await;
                        break;
                    }
                    None => {
                        if !self.shutting_down {
                            self.fail_instance(KuangError::new(
                                KuangErrorCode::RuntimeTrap,
                                "the plugin instance exited unexpectedly",
                            ))
                            .await;
                        }
                        break;
                    }
                },
                msg = self.msgs.recv() => match msg {
                    Some(msg) => match self.handle_msg(msg).await {
                        LoopStep::Continue => {}
                        LoopStep::Stop => break,
                    },
                    None => {
                        // The handle is gone; stop the instance quietly.
                        self.shutting_down = true;
                        let _ = self.child.kill().await;
                        break;
                    }
                },
            }
        }
    }

    fn now(&self) -> String {
        self.clock.now()
    }

    fn invocation_label(&self) -> String {
        // One running invocation labels the trail precisely; anything else is session-scoped.
        if self.invocations.len() == 1 {
            self.invocations
                .values()
                .next()
                .cloned()
                .unwrap_or_else(|| "session".to_owned())
        } else {
            "session".to_owned()
        }
    }

    async fn send(&mut self, envelope: &Envelope) -> Result<(), KuangError> {
        write_envelope(&mut self.stdin, envelope, self.frame_limits).await
    }

    async fn reply_ok(&mut self, seq: u64, result: Json) {
        let envelope = Envelope::Response {
            seq,
            result: Some(result),
            error: None,
        };
        let _ = self.send(&envelope).await;
    }

    async fn reply_err(&mut self, seq: u64, error: WireError) {
        let envelope = Envelope::Response {
            seq,
            result: None,
            error: Some(error),
        };
        let _ = self.send(&envelope).await;
    }

    /// Ends the instance for a protocol violation: kill, quarantine, close every stream with
    /// the violation, resolve every pending invocation as failed (spec §31.34, ADR-0041).
    async fn quarantine(&mut self, violation: KuangError) {
        let _ = self.child.kill().await;
        {
            let mut shared = lock(&self.shared);
            shared.lifecycle.quarantine(violation.message());
            shared.last_failure = Some(violation.clone());
        }
        self.close_everything(&violation);
    }

    /// Ends the instance for a crash: streams close with `runtime.trap`, the package is not
    /// quarantined — failure degrades the plugin, not the shell (spec §31.34).
    async fn fail_instance(&mut self, failure: KuangError) {
        let _ = self.child.kill().await;
        {
            let mut shared = lock(&self.shared);
            shared.last_failure = Some(failure.clone());
            // Drain the lifecycle: every invocation ends, then the instance unloads.
            while shared.lifecycle.end_invocation().is_ok() {}
            let _ = shared.lifecycle.unload();
        }
        self.close_everything(&failure);
    }

    fn close_everything(&mut self, error: &KuangError) {
        for (_, stream) in self.streams.drain() {
            let _ = stream.tx.send(StreamEvent::Failed(error.into()));
        }
        for (_, pending) in self.pending.drain() {
            match pending {
                Pending::Invocation { result, .. } => {
                    let _ = result.send(InvokeResult {
                        status: InvokeStatus::Failed,
                        error: Some(error.into()),
                    });
                }
                Pending::Probe(respond) => {
                    let _ = respond.send(Err(error.into()));
                }
                Pending::Shutdown(respond) => {
                    let _ = respond.send(());
                }
                Pending::FireAndForget => {}
            }
        }
    }

    async fn handle_msg(&mut self, msg: ActorMsg) -> LoopStep {
        match msg {
            ActorMsg::Invoke {
                command,
                arguments,
                respond,
            } => {
                let started = self
                    .start_invocation(InvocationKind::Command(command), arguments)
                    .await;
                let _ = respond.send(started);
                LoopStep::Continue
            }
            ActorMsg::Query {
                target,
                options,
                respond,
            } => {
                let started = self
                    .start_invocation(InvocationKind::Target(target), options)
                    .await;
                let _ = respond.send(started);
                LoopStep::Continue
            }
            ActorMsg::SetPolicy { policy, respond } => {
                self.policy = policy;
                let _ = respond.send(());
                LoopStep::Continue
            }
            ActorMsg::Demand { handle, credit } => {
                if let Some(stream) = self.streams.get_mut(&handle)
                    && !stream.cancelled
                {
                    {
                        stream.credit = stream.credit.saturating_add(credit);
                        let seq = self.next_seq(Pending::FireAndForget);
                        let envelope = Envelope::Request {
                            seq,
                            method: method::STREAM_DEMAND.to_owned(),
                            params: serde_json::to_value(DemandParams { handle, credit })
                                .unwrap_or(Json::Null),
                        };
                        let _ = self.send(&envelope).await;
                    }
                }
                LoopStep::Continue
            }
            ActorMsg::CancelStream { handle } => {
                if let Some(stream) = self.streams.get_mut(&handle) {
                    stream.cancelled = true;
                    let seq = self.next_seq(Pending::FireAndForget);
                    let envelope = Envelope::Request {
                        seq,
                        method: method::STREAM_CANCEL.to_owned(),
                        params: serde_json::to_value(CancelParams {
                            handle,
                            reason: CancelReason::Operator,
                        })
                        .unwrap_or(Json::Null),
                    };
                    let _ = self.send(&envelope).await;
                }
                LoopStep::Continue
            }
            ActorMsg::Probe { respond } => {
                let seq = self.next_seq(Pending::Probe(respond));
                let envelope = Envelope::Request {
                    seq,
                    method: method::HEALTH_PROBE.to_owned(),
                    params: json!({}),
                };
                let _ = self.send(&envelope).await;
                LoopStep::Continue
            }
            ActorMsg::Shutdown { reason, respond } => {
                self.shutting_down = true;
                let deadline_ms = self.contract.limits.call_deadline_ms;
                let seq = self.next_seq(Pending::Shutdown(respond));
                let envelope = Envelope::Request {
                    seq,
                    method: method::LIFECYCLE_SHUTDOWN.to_owned(),
                    params: serde_json::to_value(ShutdownParams {
                        reason,
                        deadline_ms,
                    })
                    .unwrap_or(Json::Null),
                };
                let _ = self.send(&envelope).await;
                let deadline = Duration::from_millis(deadline_ms);
                let acknowledged = tokio::time::timeout(deadline, async {
                    loop {
                        match self.frames.recv().await {
                            Some(Ok(Envelope::Response { seq: answered, .. }))
                                if answered == seq =>
                            {
                                break;
                            }
                            Some(_) => {}
                            None => break,
                        }
                    }
                })
                .await;
                let _ = acknowledged;
                let _ = self.child.kill().await;
                {
                    let mut shared = lock(&self.shared);
                    while shared.lifecycle.end_invocation().is_ok() {}
                    let _ = shared.lifecycle.unload();
                }
                let cancelled =
                    KuangError::new(KuangErrorCode::RuntimeTrap, "the instance was shut down");
                if let Some(Pending::Shutdown(respond)) = self.pending.remove(&seq) {
                    let _ = respond.send(());
                }
                self.close_everything(&cancelled);
                LoopStep::Stop
            }
        }
    }

    fn next_seq(&mut self, pending: Pending) -> u64 {
        let seq = self.seq;
        self.seq += 1;
        self.pending.insert(seq, pending);
        seq
    }

    async fn start_invocation(
        &mut self,
        kind: InvocationKind,
        arguments: JsonMap<String, Json>,
    ) -> Result<RunningInvocation, WireError> {
        let (label, expected, capabilities, method_name, params_builder): (
            String,
            Expected,
            Vec<String>,
            &str,
            _,
        );
        match &kind {
            InvocationKind::Command(command) => {
                let Some(registered) = self
                    .commands
                    .iter()
                    .find(|candidate| candidate.contribution.id == *command)
                else {
                    return Err(WireError::from_core(
                        ono_core::ErrorCode::ResolveCommandNotFound,
                        format!("the package contributes no command `{command}`"),
                    ));
                };
                label = format!("command:{command}");
                expected = parse_expected(&registered.contribution.output);
                capabilities = registered.contribution.capabilities.clone();
                method_name = method::COMMAND_INVOKE;
                params_builder = ParamsBuilder::Command(command.clone());
            }
            InvocationKind::Target(target) => {
                let Some(registered) = self
                    .targets
                    .iter()
                    .find(|candidate| candidate.contribution.name == *target)
                else {
                    return Err(WireError::from_core(
                        ono_core::ErrorCode::ResolveTargetNotFound,
                        format!("the package contributes no target `{target}`"),
                    ));
                };
                label = format!("query:{target}");
                expected =
                    Expected::Schema(registered.contribution.schema.parse().map_err(|_| {
                        WireError::from_core(
                            ono_core::ErrorCode::ProviderSchemaViolation,
                            format!(
                                "target `{target}` declares unparseable schema `{}`",
                                registered.contribution.schema
                            ),
                        )
                    })?);
                capabilities = Vec::new();
                method_name = method::PROVIDER_QUERY;
                params_builder = ParamsBuilder::Target(target.clone());
            }
        }
        // The invoked contribution's own capabilities are checked against *the plugin's*
        // grants at the moment of the call, not at load time (protocol.v1.yaml, lifecycle
        // `activate`).
        for capability_id in &capabilities {
            let Some(capability) = ono_kuang_protocol::Capability::from_id(capability_id) else {
                continue;
            };
            let evaluation = self.policy.evaluate(capability, &[]);
            if !matches!(evaluation, Evaluation::Allowed(_)) {
                let error = denial_error(capability, &evaluation);
                self.audit.record(
                    &self.package_id,
                    &label,
                    capability_id,
                    None,
                    Enforcement::Broker,
                    "command.invoke",
                    Some(Json::String(label.clone())),
                    self.now(),
                    AuditResult::Denied,
                    Some((&error).into()),
                );
                return Err(error.into());
            }
        }
        {
            let mut shared = lock(&self.shared);
            if shared.lifecycle.begin_invocation().is_err() {
                return Err(KuangError::new(
                    KuangErrorCode::RuntimeTrap,
                    "the instance is not in a loadable state for invocations",
                )
                .into());
            }
        }
        self.invocation_counter += 1;
        let invocation = self.invocation_counter;
        let output = self.next_handle;
        self.next_handle += 1;
        let credit = self.contract.limits.queue_depth;
        let (tx, rx) = mpsc::unbounded_channel();
        self.streams.insert(
            output,
            OutStream {
                tx,
                credit,
                cancelled: false,
                expected,
            },
        );
        self.invocations.insert(invocation, label.clone());
        let (result_tx, result_rx) = oneshot::channel();
        let params = match params_builder {
            ParamsBuilder::Command(command) => serde_json::to_value(InvokeParams {
                command,
                arguments,
                output,
                invocation,
                credit,
            }),
            ParamsBuilder::Target(target) => serde_json::to_value(QueryParams {
                target,
                options: arguments,
                output,
                invocation,
                credit,
            }),
        }
        .unwrap_or(Json::Null);
        let seq = self.next_seq(Pending::Invocation {
            result: result_tx,
            output,
            invocation,
        });
        let envelope = Envelope::Request {
            seq,
            method: method_name.to_owned(),
            params,
        };
        if let Err(error) = self.send(&envelope).await {
            self.fail_instance(error.clone()).await;
            return Err(error.into());
        }
        Ok(RunningInvocation {
            handle: output,
            values: rx,
            result: result_rx,
            to_actor: self.handle_sender(),
        })
    }

    fn handle_sender(&self) -> mpsc::Sender<ActorMsg> {
        // The actor cannot hold its own receiver's sender permanently (it would never close);
        // hand out a clone lazily through the shared config instead.
        self.msg_sender.clone()
    }
}

enum InvocationKind {
    Command(String),
    Target(String),
}

enum ParamsBuilder {
    Command(String),
    Target(String),
}

fn parse_expected(output: &str) -> Expected {
    let inner = output
        .strip_prefix("stream<")
        .or_else(|| output.strip_prefix("graph<"))
        .and_then(|rest| rest.strip_suffix('>'))
        .unwrap_or(output);
    if let Some(field_type) = ono_kuang_protocol::parse_type_name(inner) {
        return Expected::Type(field_type);
    }
    if inner.contains('/')
        && let Ok(id) = inner.parse()
    {
        return Expected::Schema(id);
    }
    Expected::Any
}

// --- plugin -> host dispatch -------------------------------------------------------------------

impl Actor {
    async fn handle_envelope(&mut self, envelope: Envelope) -> Result<LoopStep, KuangError> {
        match envelope {
            Envelope::Hello(_) => Err(protocol_violation("a second hello after the handshake")),
            Envelope::Response { seq, result, error } => {
                let Some(pending) = self.pending.remove(&seq) else {
                    return Err(protocol_violation(format!(
                        "an answer to a request nobody made (seq {seq})"
                    )));
                };
                match pending {
                    Pending::Invocation {
                        result: result_tx,
                        output,
                        invocation,
                    } => {
                        let outcome: InvokeResult = if let Some(error) = error {
                            InvokeResult {
                                status: InvokeStatus::Failed,
                                error: Some(error),
                            }
                        } else {
                            serde_json::from_value(result.unwrap_or(Json::Null))
                                .map_err(protocol_violation)?
                        };
                        {
                            let mut shared = lock(&self.shared);
                            let _ = shared.lifecycle.end_invocation();
                        }
                        self.invocations.remove(&invocation);
                        self.streams.remove(&output);
                        let _ = result_tx.send(outcome);
                        Ok(LoopStep::Continue)
                    }
                    Pending::Probe(respond) => {
                        let answer = if let Some(error) = error {
                            Err(error)
                        } else {
                            serde_json::from_value::<ProbeResult>(result.unwrap_or(Json::Null))
                                .map_err(protocol_violation)
                                .map_err(|violation| WireError::from(&violation))
                        };
                        let _ = respond.send(answer);
                        Ok(LoopStep::Continue)
                    }
                    Pending::Shutdown(respond) => {
                        let _ = respond.send(());
                        Ok(LoopStep::Continue)
                    }
                    Pending::FireAndForget => Ok(LoopStep::Continue),
                }
            }
            Envelope::Request {
                seq,
                method,
                params,
            } => {
                self.dispatch_host_call(seq, &method, params).await?;
                Ok(LoopStep::Continue)
            }
        }
    }

    async fn dispatch_host_call(
        &mut self,
        seq: u64,
        method_name: &str,
        params: Json,
    ) -> Result<(), KuangError> {
        match method_name {
            method::STREAMS_EMIT => self.host_emit(seq, params).await,
            method::STREAMS_CLOSE => self.host_close(seq, params).await,
            method::CAPABILITIES_CHECK => self.host_check(seq, params).await,
            method::CAPABILITIES_REQUEST => self.host_request_once(seq, params).await,
            method::AUDIT_LOG => self.host_audit_log(seq, params).await,
            method::AUDIT_EVENT => self.host_audit_event(seq, params).await,
            method::STATE_GET => self.host_state_get(seq, params).await,
            method::STATE_SET => self.host_state_set(seq, params).await,
            method::STATE_DELETE => self.host_state_delete(seq, params).await,
            method::CLOCK_NOW => self.host_clock_now(seq).await,
            method::FILESYSTEM_READ => self.host_filesystem_read(seq, params).await,
            unknown => Err(protocol_violation(format!(
                "a call to `{unknown}`, which the negotiated host API does not carry"
            ))),
        }
    }

    fn parse_params<T: serde::de::DeserializeOwned>(params: Json) -> Result<T, KuangError> {
        serde_json::from_value(params)
            .map_err(|error| protocol_violation(format!("malformed call parameters: {error}")))
    }

    /// Checks one capability use: evaluates policy against the values the operation will
    /// actually use, audits the outcome either way, and answers the structured denial the
    /// contracts name.
    fn broker_check(
        &mut self,
        capability: ono_kuang_protocol::Capability,
        action: &str,
        used: &[ScopeUse],
        target: Option<Json>,
    ) -> Result<crate::policy::Grant, KuangError> {
        let evaluation = self.policy.evaluate(capability, used);
        let label = self.invocation_label();
        match evaluation {
            Evaluation::Allowed(grant) => {
                self.audit.record(
                    &self.package_id,
                    &label,
                    capability.id(),
                    grant.scope.clone().map(Json::Object),
                    Enforcement::Broker,
                    action,
                    target,
                    self.now(),
                    AuditResult::Success,
                    None,
                );
                Ok(grant)
            }
            refused => {
                let error = denial_error(capability, &refused);
                self.audit.record(
                    &self.package_id,
                    &label,
                    capability.id(),
                    match &refused {
                        Evaluation::ScopeViolation { grant, .. } => {
                            grant.scope.clone().map(Json::Object)
                        }
                        _ => None,
                    },
                    Enforcement::Broker,
                    action,
                    target,
                    self.now(),
                    AuditResult::Denied,
                    Some((&error).into()),
                );
                Err(error)
            }
        }
    }

    async fn host_emit(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let mut emit: EmitParams = Self::parse_params(params)?;
        let Some(stream) = self.streams.get(&emit.handle) else {
            return Err(protocol_violation(format!(
                "an emission into handle {} which is not the plugin's to write",
                emit.handle
            )));
        };
        let (credit, cancelled) = (stream.credit, stream.cancelled);
        let count = u32::try_from(emit.values.len())
            .map_err(|_| protocol_violation("an emission too large to count"))?;
        if count > credit {
            // §31.15: "When a plugin cannot keep up, policy can choose", and the choice is the
            // one negotiated at load. Only `block-upstream` makes an overrun a protocol
            // violation: under it the producer was told to wait and did not.
            let window = credit as usize;
            match self.contract.overflow {
                OverflowPolicy::BlockUpstream => {
                    return Err(protocol_violation(format!(
                        "an emission of {count} values against a credit of {credit}"
                    )));
                }
                OverflowPolicy::FailStream => {
                    let error: WireError = KuangError::new(
                        KuangErrorCode::RuntimeBackpressureFailure,
                        format!(
                            "the stream emitted {count} values against a credit of {credit}, and                              its overflow policy is `fail-stream`"
                        ),
                    )
                    .with_help("spec §31.15: `fail-stream` ends the stream rather than lose data")
                    .into();
                    if let Some(stream) = self.streams.remove(&emit.handle) {
                        let _ = stream.tx.send(StreamEvent::Failed(error.clone()));
                    }
                    // The producer is told too: it asked to emit and the emission did not
                    // happen, which is exactly what an error reply says.
                    self.reply_err(seq, error).await;
                    return Ok(());
                }
                // Explicit only, never inferred: `negotiate` refuses it as a manifest
                // preference, so it is here because host policy said so.
                OverflowPolicy::DropNewest => emit.values.truncate(window),
                OverflowPolicy::DropOldest => {
                    let excess = emit.values.len() - window;
                    emit.values.drain(..excess);
                }
                OverflowPolicy::Coalesce => {
                    emit.values = coalesce_by_identity(std::mem::take(&mut emit.values));
                    if emit.values.len() > window {
                        let excess = emit.values.len() - window;
                        emit.values.drain(..excess);
                    }
                }
            }
            let dropped = count - u32::try_from(emit.values.len()).unwrap_or(credit);
            self.record_overflow(dropped);
        }
        let kept = u32::try_from(emit.values.len()).unwrap_or(credit);
        if let Some(stream) = self.streams.get_mut(&emit.handle) {
            stream.credit = stream.credit.saturating_sub(kept);
        }
        if cancelled {
            // Emissions legitimately in flight after a cancel are dropped, not punished.
            self.reply_ok(
                seq,
                serde_json::to_value(EmitResult { credit: 0 }).unwrap_or(Json::Null),
            )
            .await;
            return Ok(());
        }
        let mut failure: Option<WireError> = None;
        for mut value_json in emit.values {
            restamp_provenance(&mut value_json, &self.package_id);
            let decoded = from_json(&value_json, &self.schemas);
            let stream = match self.streams.get_mut(&emit.handle) {
                Some(stream) => stream,
                None => break,
            };
            match decoded {
                Ok(value) => {
                    if let Some(violation) = schema_violation(&stream.expected, &value) {
                        failure = Some(violation);
                        break;
                    }
                    let _ = stream.tx.send(StreamEvent::Value(value));
                }
                Err(error) => {
                    failure = Some(
                        KuangError::new(
                            KuangErrorCode::RuntimeSchemaViolation,
                            format!("an emitted value could not be decoded: {error}"),
                        )
                        .into(),
                    );
                    break;
                }
            }
        }
        if let Some(violation) = failure {
            // Invalid output closes the stream with the violation; the instance keeps running
            // (spec §31.34's schema-violation class degrades the stream, not the shell).
            if let Some(stream) = self.streams.remove(&emit.handle) {
                let _ = stream.tx.send(StreamEvent::Failed(violation.clone()));
            }
            self.reply_err(seq, violation).await;
            return Ok(());
        }
        let credit = self
            .streams
            .get(&emit.handle)
            .map_or(0, |stream| stream.credit);
        self.reply_ok(
            seq,
            serde_json::to_value(EmitResult { credit }).unwrap_or(Json::Null),
        )
        .await;
        Ok(())
    }

    /// Records that an overrun cost the stream values, in the package's own structured log.
    ///
    /// §31.33's example is exactly this line — `warn event coalescing dropped=421
    /// policy=coalesce` — and §2.17's rule applies: data the shell decided to lose is a fact the
    /// operator has to be able to find, not an absence.
    fn record_overflow(&mut self, dropped: u32) {
        let policy = serde_json::to_value(self.contract.overflow)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut fields = JsonMap::new();
        fields.insert("dropped".to_owned(), Json::from(dropped));
        fields.insert("policy".to_owned(), Json::String(policy));
        lock(&self.shared).logs.push(AuditLogParams {
            level: "warn".to_owned(),
            message: "stream overflow".to_owned(),
            fields,
        });
    }

    async fn host_close(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let close: CloseParams = Self::parse_params(params)?;
        let Some(stream) = self.streams.remove(&close.handle) else {
            return Err(protocol_violation(format!(
                "a close of handle {} which is not the plugin's",
                close.handle
            )));
        };
        if let Some(error) = close.error {
            let _ = stream.tx.send(StreamEvent::Failed(error));
        }
        drop(stream);
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    async fn host_check(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let check: CheckParams = Self::parse_params(params)?;
        let answer = match ono_kuang_protocol::Capability::from_id(&check.capability) {
            None => CheckAnswer::Unknown,
            Some(capability) => {
                if self.policy.grants_capability(capability) {
                    CheckAnswer::Granted
                } else {
                    CheckAnswer::Denied
                }
            }
        };
        self.reply_ok(seq, serde_json::to_value(answer).unwrap_or(Json::Null))
            .await;
        Ok(())
    }

    async fn host_request_once(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let request: RequestOnceParams = Self::parse_params(params)?;
        let label = self.invocation_label();
        let Some(capability) = ono_kuang_protocol::Capability::from_id(&request.capability) else {
            self.reply_err(
                seq,
                KuangError::new(
                    KuangErrorCode::CapabilityDenied,
                    format!("`{}` is not a capability", request.capability),
                )
                .into(),
            )
            .await;
            return Ok(());
        };
        let declared = request.action_context.trim();
        let denied = |message: String| {
            KuangError::new(KuangErrorCode::CapabilityDenied, message).with_help(
                "a runtime request answers an explicit user action, and never re-prompts (spec §31.17)",
            )
        };
        if declared.is_empty() {
            // A request with no user action behind it is denied without prompting.
            let error = denied(format!(
                "runtime request for `{capability}` names no user action"
            ));
            self.audit.record(
                &self.package_id,
                &label,
                capability.id(),
                request.scope.clone().map(Json::Object),
                Enforcement::Broker,
                "capabilities.request",
                None,
                self.now(),
                AuditResult::Denied,
                Some((&error).into()),
            );
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let evaluation = self.policy.evaluate(capability, &[]);
        match evaluation {
            Evaluation::Allowed(_) => {
                let expires_at = lease_expiry(&self.now());
                let lease = Lease {
                    capability: capability.id().to_owned(),
                    selector: "*".to_owned(),
                    actions: None,
                    expires_at,
                    max_uses: Some(1),
                    condition: None,
                };
                self.audit.record(
                    &self.package_id,
                    &label,
                    capability.id(),
                    request.scope.clone().map(Json::Object),
                    Enforcement::Broker,
                    "capabilities.request",
                    None,
                    self.now(),
                    AuditResult::Success,
                    None,
                );
                self.reply_ok(seq, serde_json::to_value(lease).unwrap_or(Json::Null))
                    .await;
            }
            refused => {
                let error = denial_error(capability, &refused);
                self.audit.record(
                    &self.package_id,
                    &label,
                    capability.id(),
                    request.scope.clone().map(Json::Object),
                    Enforcement::Broker,
                    "capabilities.request",
                    None,
                    self.now(),
                    AuditResult::Denied,
                    Some((&error).into()),
                );
                self.reply_err(seq, error.into()).await;
            }
        }
        Ok(())
    }

    async fn host_audit_log(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let record: AuditLogParams = Self::parse_params(params)?;
        lock(&self.shared).logs.push(record);
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    async fn host_audit_event(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        lock(&self.shared).plugin_events.push(params);
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    async fn host_state_get(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let get: StateKeyParams = Self::parse_params(params)?;
        if let Err(error) = self.state_class_check(&get.class, "state.get") {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let value = self.state.get(&get.class, &get.key).cloned();
        self.reply_ok(
            seq,
            serde_json::to_value(StateGetResult { value }).unwrap_or(Json::Null),
        )
        .await;
        Ok(())
    }

    async fn host_state_set(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let set: StateSetParams = Self::parse_params(params)?;
        if let Err(error) = self.state_class_check(&set.class, "state.set") {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        match self.state.set(&set.class, &set.key, set.value) {
            Ok(()) => self.reply_ok(seq, Json::Null).await,
            Err(error) => self.reply_err(seq, error.into()).await,
        }
        Ok(())
    }

    async fn host_state_delete(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let delete: StateKeyParams = Self::parse_params(params)?;
        if let Err(error) = self.state_class_check(&delete.class, "state.delete") {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let removed = self.state.delete(&delete.class, &delete.key);
        self.reply_ok(seq, Json::Bool(removed)).await;
        Ok(())
    }

    /// `persistent` state costs `state.persist`; the other classes are free (spec §31.31).
    fn state_class_check(&mut self, class: &str, action: &str) -> Result<(), KuangError> {
        if class == "persistent" {
            self.broker_check(
                ono_kuang_protocol::Capability::StatePersist,
                action,
                &[],
                None,
            )?;
        }
        Ok(())
    }

    async fn host_clock_now(&mut self, seq: u64) -> Result<(), KuangError> {
        match self.broker_check(
            ono_kuang_protocol::Capability::ClockRead,
            "clock.now",
            &[],
            None,
        ) {
            Ok(_) => {
                let now = self.now();
                self.reply_ok(seq, json!({"now": {"$timestamp": now}}))
                    .await;
            }
            Err(error) => self.reply_err(seq, error.into()).await,
        }
        Ok(())
    }

    async fn host_filesystem_read(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let read: FilesystemReadParams = Self::parse_params(params)?;
        // Check against the value the operation will actually use: the resolved path, with no
        // re-resolution between check and use (ADR-0015 T14).
        let resolved = std::fs::canonicalize(&read.path)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| read.path.clone());
        let target = Some(Json::String(resolved.clone()));
        match self.broker_check(
            ono_kuang_protocol::Capability::FilesystemRead,
            "filesystem.read",
            &[ScopeUse::Path {
                key: "paths",
                value: resolved.clone(),
            }],
            target.clone(),
        ) {
            Ok(_) => {}
            Err(error) => {
                self.reply_err(seq, error.into()).await;
                return Ok(());
            }
        }
        match tokio::fs::read(&resolved).await {
            Ok(mut bytes) => {
                let offset = usize::try_from(read.offset.unwrap_or(0)).unwrap_or(usize::MAX);
                if offset < bytes.len() {
                    bytes.drain(..offset);
                } else if offset > 0 {
                    bytes.clear();
                }
                let ceiling = 64 * 1024;
                let length = read
                    .length
                    .and_then(|length| usize::try_from(length).ok())
                    .unwrap_or(ceiling)
                    .min(ceiling);
                bytes.truncate(length);
                let content = to_json(&Value::Bytes(bytes.into()));
                self.reply_ok(
                    seq,
                    serde_json::to_value(FilesystemReadResult { content }).unwrap_or(Json::Null),
                )
                .await;
            }
            Err(error) => {
                let code = if error.kind() == std::io::ErrorKind::NotFound {
                    ono_core::ErrorCode::IoNotFound
                } else {
                    ono_core::ErrorCode::IoPermissionDenied
                };
                let label = self.invocation_label();
                let wire = WireError::from_core(code, error.to_string());
                self.audit.record(
                    &self.package_id,
                    &label,
                    ono_kuang_protocol::Capability::FilesystemRead.id(),
                    None,
                    Enforcement::Broker,
                    "filesystem.read",
                    target,
                    self.now(),
                    AuditResult::Failed,
                    Some(wire.clone()),
                );
                self.reply_err(seq, wire).await;
            }
        }
        Ok(())
    }
}

/// Sets the provenance provider of a `$record` value to the emitting package — a plugin cannot
/// forge provenance (spec §31.80), so whatever it wrote is overwritten by the host.
fn restamp_provenance(value: &mut Json, package_id: &str) {
    let provider = format!("plugin:{package_id}");
    let Some(record) = value.get_mut("$record") else {
        return;
    };
    let Some(object) = record.as_object_mut() else {
        return;
    };
    let schema = object
        .get("schema")
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_owned();
    match object.get_mut("provenance") {
        Some(Json::Object(provenance)) => {
            provenance.insert("provider".to_owned(), Json::String(provider));
        }
        _ => {
            object.insert(
                "provenance".to_owned(),
                json!({
                    "provider": provider,
                    "observed": Json::Null,
                    "source": Json::Null,
                    "link": "local",
                    "schema": schema,
                    "confidence": Json::Null,
                }),
            );
        }
    }
}

/// Whether `value` violates the output type the contribution declared (spec §31.34's
/// schema-violation class: contributed output is validated, not trusted).
fn schema_violation(expected: &Expected, value: &Value) -> Option<WireError> {
    let violation = |detail: String| {
        Some(
            KuangError::new(KuangErrorCode::RuntimeSchemaViolation, detail)
                .with_help("Ono validates contributed output rather than trusting it")
                .into(),
        )
    };
    match expected {
        Expected::Any => None,
        Expected::Type(field_type) => {
            if field_type.accepts(value) {
                None
            } else {
                violation(format!(
                    "the contribution declares `{}` and the value is `{}`",
                    field_type.name(),
                    value.type_name()
                ))
            }
        }
        Expected::Schema(id) => match value {
            Value::Record(record) if record.schema_id() == id => None,
            Value::Record(record) => violation(format!(
                "the contribution declares schema `{id}` and the value carries `{}`",
                record.schema_id()
            )),
            other => violation(format!(
                "the contribution declares schema `{id}` and the value is a bare `{}`",
                other.type_name()
            )),
        },
    }
}

/// A lease expiry five minutes after `now`, or `now` itself when it cannot be parsed.
fn lease_expiry(now: &str) -> String {
    now.parse::<jiff::Timestamp>()
        .ok()
        .and_then(|instant| instant.checked_add(jiff::Span::new().seconds(300)).ok())
        .map_or_else(|| now.to_owned(), |instant| instant.to_string())
}

/// Combines repeated updates by object identity, keeping the newest of each (spec §31.15).
///
/// "Requires the schema to declare one": a value whose JSON carries no identifiable key is left
/// alone rather than folded into its neighbours, because collapsing two things that were never
/// said to be the same object would lose data while claiming not to.
fn coalesce_by_identity(values: Vec<Json>) -> Vec<Json> {
    let mut newest: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut keep: Vec<bool> = vec![true; values.len()];
    for (index, value) in values.iter().enumerate() {
        let Some(identity) = coalescing_identity(value) else {
            continue;
        };
        if let Some(previous) = newest.insert(identity, index) {
            keep[previous] = false;
        }
    }
    values
        .into_iter()
        .zip(keep)
        .filter_map(|(value, keep)| keep.then_some(value))
        .collect()
}

/// The identity a coalescing policy folds by: the record's schema and its identity fields.
fn coalescing_identity(value: &Json) -> Option<String> {
    let object = value.as_object()?;
    let schema = object.get("schema")?.as_str()?;
    let identity = object.get("identity")?.as_object()?;
    Some(format!("{schema}:{identity:?}"))
}
