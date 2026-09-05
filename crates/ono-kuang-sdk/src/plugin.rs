//! The plugin runtime: hello, init, dispatch, and credit-respecting emission.

use std::collections::HashMap;
use std::io::{Read, Write};

use ono_kuang_protocol::{
    CancelParams, CheckAnswer, CheckParams, CommandContribution, ContributionSet, DemandParams,
    EmitParams, EmitResult, Envelope, FrameLimits, HealthState, Hello, InitResult, InvokeParams,
    InvokeResult, InvokeStatus, KuangError, KuangErrorCode, PACKAGE_FORMAT, PluginContract,
    ProbeResult, QueryParams, SchemaContribution, TargetContribution, ViewContribution, ViewEvent,
    ViewHandleParams, ViewOpenParams, ViewOpenResult, ViewSize, ViewSubmitParams, WireError,
    method,
};
use ono_value::{Value, to_json};
use serde_json::{Map as JsonMap, Value as Json, json};

/// How a handler ended its invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The invocation ran to completion.
    Completed,
    /// The invocation failed with a structured error.
    Failed(WireError),
    /// The invocation observed cancellation and stopped (spec §31.86: cancellation behaves as
    /// it does for a core stage).
    Cancelled,
}

/// Why an emission was not delivered.
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    /// The host cancelled the stream. The handler should stop and return
    /// [`Outcome::Cancelled`].
    #[error("the stream was cancelled by the host")]
    Cancelled,
    /// The host refused the value — a schema violation closes the stream.
    ///
    /// Boxed so the whole error stays small enough to travel in a `Result` on the emission path
    /// without the lint about it: a refusal is rare and a `WireError` carries a metadata map
    /// (ADR-0228).
    #[error("the host refused the emission: {0}")]
    Refused(Box<WireError>),
    /// The connection to the host is gone.
    #[error("the host connection ended")]
    Transport,
}

