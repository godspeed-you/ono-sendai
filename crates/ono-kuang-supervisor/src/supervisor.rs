//! Spawning, handshake, and the per-instance actor that brokers every host call.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ono_kuang_protocol::{
    AuditLogParams, AuditResult, CancelParams, CancelReason, CheckAnswer, CheckParams, CloseParams,
    CommandContribution, DemandParams, EmitParams, EmitResult, Enforcement, Envelope,
    FilesystemReadParams, FilesystemReadResult, FrameError, FrameLimits, HOST_API, Hello,
    InitParams, InitResult, InvokeParams, InvokeResult, InvokeStatus, KuangError, KuangErrorCode,
    Lease, Lifecycle, Manifest, NextParams, NextResult, OverflowPolicy, PACKAGE_FORMAT,
    PluginContract, PluginState, ProbeResult, QueryParams, RequestOnceParams, SchemaGetParams,
    SchemaListParams, ShutdownParams, ShutdownReason, StateGetResult, StateKeyParams,
    StateSetParams, StreamHandleParams, TargetContribution, VersionRange, WireError,
    decode_payload, method,
};
use ono_value::{SchemaRegistry, Value, from_json, to_json};
use serde_json::{Map as JsonMap, Value as Json, json};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

use crate::negotiate::{HostLimits, negotiate};
use crate::platform::{ConfinementPlatform, NativePlatform};
use crate::policy::{Evaluation, Policy, ScopeUse, denial_error};
use crate::report::ConfinementReport;
use crate::sandbox::Sandbox;
use crate::state::StateStore;
use crate::trail::{AuditTrail, HostClock};
use ono_kuang_protocol::ExecutionTier;

/// Whether an instance was at its memory ceiling when the host last looked (spec §31.34).
///
/// It is never *at or above* it: `RLIMIT_DATA` refuses the allocation that would cross the
/// ceiling, so an instance that ran out of room sits just below it and then fails on its next
/// request. The host therefore reads "at its ceiling" as *within a sixteenth of it*, which is the
/// span between the last observation and the refusal.
///
/// This is an inference from an observation, and it is stated as one rather than hidden: the
/// failure message carries the ceiling and the observed figure either way, so an operator can see
/// what the host saw. Making it exact instead of inferred needs the kernel to report the refusal,
/// which on Linux means a cgroup v2 `memory.events` counter, which needs a delegated cgroup the
/// shell does not have as an unprivileged user (ADR-0283).
fn at_memory_ceiling(peak: u64, ceiling: u64) -> bool {
    ceiling > 0 && peak.saturating_mul(16) >= ceiling.saturating_mul(15)
}

/// How often the host reads an instance's allocated memory (spec §31.33).
///
/// Frequent enough that a package that runs for a second is measured, cheap enough that the cost
/// is one small `/proc` read per instance per interval.
const MEMORY_SAMPLE_INTERVAL: Duration = Duration::from_millis(100);

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
    /// The package's own directory under the host's state root — spec §31.31's
    /// `~/.local/state/ono/kuang/<package-id>/`. It is where the instance's private working
    /// directory is made. `None` when the host has no state root, and then the artifact's own
    /// directory serves (spec §31.10, ADR-0283).
    pub private_dir: Option<PathBuf>,
    /// What installs the process-level confinement controls of v0.4.1 §16.1.
    ///
    /// The shell passes [`NativePlatform`], which is also the default. §59.7 requires an
    /// acceptance scenario in which `PR_SET_NO_NEW_PRIVS` fails and the plugin never runs, and
    /// no arrangement outside the process can make that call fail — so the platform is a seam
    /// the caller supplies rather than a constant this module reaches for (ADR-0443).
    pub confinement: Arc<dyn ConfinementPlatform>,
    /// The model broker `models.list` and `models.infer` reach (spec §31.43, ADR-0566). The
    /// default answers for a host with no catalogue: nothing configured, nothing answers.
    pub models: Arc<dyn ono_model_broker::ModelBroker>,
    /// What `context.get` answers with (spec §31.12, ADR-0567). The shell publishes its
    /// session's context; the default is the fixed context of the test host.
    pub context: Arc<dyn crate::context::ContextSource>,
    /// What `objects.*`, `relations.*`, `history.*`, `process.signal` and `secrets.*` reach
    /// (ADR-0568). The default serves nothing and says so.
    pub host: Arc<dyn crate::host::HostServices>,
    /// What takes a view (spec §31.27, ADR-0572): the shell's terminal, or a recorder under
    /// test. The default takes none, so every view falls back.
    pub views: Arc<dyn crate::view::ViewHost>,
}