type Handler = Box<dyn Fn(&mut Ctx<'_>) -> Outcome>;

/// A plugin under construction: identity, contributions, handlers.
pub struct Plugin {
    package: String,
    version: String,
    kuang_api: String,
    contributions: ContributionSet,
    commands: HashMap<String, Handler>,
    providers: HashMap<String, Handler>,
    features: Vec<(String, String)>,
}

impl Plugin {
    /// A plugin for `package` at `version`, speaking the current host API major.
    #[must_use]
    pub fn new(package: &str, version: &str) -> Self {
        Self {
            package: package.to_owned(),
            version: version.to_owned(),
            kuang_api: ">=11.1 <12".to_owned(),
            contributions: ContributionSet::default(),
            commands: HashMap::new(),
            providers: HashMap::new(),
            features: Vec::new(),
        }
    }

    /// Overrides the host API range the plugin declares.
    #[must_use]
    pub fn kuang_api(mut self, range: &str) -> Self {
        self.kuang_api = range.to_owned();
        self
    }

    /// Registers a command handler. The id must be the full
    /// `<package.id>.command.<kebab-name>` the package contributes (spec §31.5).
    #[must_use]
    pub fn command(
        mut self,
        id: &str,
        handler: impl Fn(&mut Ctx<'_>) -> Outcome + 'static,
    ) -> Self {
        self.commands.insert(id.to_owned(), Box::new(handler));
        self
    }

    /// Declares the contract metadata for a contributed command (spec §31.22).
    #[must_use]
    pub fn contribute_command(mut self, contribution: CommandContribution) -> Self {
        self.contributions.commands.push(contribution);
        self
    }

    /// Registers a provider handler for a contributed target (spec §31.23): queries arrive
    /// over the protocol, the handler answers as a value stream.
    #[must_use]
    pub fn provider(
        mut self,
        target: &str,
        handler: impl Fn(&mut Ctx<'_>) -> Outcome + 'static,
    ) -> Self {
        self.providers.insert(target.to_owned(), Box::new(handler));
        self
    }

    /// Declares a contributed target.
    #[must_use]
    pub fn contribute_target(mut self, contribution: TargetContribution) -> Self {
        self.contributions.targets.push(contribution);
        self
    }

    /// Declares a contributed view (spec §31.27).
    #[must_use]
    pub fn contribute_view(mut self, contribution: ViewContribution) -> Self {
        self.contributions.views.push(contribution);
        self
    }

    /// Declares a contributed schema.
    #[must_use]
    pub fn contribute_schema(mut self, contribution: SchemaContribution) -> Self {
        self.contributions.schemas.push(contribution);
        self
    }

    /// Names a feature that depends on an optional capability. When the negotiated contract
    /// denies the capability, the feature appears in `lifecycle.init`'s `disabled_features` —
    /// the plugin adapts once instead of re-prompting (spec §31.63).
    #[must_use]
    pub fn optional_feature(mut self, feature: &str, capability: &str) -> Self {
        self.features
            .push((feature.to_owned(), capability.to_owned()));
        self
    }

    /// Runs the plugin over stdin/stdout until the host shuts it down. This call does not
    /// return while the instance is serving.
    pub fn run(self) {
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        self.run_io(stdin.lock(), stdout.lock());
    }

    /// Runs the plugin over the given streams (exposed for tests).
    pub fn run_io(self, reader: impl Read, writer: impl Write) {
        let mut io = Io {
            reader,
            writer,
            limits: FrameLimits::default(),
            seq: 0,
            contract: None,
            current: None,
            shutdown: false,
            view_events: std::collections::VecDeque::new(),
        };
        let hello = Envelope::Hello(Hello {
            format: PACKAGE_FORMAT.to_owned(),
            package: self.package.clone(),
            version: self.version.clone(),
            kuang_api: self.kuang_api.clone(),
            contributions: self.contributions.clone(),
        });
        if io.send(&hello).is_err() {
            return;
        }
        while !io.shutdown {
            let Ok(Some(envelope)) = io.read() else {
                return;
            };
            let Envelope::Request {
                seq,
                method: method_name,
                params,
            } = envelope
            else {
                // A response with no request outstanding: nothing sensible to do but stop.
                return;
            };
            match method_name.as_str() {
                method::LIFECYCLE_INIT => {
                    let contract: Option<PluginContract> = params
                        .get("contract")
                        .cloned()
                        .and_then(|contract| serde_json::from_value(contract).ok());
                    let disabled: Vec<String> =
                        contract.as_ref().map_or_else(Vec::new, |contract| {
                            self.features
                                .iter()
                                .filter(|(_, capability)| {
                                    contract
                                        .denied
                                        .iter()
                                        .any(|denied| denied.capability == *capability)
                                })
                                .map(|(feature, _)| feature.clone())
                                .collect()
                        });
                    io.contract = contract;
                    let result = InitResult {
                        ready: true,
                        disabled_features: disabled,
                        error: None,
                    };
                    if io
                        .reply(seq, serde_json::to_value(result).unwrap_or(Json::Null))
                        .is_err()
                    {
                        return;
                    }
                }
                method::LIFECYCLE_SHUTDOWN => {
                    let _ = io.reply(seq, Json::Null);
                    return;
                }
                method::HEALTH_PROBE => {
                    let probe = ProbeResult {
                        state: HealthState::Ready,
                        detail: None,
                    };
                    if io
                        .reply(seq, serde_json::to_value(probe).unwrap_or(Json::Null))
                        .is_err()
                    {
                        return;
                    }
                }
                method::COMMAND_INVOKE => {
                    let Ok(invoke) = serde_json::from_value::<InvokeParams>(params) else {
                        return;
                    };
                    let handler = self.commands.get(&invoke.command);
                    let result = io.run_invocation(
                        handler,
                        invoke.arguments,
                        invoke.output,
                        invoke.credit,
                        &invoke.command,
                    );
                    if io
                        .reply(seq, serde_json::to_value(result).unwrap_or(Json::Null))
                        .is_err()
                    {
                        return;
                    }
                }
                method::PROVIDER_QUERY => {
                    let Ok(query) = serde_json::from_value::<QueryParams>(params) else {
                        return;
                    };
                    let handler = self.providers.get(&query.target);
                    let result = io.run_invocation(
                        handler,
                        query.options,
                        query.output,
                        query.credit,
                        &query.target,
                    );
                    if io
                        .reply(seq, serde_json::to_value(result).unwrap_or(Json::Null))
                        .is_err()
                    {
                        return;
                    }
                }
                method::STREAM_DEMAND | method::STREAM_CANCEL => {
                    // No invocation is running; a late control message needs only an answer.
                    if io.reply(seq, Json::Null).is_err() {
                        return;
                    }
                }
                _ => {
                    let error = WireError {
                        code: "Ono-Sendai-K11204".to_owned(),
                        name: "runtime.protocol_violation".to_owned(),
                        message: format!("the plugin does not implement `{method_name}`"),
                        help: None,
                        metadata: Box::default(),
                    };
                    if io.reply_error(seq, error).is_err() {
                        return;
                    }
                }
            }
        }
    }
}

struct Current {
    output: u64,
    credit: u32,
    cancelled: bool,
}

struct Io<R, W> {
    reader: R,
    writer: W,
    limits: FrameLimits,
    seq: u64,
    contract: Option<PluginContract>,
    current: Option<Current>,
    shutdown: bool,
    /// View events the host sent while a command was busy, in order (ADR-0572).
    view_events: std::collections::VecDeque<Json>,
}

impl<R: Read, W: Write> Io<R, W> {
    fn send(&mut self, envelope: &Envelope) -> Result<(), EmitError> {
        ono_kuang_protocol::write_frame(&mut self.writer, envelope, self.limits)
            .map_err(|_| EmitError::Transport)
    }

    fn read(&mut self) -> Result<Option<Envelope>, EmitError> {
        ono_kuang_protocol::read_frame(&mut self.reader, self.limits)
            .map_err(|_| EmitError::Transport)
    }

    fn reply(&mut self, seq: u64, result: Json) -> Result<(), EmitError> {
        self.send(&Envelope::Response {
            seq,
            result: Some(result),
            error: None,
        })
    }

    fn reply_error(&mut self, seq: u64, error: WireError) -> Result<(), EmitError> {
        self.send(&Envelope::Response {
            seq,
            result: None,
            error: Some(error),
        })
    }

    fn run_invocation(
        &mut self,
        handler: Option<&Handler>,
        arguments: JsonMap<String, Json>,
        output: u64,
        credit: u32,
        name: &str,
    ) -> InvokeResult {
        let Some(handler) = handler else {
            return InvokeResult {
                status: InvokeStatus::Failed,
                error: Some(WireError {
                    code: "Ono-Sendai-K11204".to_owned(),
                    name: "runtime.protocol_violation".to_owned(),
                    message: format!("no handler for `{name}`"),
                    help: None,
                    metadata: Box::default(),
                }),
            };
        };
        self.current = Some(Current {
            output,
            credit,
            cancelled: false,
        });
        let outcome = {
            let mut ctx = Ctx {
                io: self,
                arguments,
            };
            handler(&mut ctx)
        };
        self.current = None;
        match outcome {
            Outcome::Completed => InvokeResult {
                status: InvokeStatus::Completed,
                error: None,
            },
            Outcome::Cancelled => InvokeResult {
                status: InvokeStatus::Cancelled,
                error: None,
            },
            Outcome::Failed(error) => InvokeResult {
                status: InvokeStatus::Failed,
                error: Some(error),
            },
        }
    }

    /// Handles one incoming envelope while a call of our own is outstanding. Returns the
    /// response when it answers `waiting_for`.
    fn pump(
        &mut self,
        waiting_for: Option<u64>,
    ) -> Result<Option<(Option<Json>, Option<WireError>)>, EmitError> {
        let Some(envelope) = self.read()? else {
            return Err(EmitError::Transport);
        };
        match envelope {
            Envelope::Response { seq, result, error } => {
                if waiting_for == Some(seq) {
                    Ok(Some((result, error)))
                } else {
                    // An answer to a call this invocation is not waiting on: drop it.
                    Ok(None)
                }
            }
            Envelope::Request {
                seq,
                method: method_name,
                params,
            } => {
                match method_name.as_str() {
                    method::STREAM_DEMAND => {
                        if let Ok(demand) = serde_json::from_value::<DemandParams>(params)
                            && let Some(current) = &mut self.current
                            && current.output == demand.handle
                        {
                            current.credit = current.credit.saturating_add(demand.credit);
                        }
                        self.reply(seq, Json::Null)?;
                    }
                    method::STREAM_CANCEL => {
                        if let Ok(cancel) = serde_json::from_value::<CancelParams>(params)
                            && let Some(current) = &mut self.current
                            && current.output == cancel.handle
                        {
                            current.cancelled = true;
                        }
                        self.reply(seq, Json::Null)?;
                    }
                    method::HEALTH_PROBE => {
                        let probe = ProbeResult {
                            state: HealthState::Busy,
                            detail: None,
                        };
                        self.reply(seq, serde_json::to_value(probe).unwrap_or(Json::Null))?;
                    }
                    method::LIFECYCLE_SHUTDOWN => {
                        self.reply(seq, Json::Null)?;
                        self.shutdown = true;
                        if let Some(current) = &mut self.current {
                            current.cancelled = true;
                        }
                    }
                    method::VIEW_MOUNT | method::VIEW_EVENT | method::VIEW_UNMOUNT => {
                        // Queued for `next_view_event`; the host does not wait for the answer.
                        self.reply(seq, Json::Null)?;
                        let mut event = params;
                        if let Some(object) = event.as_object_mut() {
                            let kind = match method_name.as_str() {
                                method::VIEW_MOUNT => "mount",
                                method::VIEW_UNMOUNT => "unmount",
                                _ => "",
                            };
                            if !kind.is_empty() {
                                object
                                    .insert("event".to_owned(), serde_json::json!({"kind": kind}));
                            }
                        }
                        self.view_events.push_back(event);
                    }
                    _ => {
                        self.reply(seq, Json::Null)?;
                    }
                }
                Ok(None)
            }
            Envelope::Hello(_) => Ok(None),
        }
    }

    /// Sends a request and waits for its answer, serving interleaved host requests meanwhile.
    fn call(&mut self, method_name: &str, params: Json) -> Result<Json, WireError> {
        self.seq += 1;
        let seq = self.seq;
        let request = Envelope::Request {
            seq,
            method: method_name.to_owned(),
            params,
        };
        let transport = |_| WireError {
            code: "Ono-Sendai-K11201".to_owned(),
            name: "runtime.trap".to_owned(),
            message: "the host connection ended".to_owned(),
            help: None,
            metadata: Box::default(),
        };
        self.send(&request).map_err(transport)?;
        loop {
            if let Some((result, error)) = self.pump(Some(seq)).map_err(transport)? {
                return match error {
                    Some(error) => Err(error),
                    None => Ok(result.unwrap_or(Json::Null)),
                };
            }
        }
    }
}

/// The context a handler works in: its arguments, its output stream, and the host API
/// (spec §31.12) — every call subject to the capability broker on the other side.
pub struct Ctx<'io> {
    io: &'io mut dyn IoDyn,
    arguments: JsonMap<String, Json>,
}

/// Object-safe view of `Io` so `Ctx` does not carry the transport's type parameters.
trait IoDyn {
    fn emit_value(&mut self, value: Json) -> Result<(), EmitError>;
    fn call_host(&mut self, method_name: &str, params: Json) -> Result<Json, WireError>;
    fn is_cancelled(&self) -> bool;
    fn contract(&self) -> Option<&PluginContract>;
    fn next_view_event(&mut self) -> Result<Json, WireError>;
}

impl<R: Read, W: Write> IoDyn for Io<R, W> {
    fn emit_value(&mut self, value: Json) -> Result<(), EmitError> {
        loop {
            let Some(current) = &self.current else {
                return Err(EmitError::Transport);
            };
            if current.cancelled {
                return Err(EmitError::Cancelled);
            }
            if current.credit > 0 {
                break;
            }
            // No credit: wait for the host's demand instead of outrunning it (spec §31.15).
            self.pump(None)?;
            if self.shutdown {
                return Err(EmitError::Cancelled);
            }
        }
        let (handle, _) = {
            let current = self.current.as_mut().ok_or(EmitError::Transport)?;
            current.credit -= 1;
            (current.output, current.credit)
        };
        self.seq += 1;
        let seq = self.seq;
        let request = Envelope::Request {
            seq,
            method: method::STREAMS_EMIT.to_owned(),
            params: serde_json::to_value(EmitParams {
                handle,
                values: vec![value],
            })
            .unwrap_or(Json::Null),
        };
        self.send(&request)?;
        loop {
            if let Some((result, error)) = self.pump(Some(seq))? {
                if let Some(error) = error {
                    return Err(EmitError::Refused(Box::new(error)));
                }
                if let Ok(emit) = serde_json::from_value::<EmitResult>(result.unwrap_or(Json::Null))
                    && let Some(current) = &mut self.current
                {
                    current.credit = emit.credit;
                }
                return Ok(());
            }
        }
    }

    fn call_host(&mut self, method_name: &str, params: Json) -> Result<Json, WireError> {
        self.call(method_name, params)
    }

    fn next_view_event(&mut self) -> Result<Json, WireError> {
        loop {
            if let Some(event) = self.view_events.pop_front() {
                return Ok(event);
            }
            if self.shutdown {
                return Ok(serde_json::json!({"event": {"kind": "close"}}));
            }
            self.pump(None).map_err(|_| WireError {
                code: "Ono-Sendai-K11201".to_owned(),
                name: "runtime.trap".to_owned(),
                message: "the host connection ended".to_owned(),
                help: None,
                metadata: Box::default(),
            })?;
        }
    }

    fn is_cancelled(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.cancelled)
    }

    fn contract(&self) -> Option<&PluginContract> {
        self.contract.as_ref()
    }
}

impl Ctx<'_> {
    /// The invocation's arguments, already bound and typed by the host's command layer.
    #[must_use]
    pub const fn arguments(&self) -> &JsonMap<String, Json> {
        &self.arguments
    }

    /// The negotiated contract delivered by `lifecycle.init` (spec §31.63).
    #[must_use]
    pub fn contract(&self) -> Option<&PluginContract> {
        self.io.contract()
    }

    /// Whether the host has cancelled this invocation's stream.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        self.io.is_cancelled()
    }

    /// Emits one typed value on the invocation's output stream, waiting for credit when the
    /// host has none to give.
    ///
    /// # Errors
    ///
    /// [`EmitError::Cancelled`] when the host cancelled the stream — return
    /// [`Outcome::Cancelled`]; [`EmitError::Refused`] when the host rejected the value.
    pub fn emit(&mut self, value: &Value) -> Result<(), EmitError> {
        self.io.emit_value(to_json(value))
    }

    /// Calls a host API method with raw parameters (spec §31.12). Every call answers with the
    /// structured denial of `docs/contracts/kuang/errors.v1.yaml` when a capability is missing.
    ///
    /// # Errors
    ///
    /// The structured error the host answered with.
    pub fn host_call(&mut self, method_name: &str, params: Json) -> Result<Json, WireError> {
        self.io.call_host(method_name, params)
    }

    /// Opens a contributed view (spec §31.27). `mounted` is false when output is redirected,
    /// and the command emits the view's declared fallback instead (spec §31.28).
    ///
    /// # Errors
    ///
    /// The host's refusal: no `ui.view` grant, or a view the package does not contribute.
    pub fn open_view(&mut self, view: &str, input: Option<u64>) -> Result<OpenedView, WireError> {
        let params = serde_json::to_value(ViewOpenParams {
            view: view.to_owned(),
            input,
        })
        .unwrap_or(Json::Null);
        let answer = self.host_call(method::VIEWS_OPEN, params)?;
        let opened: ViewOpenResult = serde_json::from_value(answer).map_err(|error| {
            WireError::from(KuangError::new(
                KuangErrorCode::ViewProtocolError,
                format!("the host's answer to views.open is not one: {error}"),
            ))
        })?;
        Ok(OpenedView {
            handle: opened.handle,
            mounted: opened.mounted,
            size: opened.size,
        })
    }

    /// Submits a tree of the components in `contributions.v1.yaml` for the host to draw.
    ///
    /// # Errors
    ///
    /// `view.protocol_error` when the tree is invalid; the host has torn the view down.
    pub fn submit_view(&mut self, view: u64, tree: Json) -> Result<(), WireError> {
        let params = serde_json::to_value(ViewSubmitParams { view, tree }).unwrap_or(Json::Null);
        self.host_call(method::VIEWS_SUBMIT, params).map(|_| ())
    }

    /// Closes a view; the terminal comes back. Idempotent.
    ///
    /// # Errors
    ///
    /// Transport failures only.
    pub fn close_view(&mut self, view: u64) -> Result<(), WireError> {
        let params = serde_json::to_value(ViewHandleParams { view }).unwrap_or(Json::Null);
        self.host_call(method::VIEWS_CLOSE, params).map(|_| ())
    }

    /// The next event of any open view, in the order the host sent them: `mount`, `key`,
    /// `resize`, `focus`, `blur`, `cancel`, `close` and `unmount`. Blocks until one arrives;
    /// answers `close` once the host has asked the plugin to shut down.
    ///
    /// # Errors
    ///
    /// Transport failures only.
    pub fn next_view_event(&mut self) -> Result<ViewNotice, WireError> {
        let raw = self.io.next_view_event()?;
        let view = raw.get("view").and_then(Json::as_u64).unwrap_or_default();
        let event: ViewEvent = raw
            .get("event")
            .cloned()
            .and_then(|event| serde_json::from_value(event).ok())
            .unwrap_or(ViewEvent {
                kind: "close".to_owned(),
                key: None,
                size: None,
            });
        let size = event.size.or_else(|| {
            raw.get("size")
                .cloned()
                .and_then(|size| serde_json::from_value(size).ok())
        });
        Ok(ViewNotice {
            view,
            kind: event.kind,
            key: event.key,
            size,
        })
    }

    /// Checks a grant without prompting (spec §31.61's `capabilities.check`).
    ///
    /// # Errors
    ///
    /// The transport failure, when the host is gone.
    pub fn check_capability(&mut self, capability: &str) -> Result<CheckAnswer, WireError> {
        let params = serde_json::to_value(CheckParams {
            capability: capability.to_owned(),
            scope: None,
        })
        .unwrap_or(Json::Null);
        let answer = self.host_call(method::CAPABILITIES_CHECK, params)?;
        serde_json::from_value(answer).map_err(|error| WireError {
            code: "Ono-Sendai-K11204".to_owned(),
            name: "runtime.protocol_violation".to_owned(),
            message: format!("the host's answer was not a check answer: {error}"),
            help: None,
            metadata: Box::default(),
        })
    }

    /// Reads wall-clock time through the host (`clock.now`, costs `clock.read`).
    ///
    /// # Errors
    ///
    /// `capability.denied` without a `clock.read` grant.
    pub fn clock_now(&mut self) -> Result<String, WireError> {
        let result = self.host_call(method::CLOCK_NOW, json!({}))?;
        Ok(result
            .get("now")
            .and_then(|now| now.get("$timestamp"))
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned())
    }
}

/// What `views.open` answered (spec §31.27).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenedView {
    /// The view's handle.
    pub handle: u64,
    /// Whether a terminal took it; when not, emit the declared fallback.
    pub mounted: bool,
    /// The terminal's size when mounted.
    pub size: Option<ViewSize>,
}

/// One event for a view, as [`Ctx::next_view_event`] hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewNotice {
    /// The view it concerns.
    pub view: u64,
    /// `mount`, `key`, `resize`, `focus`, `blur`, `cancel`, `close` or `unmount`.
    pub kind: String,
    /// The key, for `key`.
    pub key: Option<String>,
    /// The size, for `mount` and `resize`.
    pub size: Option<ViewSize>,
}