impl std::fmt::Debug for LoadConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn ConfinementPlatform` is not `Debug`, and requiring it of every implementation
        // would buy nothing: what a reader wants from this is the artifact and the manifest.
        f.debug_struct("LoadConfig")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("manifest", &self.manifest)
            .field("policy", &self.policy)
            .field("limits", &self.limits)
            .field("clock", &self.clock)
            .field("platform", &self.platform)
            .field("private_dir", &self.private_dir)
            .finish_non_exhaustive()
    }
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
            private_dir: None,
            confinement: NativePlatform::shared(),
            models: Arc::new(ono_model_broker::NoModels),
            context: Arc::new(crate::context::FixedContext::test_host()),
            host: Arc::new(crate::host::NoHost),
            views: Arc::new(crate::view::NoViews),
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
            private_dir,
            confinement,
            models,
            context,
            host,
            views,
        } = config;
        manifest.check_host(HOST_API, &platform)?;
        // Negotiation before code: a denied required capability means nothing is spawned.
        let contract = negotiate(&manifest, &policy, &limits)?;
        let frame_limits = FrameLimits {
            max_frame: contract.limits.max_frame,
        };
        // Spec §31.10, §31.15: the artifact runs inside the ceilings the manifest declared and
        // the host capped, in a directory and an environment it did not choose, in its own
        // session. All of it is set between fork and exec, so no package instruction ever runs
        // outside it (ADR-0283).
        let sandbox = crate::sandbox::native_process(
            contract.limits.memory_max.unwrap_or(limits.memory_max),
            manifest
                .runtime
                .as_ref()
                .map_or(ono_kuang_protocol::CpuBudget::Interactive, |runtime| {
                    runtime.cpu_budget
                }),
            crate::sandbox::working_directory(private_dir.as_deref(), &program),
        );
        let is_component = manifest
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.kind == ono_kuang_protocol::RuntimeKind::WasmComponent);
        let (frame_tx, frame_rx) = mpsc::channel(64);
        let (mut child, stdin, confinement, sandbox): (Runtime, Stdin, ConfinementReport, Sandbox) =
            if is_component {
                // Spec §31.10's T2: the component runs inside the runtime Ono embeds, with
                // nothing but its standard streams (ADR-0569). What the shell reports is the
                // component's sandbox, and the report says which controls the runtime installs.
                let sandbox = crate::sandbox::wasm_component(
                    sandbox.memory_max,
                    sandbox.cpu_class,
                    sandbox.working_directory.clone(),
                );
                let confinement =
                    crate::report::parent_only(ExecutionTier::Wasm, &component_controls(&sandbox));
                if let Some(refusal) = confinement.refusal(&manifest.package.id) {
                    return Err(refusal);
                }
                let (instance, host_in, host_out) =
                    crate::wasm::WasmInstance::spawn(&program, sandbox.memory_max).map_err(
                        |why| {
                            KuangError::new(
                                KuangErrorCode::PluginConfinementFailed,
                                format!(
                                    "`{}` could not be started as a component: {why}",
                                    manifest.package.id
                                ),
                            )
                        },
                    )?;
                tokio::spawn(read_frames(host_out, frame_limits, frame_tx));
                (
                    Runtime::Wasm(instance),
                    Box::new(host_in),
                    confinement,
                    sandbox,
                )
            } else {
                let mut command = Command::new(&program);
                command.args(&args);
                // v0.4.1 §2.3, §16.3, §18.1: a mandatory control that could not be installed
                // abandons the spawn here, before a single plugin instruction runs. The package
                // is not quarantined — it never started, so there is nothing to hold (ADR-0444).
                let (mut child, confinement) = crate::sandbox::spawn(
                    &mut command,
                    &sandbox,
                    &confinement,
                    &manifest.package.id,
                )?;
                let stdin = child.stdin.take().ok_or_else(broken_pipes)?;
                let stdout = child.stdout.take().ok_or_else(broken_pipes)?;
                tokio::spawn(read_frames(stdout, frame_limits, frame_tx));
                (
                    Runtime::Native(child),
                    Box::new(stdin),
                    confinement,
                    sandbox,
                )
            };

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
        for view in &hello.contributions.views {
            ono_kuang_protocol::validate_contributed_id(&package_id, "view", &view.id)?;
            if !view.id.starts_with(&format!("{package_id}.view.")) {
                return Err(KuangError::new(
                    KuangErrorCode::PackageInvalid,
                    format!(
                        "view id `{}` is not `<package.id>.view.<kebab-name>` (spec §31.27)",
                        view.id
                    ),
                ));
            }
            if !matches!(view.mode.as_str(), "interactive" | "static") {
                return Err(KuangError::new(
                    KuangErrorCode::PackageInvalid,
                    format!("view `{}` declares the mode `{}`", view.id, view.mode),
                ));
            }
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
            peak_memory: None,
            current_memory: None,
            cpu_time: None,
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
            models,
            context,
            host,
            views,
            open_views: HashMap::new(),
            contributed_views: hello.contributions.views.clone(),
            secrets: HashMap::new(),
            inbound: HashMap::new(),
            disclosed_remote: false,
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
            sandbox: sandbox.clone(),
        };
        tokio::spawn(actor.run());
        Ok(LoadedPlugin {
            views: hello.contributions.views.clone(),
            package_id,
            shared,
            sandbox,
            confinement,
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

// --- the instance's runtime ---------------------------------------------------------------------

/// What runs the package: a confined process of the Ono user, or a component inside the
/// runtime Ono embeds (spec §31.10, ADR-0569). The actor treats both alike: a writer for its
/// standard input, frames from its standard output, and a way to end it and learn how it ended.
enum Runtime {
    Native(Child),
    Wasm(crate::wasm::WasmInstance),
}

/// How an instance ended, in the terms the actor reports (spec §31.34).
enum Ended {
    Native(Option<std::process::ExitStatus>),
    Wasm(crate::wasm::Exit),
}

impl Runtime {
    async fn kill(&mut self) {
        match self {
            Runtime::Native(child) => {
                let _ = child.kill().await;
            }
            Runtime::Wasm(instance) => instance.kill(),
        }
    }

    async fn wait(&mut self) -> Ended {
        match self {
            Runtime::Native(child) => Ended::Native(child.wait().await.ok()),
            Runtime::Wasm(instance) => Ended::Wasm(instance.wait().await),
        }
    }
}

/// The writer the actor sends frames through: a process's pipe, or a component's stdin.
type Stdin = Box<dyn AsyncWrite + Send + Unpin>;

/// The controls the component tier installs by construction, and the ones it does not
/// (v0.4.1 §16.4, ADR-0569): the report an `inspect plugin` shows for a component.
fn component_controls(
    sandbox: &Sandbox,
) -> Vec<(
    ono_kuang_protocol::Control,
    crate::report::ControlResult,
    Option<String>,
)> {
    use crate::report::ControlResult;
    use ono_kuang_protocol::{Control, Requirement};
    Control::ALL
        .iter()
        .map(|control| {
            let detail = match control {
                Control::RlimitData => Some(format!(
                    "the runtime refuses every growth of linear memory beyond {} bytes",
                    sandbox.memory_max
                )),
                Control::FilesystemIsolation => {
                    Some("the WASI context preopens no directory".to_owned())
                }
                Control::NetworkIsolation => Some("the WASI context allows no address".to_owned()),
                Control::FdHygiene => Some("a component holds no descriptor at all".to_owned()),
                Control::ProtocolStdio => Some(
                    "the component's standard input and output are the protocol streams".to_owned(),
                ),
                Control::EnvironmentSanitization => {
                    Some("the WASI context carries no environment".to_owned())
                }
                Control::ProcessLifetime => {
                    Some("the component's task ends with the instance".to_owned())
                }
                _ => None,
            };
            let result = if ExecutionTier::Wasm.requirement(*control) == Requirement::NotProvided {
                ControlResult::NotAttempted
            } else {
                ControlResult::Applied
            };
            (*control, result, detail)
        })
        .collect()
}

// --- handshake ---------------------------------------------------------------------------------

struct Handshake<'a> {
    child: &'a mut Runtime,
    frames: mpsc::Receiver<Result<Envelope, FrameError>>,
    stdin: Stdin,
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
            Stdin,
            Hello,
            InitResult,
        ),
        KuangError,
    > {
        let result = self.drive().await;
        match result {
            Ok((hello, init)) => Ok((self.frames, self.stdin, hello, init)),
            Err(error) => {
                self.child.kill().await;
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
    stdin: &mut Stdin,
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
    mut stdout: impl AsyncRead + Unpin + Send + 'static,
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
    /// The most memory the instance has been observed to have allocated, in bytes — spec
    /// §31.33's `memory/current`, and the evidence behind a resource-limit failure (§31.34).
    /// `None` until the first sample, because an unobserved figure is not a zero (spec §35.3).
    peak_memory: Option<u64>,
    /// The instance's allocated memory at the last sample, in bytes (spec §31.33's `memory`).
    current_memory: Option<u64>,
    /// The CPU time the instance has used, in nanoseconds (spec §31.33's `cpu time`).
    cpu_time: Option<i128>,
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
    sandbox: Sandbox,
    confinement: ConfinementReport,
    contract: PluginContract,
    disabled_features: Vec<String>,
    commands: Vec<RegisteredCommand>,
    targets: Vec<RegisteredTarget>,
    views: Vec<ono_kuang_protocol::ViewContribution>,
    audit: AuditTrail,
    to_actor: mpsc::Sender<ActorMsg>,
}

impl LoadedPlugin {
    /// The package id this instance runs.
    #[must_use]
    pub fn package_id(&self) -> &str {
        &self.package_id
    }

    /// The views the package contributed (spec §31.27).
    #[must_use]
    pub fn views(&self) -> &[ono_kuang_protocol::ViewContribution] {
        &self.views
    }

    /// What the instance was started inside (spec §31.10).
    #[must_use]
    pub fn sandbox(&self) -> &Sandbox {
        &self.sandbox
    }

    /// What the instance's confinement actually is, control by control (v0.4.1 §16.5).
    ///
    /// Every `required` row reads `applied`, because a spawn for which that did not hold never
    /// produced a `LoadedPlugin` (§2.3). A best-effort row that reads `failed` is the diagnostic
    /// §16.4 requires such a failure to remain visible in.
    #[must_use]
    pub const fn confinement(&self) -> &ConfinementReport {
        &self.confinement
    }

    /// The most memory the instance has been observed to have allocated, in bytes
    /// (spec §31.33). `None` until the host has taken a sample.
    #[must_use]
    pub fn peak_memory(&self) -> Option<u64> {
        lock(&self.shared).peak_memory
    }

    /// The instance's allocated memory at the last sample, in bytes (spec §31.33).
    #[must_use]
    pub fn current_memory(&self) -> Option<u64> {
        lock(&self.shared).current_memory
    }

    /// The CPU time the instance has used, in nanoseconds (spec §31.33).
    #[must_use]
    pub fn cpu_time(&self) -> Option<i128> {
        lock(&self.shared).cpu_time
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
    /// An event the terminal produced for a view, to be forwarded to the package.
    ViewEvent {
        view: u64,
        event: ono_kuang_protocol::ViewEvent,
    },
    /// A cancelled view the package did not close in time: the host closes it (§31.27).
    ForceCloseView {
        view: u64,
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

/// A view the instance has open (ADR-0572).
struct OpenView {
    #[allow(dead_code, reason = "named for the trail and diagnostics that follow")]
    id: String,
    mounted: Option<Box<dyn crate::view::MountedView>>,
    forwarder: tokio::task::JoinHandle<()>,
}

/// A stream the host produced and the plugin pulls with `streams.next` (ADR-0567): what has
/// arrived and not been read, the live source it arrives from, whether it is over, and how.
struct Inbound {
    values: std::collections::VecDeque<Json>,
    live: Option<crate::host::LiveStream>,
    /// Where the plugin's own bytes go, when the stream is a connection.
    writer: Option<tokio::sync::mpsc::Sender<Vec<u8>>>,
    /// The connections a listener accepted and the plugin has not been handed yet.
    accepted: Option<tokio::sync::mpsc::Receiver<(String, crate::host::Connection)>>,
    complete: bool,
    error: Option<WireError>,
}

impl Inbound {
    /// Pulls from the live source: the first value with a deadline, the rest as they are there,
    /// up to `wanted`. A closed source completes the stream; a failure ends it with the error.
    async fn fill(&mut self, wanted: usize, deadline: Duration) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        while self.values.len() < wanted {
            let next = if self.values.is_empty() {
                match tokio::time::timeout(deadline, live.recv()).await {
                    Ok(next) => next,
                    Err(_) => break,
                }
            } else {
                match live.try_recv() {
                    Ok(next) => Some(next),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => None,
                }
            };
            match next {
                Some(Ok(value)) => self.values.push_back(value),
                Some(Err(error)) => {
                    self.error = Some(error);
                    self.complete = true;
                    self.live = None;
                    break;
                }
                None => {
                    self.complete = true;
                    self.live = None;
                    break;
                }
            }
        }
    }
}

/// A schema as `schemas.get` and `schemas.list` describe it: fields, types, units, nullability,
/// identity, default view, and where it came from (spec §31.64).
fn schema_record(schema: &ono_value::Schema, package_id: &str) -> Json {
    let id = schema.id().to_string();
    let origin = if id.starts_with("ono.") {
        "core"
    } else if id.starts_with(package_id) {
        "package"
    } else {
        "provider"
    };
    let fields: Vec<Json> = schema
        .fields()
        .iter()
        .map(|field| {
            json!({
                "name": field.name(),
                "type": field.ty().to_string(),
                "required": field.is_required(),
                "nullable": field.is_nullable(),
                "unit": field.unit().map(|unit| unit.to_string()),
                "doc": field.doc(),
            })
        })
        .collect();
    json!({
        "id": id,
        "name": schema.name(),
        "doc": schema.doc(),
        "identity": schema.identity().iter().map(|name| name.to_string()).collect::<Vec<_>>(),
        "identity_fallback": schema.identity_fallback().iter().map(|name| name.to_string()).collect::<Vec<_>>(),
        "fields": fields,
        "default_view": schema.default_view().iter().map(|name| name.to_string()).collect::<Vec<_>>(),
        "origin": origin,
    })
}

enum Expected {
    Schema(ono_value::SchemaId),
    Type(ono_value::FieldType),
    Any,
}

struct Actor {
    child: Runtime,
    stdin: Stdin,
    frames: mpsc::Receiver<Result<Envelope, FrameError>>,
    msgs: mpsc::Receiver<ActorMsg>,
    shared: Arc<Mutex<Shared>>,
    audit: AuditTrail,
    policy: Policy,
    clock: HostClock,
    /// What `models.*` reach (ADR-0566).
    models: Arc<dyn ono_model_broker::ModelBroker>,
    /// What `context.get` answers with (ADR-0567).
    context: Arc<dyn crate::context::ContextSource>,
    /// What the object, relation, history, process and secret domains reach (ADR-0568).
    host: Arc<dyn crate::host::HostServices>,
    /// What takes a view (ADR-0572).
    views: Arc<dyn crate::view::ViewHost>,
    /// The views this instance has open, by handle.
    open_views: HashMap<u64, OpenView>,
    /// The views the package contributed in its hello.
    contributed_views: Vec<ono_kuang_protocol::ViewContribution>,
    /// The secret handles this instance holds, by handle: the name, never the material.
    secrets: HashMap<u64, String>,
    /// The streams the host produces and the plugin pulls with `streams.next`, by handle
    /// (spec §31.15, ADR-0567).
    inbound: HashMap<u64, Inbound>,
    /// Whether the data-boundary plan of spec §31.82 has been disclosed for a remote provider.
    disclosed_remote: bool,
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
    /// What the instance was started inside, so a death can be checked against its ceilings
    /// (spec §31.34).
    sandbox: Sandbox,
}

enum LoopStep {
    Continue,
    Stop,
}

impl Actor {
    async fn run(mut self) {
        // Spec §31.33 asks `inspect plugin` for `memory/current/limit`, and spec §31.34 makes
        // "resource limit" a failure class of its own. Both need the same number, and the kernel
        // already keeps it: sampling `VmData` — the figure `RLIMIT_DATA` bounds — turns a
        // ceiling nobody could observe into a health field and into evidence for why an
        // instance died.
        let mut sample = tokio::time::interval(MEMORY_SAMPLE_INTERVAL);
        sample.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = sample.tick() => {
                    self.sample_memory();
                }
                frame = self.frames.recv() => match frame {
                    Some(Ok(envelope)) => {
                        // The actor is awake anyway, so this costs one small `/proc` read and
                        // makes the health figures follow the instance's real activity rather
                        // than only the tick (spec §31.33).
                        self.sample_memory();
                        match self.handle_envelope(envelope).await {
                            Ok(LoopStep::Continue) => {}
                            Ok(LoopStep::Stop) => break,
                            Err(violation) => {
                                self.quarantine(violation).await;
                                break;
                            }
                        }
                    }
                    Some(Err(frame_error)) => {
                        self.quarantine(protocol_violation(frame_error)).await;
                        break;
                    }
                    None => {
                        if !self.shutting_down {
                            let failure = self.death().await;
                            self.fail_instance(failure).await;
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
                        self.child.kill().await;
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

    /// Reads the instance's health from the kernel: what it has allocated, its high-water mark,
    /// and the CPU time it has used (spec §31.33).
    fn sample_memory(&mut self) {
        let (allocated, cpu) = match &self.child {
            Runtime::Native(child) => {
                let Some(pid) = child.id() else {
                    return;
                };
                (
                    crate::sandbox::allocated_bytes(pid),
                    crate::sandbox::cpu_nanoseconds(pid),
                )
            }
            // The runtime accounts a component's memory itself: every growth passes its
            // limiter, so the gauge is exact rather than sampled.
            Runtime::Wasm(instance) => (Some(instance.gauge().current()), None),
        };
        let mut shared = lock(&self.shared);
        if let Some(allocated) = allocated {
            shared.current_memory = Some(allocated);
            shared.peak_memory = Some(
                shared
                    .peak_memory
                    .map_or(allocated, |peak| peak.max(allocated)),
            );
        }
        if cpu.is_some() {
            shared.cpu_time = cpu;
        }
    }

    /// Why the instance is gone, from what the kernel says about how it ended (spec §31.34).
    ///
    /// The classification is evidence, never a guess. A signal the kernel raises for a resource
    /// limit names that limit exactly. Memory is the case with no signal of its own:
    /// `RLIMIT_DATA` makes an over-large allocation *fail*, and what the package does then is the
    /// package's business — a Rust artifact aborts, a C one may carry on. So the host reports
    /// `runtime.memory_limit` when it observed the instance at its ceiling, and otherwise names
    /// the signal, the ceiling that was in force and the high-water mark it did observe, so the
    /// operator sees the relationship between them instead of being told a story about it.
    async fn death(&mut self) -> KuangError {
        let ended = self.child.wait().await;
        let peak = lock(&self.shared).peak_memory;
        let status = match ended {
            Ended::Native(status) => status,
            Ended::Wasm(exit) => return self.component_death(exit, peak),
        };
        let signal = status.and_then(|status| {
            #[cfg(unix)]
            {
                std::os::unix::process::ExitStatusExt::signal(&status)
            }
            #[cfg(not(unix))]
            {
                let _ = status;
                None
            }
        });
        let at_ceiling = peak.is_some_and(|peak| at_memory_ceiling(peak, self.sandbox.memory_max));
        match signal {
            // The kernel's own resource signals, which name their limit exactly.
            Some(libc::SIGXCPU) => KuangError::new(
                KuangErrorCode::RuntimeTimeout,
                "the plugin instance exhausted its CPU limit and was stopped",
            )
            .with_metadata("resource_class", json!("cpu")),
            Some(libc::SIGXFSZ) => KuangError::new(
                KuangErrorCode::RuntimeTrap,
                format!(
                    "the plugin instance tried to write beyond its file-size limit of {} bytes",
                    self.sandbox.file_size
                ),
            )
            .with_metadata("resource_class", json!("file_size")),
            _ if at_ceiling => KuangError::new(
                KuangErrorCode::RuntimeMemoryLimit,
                format!(
                    "the plugin instance reached its memory ceiling of {} bytes and ended",
                    self.sandbox.memory_max
                ),
            )
            // v0.4.1 §18.3: the error identifies the enforced resource class rather than
            // reporting "plugin exited", so a caller can tell a limit from a defect without
            // reading the sentence.
            .with_metadata("resource_class", json!("memory"))
            .with_help(
                "`runtime.memory_max` in the package's manifest declares the ceiling; the host \
                 caps it and never raises it",
            ),
            Some(number) => KuangError::new(
                KuangErrorCode::RuntimeTrap,
                format!(
                    "the plugin instance was killed by signal {number}; {}",
                    self.memory_account(peak)
                ),
            ),
            None => match status.and_then(|status| status.code()) {
                Some(code) => KuangError::new(
                    KuangErrorCode::RuntimeTrap,
                    format!(
                        "the plugin instance exited with status {code}; {}",
                        self.memory_account(peak)
                    ),
                ),
                None => KuangError::new(
                    KuangErrorCode::RuntimeTrap,
                    "the plugin instance exited unexpectedly",
                ),
            },
        }
    }

    /// What the host saw of the instance's memory, against the ceiling it was under.
    ///
    /// Part of every abnormal-death message, because "killed by signal 6" on its own leaves the
    /// operator to guess whether the package crashed or ran out of the room it declared.
    fn memory_account(&self, peak: Option<u64>) -> String {
        match peak {
            Some(peak) => format!(
                "it had allocated {peak} bytes of its {} byte ceiling when last observed",
                self.sandbox.memory_max
            ),
            None => format!(
                "its memory ceiling was {} bytes and the host observed no sample",
                self.sandbox.memory_max
            ),
        }
    }

    /// How a component's end reads (spec §31.34): the ceiling when it reached it, the trap
    /// otherwise, and its own status when it simply returned.
    fn component_death(&self, exit: crate::wasm::Exit, peak: Option<u64>) -> KuangError {
        let at_ceiling = peak.is_some_and(|peak| at_memory_ceiling(peak, self.sandbox.memory_max));
        match exit {
            _ if at_ceiling => KuangError::new(
                KuangErrorCode::RuntimeMemoryLimit,
                format!(
                    "the component reached its memory ceiling of {} bytes and ended",
                    self.sandbox.memory_max
                ),
            )
            .with_metadata("resource_class", json!("memory"))
            .with_help(
                "`runtime.memory_max` in the package's manifest declares the ceiling; the host \
                 caps it and never raises it",
            ),
            crate::wasm::Exit::Trapped(trap) => KuangError::new(
                KuangErrorCode::RuntimeTrap,
                format!(
                    "the component trapped: {trap}; {}",
                    self.memory_account(peak)
                ),
            ),
            crate::wasm::Exit::Returned { success } => KuangError::new(
                KuangErrorCode::RuntimeTrap,
                format!(
                    "the component returned {} while the host still needed it; {}",
                    if success { "success" } else { "failure" },
                    self.memory_account(peak)
                ),
            ),
            crate::wasm::Exit::Killed => KuangError::new(
                KuangErrorCode::RuntimeTrap,
                "the component was stopped by the host",
            ),
        }
    }

    /// Ends the instance for a protocol violation: kill, quarantine, close every stream with
    /// the violation, resolve every pending invocation as failed (spec §31.34, ADR-0041).
    async fn quarantine(&mut self, violation: KuangError) {
        self.child.kill().await;
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
        self.child.kill().await;
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
            ActorMsg::ViewEvent { view, event } => {
                if self.open_views.contains_key(&view) {
                    if event.kind == "cancel" {
                        // The package has the call deadline to close it; then the host does.
                        let deadline = Duration::from_millis(self.contract.limits.call_deadline_ms);
                        let sender = self.msg_sender.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(deadline).await;
                            let _ = sender.send(ActorMsg::ForceCloseView { view }).await;
                        });
                    }
                    let params =
                        serde_json::to_value(ono_kuang_protocol::ViewEventParams { view, event })
                            .unwrap_or(Json::Null);
                    self.notify_plugin(method::VIEW_EVENT, params).await;
                }
                LoopStep::Continue
            }
            ActorMsg::ForceCloseView { view } => {
                if self.open_views.contains_key(&view) {
                    self.close_view(view, true).await;
                }
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
                self.child.kill().await;
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
                        // A view outlives no invocation (spec §31.28): the terminal comes back
                        // however the command ended.
                        self.close_all_views(false).await;
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
            method::STREAMS_NEXT => self.host_streams_next(seq, params).await,
            method::STREAMS_CANCEL => self.host_streams_cancel(seq, params).await,
            method::CONTEXT_GET => self.host_context_get(seq).await,
            method::SCHEMAS_GET => self.host_schemas_get(seq, params).await,
            method::SCHEMAS_LIST => self.host_schemas_list(seq, params).await,
            method::OBJECTS_GET => self.host_objects_get(seq, params).await,
            method::OBJECTS_QUERY => self.host_objects_stream(seq, params, "objects.query").await,
            method::OBJECTS_RESOLVE => self.host_objects_resolve(seq, params).await,
            method::OBJECTS_SNAPSHOT => {
                self.host_objects_stream(seq, params, "objects.snapshot")
                    .await
            }
            method::OBJECTS_SUBSCRIBE => {
                self.host_objects_stream(seq, params, "objects.subscribe")
                    .await
            }
            method::OBJECTS_WATCH => self.host_objects_stream(seq, params, "objects.watch").await,
            method::RELATIONS_QUERY => self.host_relations_query(seq, params).await,
            method::RELATIONS_CONTRIBUTE => self.host_relations_contribute(seq, params).await,
            method::HISTORY_QUERY => self.host_history_query(seq, params).await,
            method::HISTORY_APPEND => self.host_history_append(seq, params).await,
            method::PROCESS_SIGNAL => self.host_process_signal(seq, params).await,
            method::VIEWS_OPEN => self.host_views_open(seq, params).await,
            method::VIEWS_SUBMIT => self.host_views_submit(seq, params).await,
            method::VIEWS_CLOSE => self.host_views_close(seq, params).await,
            method::PROCESS_EXEC => self.host_process_exec(seq, params).await,
            method::NETWORK_CONNECT => self.host_network_connect(seq, params).await,
            method::NETWORK_CLOSE => self.host_network_close(seq, params).await,
            method::NETWORK_LISTEN => self.host_network_listen(seq, params).await,
            method::NETWORK_REQUEST => {
                // Not the host's to serve (ADR-0571): a request is a package's own protocol over
                // the brokered connection, so the trust decision stays where §31.21 puts it — in
                // the operator's scope for `network.connect` — and the shell carries no client
                // for a protocol it does not speak.
                self.reply_err(
                    seq,
                    WireError::from_core(
                        ono_core::ErrorCode::ProviderUnavailable,
                        "this host serves no `network.request`: a request is the package's own \
                         protocol over `network.connect` (ADR-0571)",
                    ),
                )
                .await;
                Ok(())
            }
            method::SECRETS_REQUEST => self.host_secrets_request(seq, params).await,
            method::SECRETS_RELEASE => self.host_secrets_release(seq, params).await,
            method::MODELS_LIST => self.host_models_list(seq).await,
            method::MODELS_INFER => self.host_models_infer(seq, params).await,
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
        // A connection is written the way a stream is emitted into: the values are bytes, and
        // they go to the socket the host holds (spec §31.21).
        if let Some(writer) = self
            .inbound
            .get(&emit.handle)
            .and_then(|stream| stream.writer.clone())
        {
            for value in &emit.values {
                let bytes = match ono_value::from_json(value, &self.schemas) {
                    Ok(Value::Bytes(bytes)) => bytes.to_vec(),
                    Ok(Value::String(text)) => text.as_bytes().to_vec(),
                    _ => {
                        return Err(protocol_violation(
                            "a connection carries bytes, and the emission was neither bytes nor text",
                        ));
                    }
                };
                if writer.send(bytes).await.is_err() {
                    self.reply_err(
                        seq,
                        WireError::from_core(
                            ono_core::ErrorCode::ProviderUnavailable,
                            "the connection is closed",
                        ),
                    )
                    .await;
                    return Ok(());
                }
            }
            self.reply_ok(
                seq,
                serde_json::to_value(EmitResult { credit: u32::MAX }).unwrap_or(Json::Null),
            )
            .await;
            return Ok(());
        }
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

    /// Opens a stream the plugin pulls with `streams.next`, holding `values` already produced.
    fn open_inbound(&mut self, values: Vec<Json>) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.inbound.insert(
            handle,
            Inbound {
                values: values.into(),
                live: None,
                writer: None,
                accepted: None,
                complete: true,
                error: None,
            },
        );
        handle
    }

    /// Opens a stream over a live source the host service produced.
    fn open_live(&mut self, live: crate::host::LiveStream) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.inbound.insert(
            handle,
            Inbound {
                values: std::collections::VecDeque::new(),
                live: Some(live),
                writer: None,
                accepted: None,
                complete: false,
                error: None,
            },
        );
        handle
    }

    /// Opens a brokered connection as a stream the plugin reads and writes.
    fn open_connection(&mut self, connection: crate::host::Connection) -> u64 {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.inbound.insert(
            handle,
            Inbound {
                values: std::collections::VecDeque::new(),
                live: Some(connection.incoming),
                writer: Some(connection.outgoing),
                accepted: None,
                complete: false,
                error: None,
            },
        );
        handle
    }

    /// `streams.next`: at most `max` values of a host stream — the credit of spec §31.15,
    /// pulled rather than pushed, so the plugin decides how much it is ready for.
    async fn host_streams_next(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let next: NextParams = Self::parse_params(params)?;
        let deadline = next
            .deadline
            .as_ref()
            .and_then(duration_of)
            .unwrap_or(Duration::from_millis(self.contract.limits.call_deadline_ms));
        let Some(mut stream) = self.inbound.remove(&next.handle) else {
            return Err(protocol_violation(format!(
                "`streams.next` on handle {}, which the host never opened",
                next.handle
            )));
        };
        let max = usize::try_from(next.max).unwrap_or(usize::MAX);
        // A listener's values are connections, and each becomes a handle of its own in the
        // table this call is reading from — so the listener is taken out, filled, and put back.
        if let Some(accepted) = stream.accepted.as_mut() {
            while stream.values.len() < max {
                let next_peer = if stream.values.is_empty() {
                    match tokio::time::timeout(deadline, accepted.recv()).await {
                        Ok(peer) => peer,
                        Err(_) => break,
                    }
                } else {
                    match accepted.try_recv() {
                        Ok(peer) => Some(peer),
                        Err(_) => break,
                    }
                };
                match next_peer {
                    Some((peer, connection)) => {
                        let handle = self.open_connection(connection);
                        stream
                            .values
                            .push_back(json!({"connection": handle, "peer": peer}));
                    }
                    None => {
                        stream.complete = true;
                        stream.accepted = None;
                        break;
                    }
                }
            }
        } else {
            stream.fill(max, deadline).await;
        }
        self.inbound.insert(next.handle, stream);
        let Some(stream) = self.inbound.get_mut(&next.handle) else {
            return Ok(());
        };
        let mut values = Vec::with_capacity(max.min(stream.values.len()));
        while values.len() < max {
            let Some(value) = stream.values.pop_front() else {
                break;
            };
            values.push(value);
        }
        let complete = stream.complete && stream.values.is_empty();
        let error = if complete { stream.error.take() } else { None };
        if complete {
            self.inbound.remove(&next.handle);
        }
        let answer = NextResult {
            values,
            complete,
            error,
        };
        self.reply_ok(seq, serde_json::to_value(answer).unwrap_or(Json::Null))
            .await;
        Ok(())
    }

    /// `streams.cancel`: a stream in either direction is over, and the host stops feeding or
    /// accepting it.
    async fn host_streams_cancel(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let cancel: StreamHandleParams = Self::parse_params(params)?;
        if self.inbound.remove(&cancel.handle).is_none() {
            match self.streams.get_mut(&cancel.handle) {
                Some(stream) => stream.cancelled = true,
                None => {
                    return Err(protocol_violation(format!(
                        "`streams.cancel` on handle {}, which is not open",
                        cancel.handle
                    )));
                }
            }
        }
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    /// `context.get`: the context stack the shell published, and nothing beyond it.
    async fn host_context_get(&mut self, seq: u64) -> Result<(), KuangError> {
        match self.broker_check(
            ono_kuang_protocol::Capability::ContextRead,
            "context.get",
            &[],
            None,
        ) {
            Ok(_) => {
                let context = self.context.context();
                self.reply_ok(seq, context).await;
            }
            Err(error) => self.reply_err(seq, error.into()).await,
        }
        Ok(())
    }

    /// `schemas.get`: one registered schema — core, this package's, or a provider's — as a
    /// record of its fields, identity and default view (spec §31.64).
    async fn host_schemas_get(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let get: SchemaGetParams = Self::parse_params(params)?;
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::SchemaRead,
            "schemas.get",
            &[],
            Some(Json::String(get.id.clone())),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let found = get
            .id
            .parse::<ono_value::SchemaId>()
            .ok()
            .and_then(|id| self.schemas.get(&id));
        match found {
            Some(schema) => {
                let record = schema_record(&schema, &self.package_id);
                self.reply_ok(seq, record).await;
            }
            None => {
                self.reply_err(
                    seq,
                    WireError::from_core(
                        ono_core::ErrorCode::ResolveTargetNotFound,
                        format!("no schema `{}` is registered", get.id),
                    ),
                )
                .await;
            }
        }
        Ok(())
    }

    /// `schemas.list`: every registered schema under a prefix, as a stream the plugin pulls.
    async fn host_schemas_list(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let list: SchemaListParams = Self::parse_params(params)?;
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::SchemaRead,
            "schemas.list",
            &[],
            list.prefix.clone().map(Json::String),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let mut ids: Vec<String> = self
            .schemas
            .ids()
            .map(ToString::to_string)
            .filter(|id| {
                list.prefix
                    .as_ref()
                    .is_none_or(|prefix| id.starts_with(prefix))
            })
            .collect();
        ids.sort();
        let package_id = self.package_id.clone();
        let records: Vec<Json> = ids
            .iter()
            .filter_map(|id| id.parse::<ono_value::SchemaId>().ok())
            .filter_map(|id| self.schemas.get(&id))
            .map(|schema| schema_record(&schema, &package_id))
            .collect();
        let handle = self.open_inbound(records);
        self.reply_ok(seq, json!({"handle": handle})).await;
        Ok(())
    }

    /// Records a host service's failure after the check passed: the broker said yes and the
    /// operation did not happen, which the trail has to say too (spec §31.37).
    fn audit_failed(
        &mut self,
        capability: ono_kuang_protocol::Capability,
        action: &str,
        target: Option<Json>,
        error: &WireError,
    ) {
        let label = self.invocation_label();
        self.audit.record(
            &self.package_id,
            &label,
            capability.id(),
            None,
            Enforcement::Broker,
            action,
            target,
            self.now(),
            AuditResult::Failed,
            Some(error.clone()),
        );
    }

    /// Answers a host service's result: the value on success, the failure audited and answered.
    async fn reply_service(
        &mut self,
        seq: u64,
        capability: ono_kuang_protocol::Capability,
        action: &str,
        target: Option<Json>,
        outcome: Result<Json, crate::host::HostError>,
    ) {
        match outcome {
            Ok(value) => self.reply_ok(seq, value).await,
            Err(error) => {
                let error: WireError = error.into();
                self.audit_failed(capability, action, target, &error);
                self.reply_err(seq, error).await;
            }
        }
    }

    /// `objects.get`: one object by identity, through the host's providers.
    async fn host_objects_get(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let id = params.get("id").cloned().unwrap_or(Json::Null);
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::ObjectRead,
            "objects.get",
            &[],
            Some(id.clone()),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host.object_get(id.clone()).await;
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::ObjectRead,
            "objects.get",
            Some(id),
            outcome,
        )
        .await;
        Ok(())
    }

    /// `objects.query`, `objects.snapshot`, `objects.subscribe`, `objects.watch`: a stream the
    /// plugin pulls, over what the host's providers produce.
    async fn host_objects_stream(
        &mut self,
        seq: u64,
        params: Json,
        action: &'static str,
    ) -> Result<(), KuangError> {
        let query = params.get("query").cloned().unwrap_or(Json::Null);
        let target = query
            .get("target")
            .and_then(Json::as_str)
            .map(str::to_owned);
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::ObjectRead,
            action,
            &[],
            target.map(Json::String),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let audited_target = query
            .get("target")
            .and_then(Json::as_str)
            .map(|target| Json::String(target.to_owned()));
        let opened = match action {
            "objects.snapshot" => host.object_snapshot(query).await,
            "objects.subscribe" => {
                let overflow = params
                    .get("overflow")
                    .and_then(Json::as_str)
                    .map(str::to_owned);
                host.object_subscribe(query, overflow).await
            }
            "objects.watch" => {
                let policy = params.get("policy").cloned().unwrap_or(Json::Null);
                host.object_watch(query, policy).await
            }
            _ => host.object_query(query).await,
        };
        let outcome = opened.map(|live| {
            let handle = self.open_live(live);
            json!({"handle": handle})
        });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::ObjectRead,
            action,
            audited_target,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `objects.resolve`: the references a selector matches. A selector needs a target to be
    /// resolved against; one that names an identity carries its schema and needs none.
    async fn host_objects_resolve(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let selector = params.get("selector").cloned().unwrap_or(Json::Null);
        let target = params
            .get("target")
            .and_then(Json::as_str)
            .map(str::to_owned)
            .or_else(|| {
                selector
                    .get("identity")
                    .and_then(|identity| identity.get("schema"))
                    .and_then(Json::as_str)
                    .and_then(|schema| schema.strip_prefix("ono."))
                    .and_then(|rest| rest.split('/').next())
                    .map(str::to_owned)
            });
        let Some(target) = target else {
            self.reply_err(
                seq,
                WireError::from_core(
                    ono_core::ErrorCode::TypeMismatch,
                    "`objects.resolve` needs a `target`, or a selector that names an identity",
                ),
            )
            .await;
            return Ok(());
        };
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::ObjectRead,
            "objects.resolve",
            &[],
            Some(Json::String(target.clone())),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host
            .object_resolve(target.clone(), selector)
            .await
            .map(Json::Array);
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::ObjectRead,
            "objects.resolve",
            Some(Json::String(target)),
            outcome,
        )
        .await;
        Ok(())
    }

    /// `relations.query`: the edges around an object, as a stream of `ono.graph-edge/1`.
    async fn host_relations_query(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let from = params.get("from").filter(|v| !v.is_null()).cloned();
        let to = params.get("to").filter(|v| !v.is_null()).cloned();
        let relations = params
            .get("relations")
            .and_then(Json::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect()
            });
        let depth = params.get("depth").and_then(Json::as_u64);
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::RelationRead,
            "relations.query",
            &[],
            from.clone().or_else(|| to.clone()),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let target = from.clone().or_else(|| to.clone());
        let outcome = host
            .relations_query(from, to, relations, depth)
            .await
            .map(|live| {
                let handle = self.open_live(live);
                json!({"handle": handle})
            });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::RelationRead,
            "relations.query",
            target,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `relations.contribute`: edges the package asserts; the host sets `provider` to it.
    async fn host_relations_contribute(
        &mut self,
        seq: u64,
        params: Json,
    ) -> Result<(), KuangError> {
        let edges: Vec<Json> = params
            .get("edges")
            .and_then(Json::as_array)
            .cloned()
            .unwrap_or_default();
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::RelationWrite,
            "relations.contribute",
            &[],
            Some(Json::from(edges.len())),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let count = edges.len();
        let outcome = host
            .relations_contribute(&self.package_id, edges)
            .await
            .map(Json::from);
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::RelationWrite,
            "relations.contribute",
            Some(Json::from(count)),
            outcome,
        )
        .await;
        Ok(())
    }

    /// `history.query`: bounded history, redacted by the host before it is assembled.
    async fn host_history_query(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let window = params
            .get("window")
            .and_then(Json::as_str)
            .map(str::to_owned);
        let filter = params.get("filter").filter(|v| !v.is_null()).cloned();
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::HistoryRead,
            "history.query",
            &[],
            window.clone().map(Json::String),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let target = window.clone().map(Json::String);
        let outcome = host.history_query(window, filter).await.map(|live| {
            let handle = self.open_live(live);
            json!({"handle": handle})
        });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::HistoryRead,
            "history.query",
            target,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `history.append`: an entry the host attributes to the package.
    async fn host_history_append(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let entry = params.get("entry").cloned().unwrap_or(Json::Null);
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::HistoryWrite,
            "history.append",
            &[],
            None,
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host
            .history_append(&self.package_id, entry)
            .await
            .map(|()| Json::Null);
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::HistoryWrite,
            "history.append",
            None,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `process.signal`: a signal within the granted `signals` scope, to an object the host
    /// resolves again before it acts.
    async fn host_process_signal(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let object = params.get("object").cloned().unwrap_or(Json::Null);
        let signal = params
            .get("signal")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::ProcessSignal,
            "process.signal",
            &[ScopeUse::Name {
                key: "signals",
                value: signal.clone(),
            }],
            Some(object.clone()),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host.process_signal(object.clone(), signal).await;
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::ProcessSignal,
            "process.signal",
            Some(object),
            outcome,
        )
        .await;
        Ok(())
    }

    /// A request to the package the host does not wait for: `view.mount`, `view.event`,
    /// `view.unmount`.
    async fn notify_plugin(&mut self, method_name: &str, params: Json) {
        let seq = self.next_seq(Pending::FireAndForget);
        let envelope = Envelope::Request {
            seq,
            method: method_name.to_owned(),
            params,
        };
        let _ = self.send(&envelope).await;
    }

    /// `views.open`: a contributed view, taken by the host when a terminal is there and
    /// answered `mounted: false` when output is redirected (spec §31.27, §31.28).
    async fn host_views_open(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let open: ono_kuang_protocol::ViewOpenParams = Self::parse_params(params)?;
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::UiView,
            "views.open",
            &[],
            Some(Json::String(open.view.clone())),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let Some(contribution) = self
            .contributed_views
            .iter()
            .find(|view| view.id == open.view)
            .cloned()
        else {
            self.reply_err(
                seq,
                KuangError::new(
                    KuangErrorCode::ViewProtocolError,
                    format!("the package contributes no view `{}`", open.view),
                )
                .into(),
            )
            .await;
            return Ok(());
        };
        let (events_tx, mut events_rx) = mpsc::channel(32);
        let mounted = match self.views.open(&self.package_id, &contribution, events_tx) {
            Ok(mounted) => mounted,
            Err(why) => {
                self.reply_err(
                    seq,
                    KuangError::new(
                        KuangErrorCode::ViewProtocolError,
                        format!("the terminal refused the view: {why}"),
                    )
                    .into(),
                )
                .await;
                return Ok(());
            }
        };
        let handle = self.next_handle;
        self.next_handle += 1;
        let size = mounted.as_ref().map(|view| view.size());
        let forwarder = {
            let sender = self.msg_sender.clone();
            tokio::spawn(async move {
                while let Some(event) = events_rx.recv().await {
                    if sender
                        .send(ActorMsg::ViewEvent {
                            view: handle,
                            event,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            })
        };
        let is_mounted = mounted.is_some();
        self.open_views.insert(
            handle,
            OpenView {
                id: contribution.id.clone(),
                mounted,
                forwarder,
            },
        );
        self.reply_ok(
            seq,
            serde_json::to_value(ono_kuang_protocol::ViewOpenResult {
                handle,
                mounted: is_mounted,
                size,
            })
            .unwrap_or(Json::Null),
        )
        .await;
        if let Some(size) = size {
            let params = serde_json::to_value(ono_kuang_protocol::ViewMountParams {
                view: handle,
                size,
                input: open.input,
            })
            .unwrap_or(Json::Null);
            self.notify_plugin(method::VIEW_MOUNT, params).await;
        }
        Ok(())
    }

    /// `views.submit`: a tree, validated and drawn; an invalid one tears the view down.
    async fn host_views_submit(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let submit: ono_kuang_protocol::ViewSubmitParams = Self::parse_params(params)?;
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::UiView,
            "views.submit",
            &[],
            Some(Json::from(submit.view)),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        if !self.open_views.contains_key(&submit.view) {
            self.reply_err(
                seq,
                KuangError::new(
                    KuangErrorCode::ViewProtocolError,
                    format!("view {} is not open", submit.view),
                )
                .into(),
            )
            .await;
            return Ok(());
        }
        let drawn = crate::view::validate_tree(&submit.tree, 0).and_then(|()| {
            self.open_views
                .get(&submit.view)
                .and_then(|view| view.mounted.as_ref())
                .map_or(Ok(()), |mounted| mounted.submit(&submit.tree))
        });
        match drawn {
            Ok(()) => self.reply_ok(seq, Json::Null).await,
            Err(why) => {
                // Spec §31.27: an invalid layout is the package's defect, and the terminal is
                // restored whatever the package does next.
                self.close_view(submit.view, true).await;
                self.reply_err(
                    seq,
                    KuangError::new(
                        KuangErrorCode::ViewProtocolError,
                        format!("the view tree was refused: {why}"),
                    )
                    .into(),
                )
                .await;
            }
        }
        Ok(())
    }

    /// `views.close`: idempotent; the host closes a view its invocation leaves open anyway.
    async fn host_views_close(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let close: ono_kuang_protocol::ViewHandleParams = Self::parse_params(params)?;
        if self.open_views.contains_key(&close.view) {
            self.close_view(close.view, false).await;
        }
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    /// Tears a view down: the terminal is restored, the package is told when the host decided.
    async fn close_view(&mut self, handle: u64, tell_plugin: bool) {
        let Some(view) = self.open_views.remove(&handle) else {
            return;
        };
        if let Some(mounted) = view.mounted {
            mounted.close();
        }
        view.forwarder.abort();
        if tell_plugin {
            let params =
                serde_json::to_value(ono_kuang_protocol::ViewHandleParams { view: handle })
                    .unwrap_or(Json::Null);
            self.notify_plugin(method::VIEW_UNMOUNT, params).await;
        }
    }

    /// Every view the instance has open is closed: an invocation ended, or the instance did.
    async fn close_all_views(&mut self, tell_plugin: bool) {
        let handles: Vec<u64> = self.open_views.keys().copied().collect();
        for handle in handles {
            self.close_view(handle, tell_plugin).await;
        }
    }

    /// `process.exec`: a program within the granted `programs` scope, run under the host's own
    /// confinement; its output and exit status come back as a stream (spec §31.12).
    async fn host_process_exec(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let program = params
            .get("program")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        let arguments: Vec<String> = params
            .get("arguments")
            .and_then(Json::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let environment: Vec<(String, String)> = params
            .get("environment")
            .and_then(Json::as_object)
            .map(|object| {
                object
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.clone(),
                            value
                                .as_str()
                                .map_or_else(|| value.to_string(), str::to_owned),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        // Checked against the value the operation will use: the resolved program, with no
        // re-resolution between check and use (ADR-0015 T11).
        let resolved = std::fs::canonicalize(&program)
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| program.clone());
        let executable = std::path::Path::new(&resolved)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let target = Some(Json::String(resolved.clone()));
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::ProcessExec,
            "process.exec",
            &[
                ScopeUse::Path {
                    key: "programs",
                    value: resolved.clone(),
                },
                ScopeUse::Name {
                    key: "executables",
                    value: executable,
                },
            ],
            target.clone(),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host
            .process_exec(&self.package_id, resolved, arguments, environment)
            .await
            .map(|live| {
                let handle = self.open_live(live);
                json!({"handle": handle})
            });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::ProcessExec,
            "process.exec",
            target,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `network.connect`: a brokered connection within the granted `hosts` and `ports`
    /// scopes. The package reads it with `streams.next` and writes it with `streams.emit`;
    /// it never receives a descriptor (spec §31.21).
    async fn host_network_connect(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let host_name = params
            .get("host")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        let port = params
            .get("port")
            .and_then(Json::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or_default();
        let protocol = params
            .get("protocol")
            .and_then(Json::as_str)
            .unwrap_or("tcp")
            .to_owned();
        let target = Some(json!({"host": host_name, "port": port, "protocol": protocol}));
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::NetworkConnect,
            "network.connect",
            &[
                ScopeUse::Name {
                    key: "hosts",
                    value: host_name.clone(),
                },
                ScopeUse::Port {
                    key: "ports",
                    value: port,
                },
            ],
            target.clone(),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host
            .network_connect(host_name, port, protocol)
            .await
            .map(|connection| {
                let handle = self.open_connection(connection);
                json!({"handle": handle})
            });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::NetworkConnect,
            "network.connect",
            target,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `network.listen`: a listener within the granted `ports` scope. Accepted connections
    /// arrive on the listener's stream as `{connection: handle, peer}` values, each handle a
    /// connection the package reads and writes like one it opened (spec §31.21).
    async fn host_network_listen(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let port = params
            .get("port")
            .and_then(Json::as_u64)
            .and_then(|port| u16::try_from(port).ok())
            .unwrap_or_default();
        let protocol = params
            .get("protocol")
            .and_then(Json::as_str)
            .unwrap_or("tcp")
            .to_owned();
        let target = Some(json!({"port": port, "protocol": protocol}));
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::NetworkListen,
            "network.listen",
            &[ScopeUse::Port {
                key: "ports",
                value: port,
            }],
            target.clone(),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host.network_listen(port, protocol).await.map(|accepted| {
            // Each accepted connection becomes a handle of its own when the plugin pulls the
            // listener's stream; until then it waits here.
            let handle = self.next_handle;
            self.next_handle += 1;
            self.inbound.insert(
                handle,
                Inbound {
                    values: std::collections::VecDeque::new(),
                    live: None,
                    writer: None,
                    accepted: Some(accepted),
                    complete: false,
                    error: None,
                },
            );
            json!({"handle": handle})
        });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::NetworkListen,
            "network.listen",
            target,
            outcome,
        )
        .await;
        Ok(())
    }

    /// `network.close`: the connection is dropped, and with it the socket the host held.
    async fn host_network_close(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let handle = params
            .get("connection")
            .and_then(Json::as_u64)
            .unwrap_or_default();
        if self.inbound.remove(&handle).is_none() {
            return Err(protocol_violation(format!(
                "`network.close` on handle {handle}, which the host never opened"
            )));
        }
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    /// `secrets.request`: an opaque handle for a named secret; the material never crosses.
    async fn host_secrets_request(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let name = params
            .get("name")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        let purpose = params
            .get("purpose")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::SecretUse,
            "secrets.request",
            &[ScopeUse::Name {
                key: "secrets",
                value: name.clone(),
            }],
            Some(Json::String(name.clone())),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let host = Arc::clone(&self.host);
        let outcome = host
            .secret_request(&self.package_id, &name, &purpose)
            .await
            .map(|()| {
                let handle = self.next_handle;
                self.next_handle += 1;
                self.secrets.insert(handle, name.clone());
                json!({"handle": handle})
            });
        self.reply_service(
            seq,
            ono_kuang_protocol::Capability::SecretUse,
            "secrets.request",
            Some(Json::String(name)),
            outcome,
        )
        .await;
        Ok(())
    }

    /// `secrets.release`: the handle is invalidated; releasing early is hygiene, not a duty.
    async fn host_secrets_release(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let handle = params
            .get("secret")
            .and_then(Json::as_u64)
            .unwrap_or_default();
        if self.secrets.remove(&handle).is_none() {
            return Err(protocol_violation(format!(
                "`secrets.release` on handle {handle}, which the host never issued"
            )));
        }
        self.reply_ok(seq, Json::Null).await;
        Ok(())
    }

    /// `models.list`: the providers this package may use — the catalogue, filtered by the
    /// grant's `providers` scope (spec §31.43, ADR-0566).
    async fn host_models_list(&mut self, seq: u64) -> Result<(), KuangError> {
        match self.broker_check(
            ono_kuang_protocol::Capability::ModelInfer,
            "models.list",
            &[],
            None,
        ) {
            Ok(grant) => {
                let path = std::env::var_os("PATH");
                let listed: Vec<Json> = self
                    .models
                    .providers()
                    .iter()
                    .filter(|provider| scope_names(&grant, "providers", &provider.id))
                    .map(|provider| provider.to_json(path.as_ref()))
                    .collect();
                self.reply_ok(seq, Json::Array(listed)).await;
            }
            Err(error) => self.reply_err(seq, error.into()).await,
        }
        Ok(())
    }

    /// `models.infer`: operator-approved inference (spec §31.43, §31.44, §31.82; ADR-0566).
    ///
    /// The order is the boundary: the provider is chosen, the grant is checked against that
    /// provider's id, the data policy is applied to every segment, the plan is disclosed before
    /// the first remote call, and only then does anything leave. The broker that talks to the
    /// model receives an already-checked provider and an already-classified request; it has no
    /// way to reach a grant or a decision.
    async fn host_models_infer(&mut self, seq: u64, params: Json) -> Result<(), KuangError> {
        let params: ModelsInferParams = Self::parse_params(params)?;
        let request = params.request;
        let path = std::env::var_os("PATH");
        let providers = self.models.providers();
        let within_scope = match self
            .policy
            .evaluate(ono_kuang_protocol::Capability::ModelInfer, &[])
        {
            Evaluation::Allowed(grant) => Some(grant),
            _ => None,
        };
        let chosen = match request.provider.as_deref() {
            Some(id) => providers.iter().find(|provider| provider.id == id).cloned(),
            None => providers
                .iter()
                .filter(|provider| {
                    within_scope
                        .as_ref()
                        .is_none_or(|grant| scope_names(grant, "providers", &provider.id))
                })
                .find(|provider| provider.unavailable_reason(path.as_ref()).is_none())
                .cloned(),
        };
        let label = self.invocation_label();
        let Some(provider) = chosen else {
            let error = KuangError::new(
                KuangErrorCode::ModelProviderUnavailable,
                match request.provider.as_deref() {
                    Some(id) => format!("no configured model provider is called `{id}`"),
                    None => "no configured model provider is available within this package's \
                             grant"
                        .to_owned(),
                },
            )
            .with_help("`get model` lists what the operator configured; `<config>/kuang/models.yaml` is where a provider is added");
            self.audit.record(
                &self.package_id,
                &label,
                "model.infer",
                None,
                Enforcement::Broker,
                "models.infer",
                request.provider.clone().map(Json::String),
                self.now(),
                AuditResult::Failed,
                Some(error.clone().into()),
            );
            self.reply_err(seq, error.into()).await;
            return Ok(());
        };
        let target = Some(Json::String(provider.id.clone()));
        if let Err(error) = self.broker_check(
            ono_kuang_protocol::Capability::ModelInfer,
            "models.infer",
            &[ScopeUse::Name {
                key: "providers",
                value: provider.id.clone(),
            }],
            target.clone(),
        ) {
            self.reply_err(seq, error.into()).await;
            return Ok(());
        }
        let (prepared, plan) = match ono_model_broker::classify(&provider, &request) {
            Ok(prepared) => prepared,
            Err(denied) => {
                let error = KuangError::new(
                    KuangErrorCode::ModelPolicyDenied,
                    format!(
                        "the request carries {} `{}` may not receive: {}",
                        if denied.classes.len() == 1 { "a data class" } else { "data classes" },
                        provider.id,
                        denied.classes.join(", ")
                    ),
                )
                .with_metadata(
                    "denied_classes",
                    Json::Array(denied.classes.iter().cloned().map(Json::String).collect()),
                )
                .with_help("nothing was sent. The request is refused whole rather than trimmed, so the boundary stays visible (spec §31.44)");
                self.audit.record(
                    &self.package_id,
                    &label,
                    "model.infer",
                    None,
                    Enforcement::Broker,
                    "models.infer",
                    target,
                    self.now(),
                    AuditResult::Failed,
                    Some(error.clone().into()),
                );
                self.reply_err(seq, error.into()).await;
                return Ok(());
            }
        };
        let plan_json = serde_json::to_value(&plan).unwrap_or(Json::Null);
        // Spec §31.82: before the first remote inference, the data-boundary plan is shown. It
        // is an audit record, so `get audit --plugin <id>` is where it stays inspectable.
        if provider.kind == ono_model_broker::Kind::Remote && !self.disclosed_remote {
            self.disclosed_remote = true;
            self.audit.record(
                &self.package_id,
                &label,
                "model.infer",
                None,
                Enforcement::Broker,
                "model.disclosure",
                Some(plan_json.clone()),
                self.now(),
                AuditResult::Success,
                None,
            );
        }
        let models = Arc::clone(&self.models);
        match models.infer(&provider, &prepared).await {
            Ok(parts) => {
                self.reply_ok(
                    seq,
                    json!({"provider": provider.id, "plan": plan_json, "parts": parts}),
                )
                .await;
            }
            Err(failure) => {
                let code = match failure {
                    ono_model_broker::InferenceError::Timeout(_) => KuangErrorCode::RuntimeTimeout,
                    _ => KuangErrorCode::ModelProviderUnavailable,
                };
                let error = KuangError::new(code, failure.to_string());
                self.audit.record(
                    &self.package_id,
                    &label,
                    "model.infer",
                    None,
                    Enforcement::Broker,
                    "models.infer",
                    target,
                    self.now(),
                    AuditResult::Failed,
                    Some(error.clone().into()),
                );
                self.reply_err(seq, error.into()).await;
            }
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

#[cfg(test)]
mod tests {
    use super::at_memory_ceiling;

    #[test]
    fn should_read_an_instance_just_under_its_ceiling_as_having_reached_it() {
        // The figures the host actually observed of a package that allocated until it could not:
        // 66_568_192 bytes against a declared 64 MiB. The kernel refused the request that would
        // have crossed the line, so the last observation is below it and never above it.
        assert!(at_memory_ceiling(66_568_192, 67_108_864));
    }

    #[test]
    fn should_not_read_an_ordinary_crash_as_a_memory_limit() {
        // A package using an eighth of its room and then trapping did not run out of room, and
        // saying it did would be a story rather than a report (spec §35.3).
        assert!(!at_memory_ceiling(8 * 1024 * 1024, 67_108_864));
    }

    #[test]
    fn should_not_claim_a_ceiling_that_does_not_exist() {
        assert!(!at_memory_ceiling(0, 0));
    }
}

/// A duration as the wire spells it: seconds, a span like `30s`, or `{"$duration": …}`.
fn duration_of(value: &Json) -> Option<Duration> {
    match value {
        Json::Number(seconds) => seconds
            .as_f64()
            .filter(|seconds| *seconds > 0.0)
            .map(Duration::from_secs_f64),
        Json::String(text) => text
            .parse::<jiff::SignedDuration>()
            .ok()
            .and_then(|span| Duration::try_from(span).ok()),
        Json::Object(object) => object.get("$duration").and_then(duration_of),
        _ => None,
    }
}

/// The parameters of `models.infer`: the request, as `assistants.v1.yaml` shapes it.
#[derive(Debug, serde::Deserialize)]
struct ModelsInferParams {
    request: ono_model_broker::ModelRequest,
}

/// Whether `name` is inside the `key` name-list of `grant`'s scope: an absent scope or key
/// admits every name; `*`, `operator-selected` and a `prefix*` glob admit by pattern.
fn scope_names(grant: &crate::policy::Grant, key: &str, name: &str) -> bool {
    let Some(scope) = grant.scope.as_ref() else {
        return true;
    };
    let Some(Json::Array(patterns)) = scope.get(key) else {
        return true;
    };
    patterns.iter().any(|pattern| match pattern {
        Json::String(pattern) => {
            pattern == name
                || pattern == "*"
                || pattern == "operator-selected"
                || pattern
                    .strip_suffix('*')
                    .is_some_and(|prefix| name.starts_with(prefix))
        }
        _ => false,
    })
}
