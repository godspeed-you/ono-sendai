//! The plugin runtime: hello, init, dispatch, and credit-respecting emission.
//!
//! One loop reads frames and routes them; each invocation runs on a worker of its own, up to
//! the ceiling the negotiated contract carries (ADR-0586). Everything an invocation owns — its
//! credit window, its cancellation, the views it opened — is keyed by its output handle, so two
//! open invocations neither see nor starve each other.
//!
//! Where the target has no threads — the component tier is one thread and cannot make another —
//! the handler runs on the reading loop's own stack and pumps frames itself while it waits. The
//! ceiling is then one, and a second invocation is refused with `runtime.concurrency_limit`
//! rather than left unanswered.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError, mpsc};

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

/// A handler runs on a worker of its own, so it is shared across threads rather than owned by
/// one. A closure that captures nothing — which is what a handler usually is — satisfies this
/// without saying anything.
type Handler = Box<dyn Fn(&mut Ctx<'_>) -> Outcome + Send + Sync>;

/// A plugin under construction: identity, contributions, handlers.
pub struct Plugin {
    package: String,
    version: String,
    kuang_api: String,
    contributions: ContributionSet,
    commands: HashMap<String, Handler>,
    providers: HashMap<String, Handler>,
    features: Vec<(String, String)>,
    concurrency: u32,
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
            // One at a time is the default, because it is the only model whose safety the SDK
            // can guarantee on the author's behalf (ADR-0586).
            concurrency: 1,
        }
    }

    /// Overrides the host API range the plugin declares.
    #[must_use]
    pub fn kuang_api(mut self, range: &str) -> Self {
        self.kuang_api = range.to_owned();
        self
    }

    /// Declares that this package's handlers may run at the same time as one another, up to
    /// `at_once` of them.
    ///
    /// The default is one: a package that says nothing answers one invocation at a time and a
    /// second one is refused with `runtime.concurrency_limit` while the first is open. Saying
    /// more than one is a statement about the *code* — every handler must be safe to run
    /// beside itself and beside its siblings, because each runs on a worker of its own.
    ///
    /// The number here is what the package is willing to do; the effective ceiling is the
    /// smaller of it and `max_concurrent_invocations` in the negotiated contract, which is the
    /// operator's (spec §31.15, ADR-0586).
    #[must_use]
    pub const fn concurrent_invocations(mut self, at_once: u32) -> Self {
        self.concurrency = at_once;
        self
    }

    /// Registers a command handler. The id must be the full
    /// `<package.id>.command.<kebab-name>` the package contributes (spec §31.5).
    #[must_use]
    pub fn command(
        mut self,
        id: &str,
        handler: impl Fn(&mut Ctx<'_>) -> Outcome + Send + Sync + 'static,
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
        handler: impl Fn(&mut Ctx<'_>) -> Outcome + Send + Sync + 'static,
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
        // The handles rather than their locks: a worker reads and writes its own frames, and a
        // lock guard does not cross a thread boundary. Nothing else in the process uses them.
        self.run_io(std::io::stdin(), std::io::stdout());
    }

    /// Runs the plugin over the given streams (exposed for tests).
    pub fn run_io(self, reader: impl Read + Send, writer: impl Write + Send) {
        let plugin = self;
        let shared = Shared {
            reader: Mutex::new(reader),
            writer: Mutex::new(writer),
            limits: FrameLimits::default(),
            seq: AtomicU64::new(0),
            contract: OnceLock::new(),
            calls: Mutex::new(HashMap::new()),
            live: Mutex::new(Live::default()),
            ended: AtomicBool::new(false),
        };
        let hello = Envelope::Hello(Hello {
            format: PACKAGE_FORMAT.to_owned(),
            package: plugin.package.clone(),
            version: plugin.version.clone(),
            kuang_api: plugin.kuang_api.clone(),
            contributions: plugin.contributions.clone(),
        });
        if shared.send(&hello).is_err() {
            return;
        }
        std::thread::scope(|scope| {
            let admit = Admit {
                plugin: &plugin,
                shared: &shared,
                scope,
            };
            // This loop never runs package code where there are threads to run it on, which is
            // what lets demand and cancellation reach an invocation that is already busy.
            while let Some(envelope) = shared.read_one() {
                if matches!(serve(&shared, envelope, Some(&admit)), Flow::Stop) {
                    break;
                }
            }
            // Whatever ended the loop, nothing else will arrive: every handler waiting on the
            // host has to learn that rather than wait for it.
            shared.ended.store(true, Ordering::SeqCst);
            lock(&shared.calls).clear();
            for invocation in lock(&shared.live).invocations.values() {
                // Notified while the state is held, so a worker between its last look at
                // `ended` and its wait cannot sleep through the news.
                let _waiting = lock(&invocation.state);
                invocation.wake.notify_all();
            }
        });
    }
}

/// Whether this target can run an invocation on a worker of its own.
///
/// The component tier is one WASI thread with no way to make another, so an invocation there
/// runs on the reading loop's own stack and the ceiling is one — the answer a package that
/// never opted in gets everywhere (ADR-0586).
const WORKERS: bool = cfg!(not(target_family = "wasm"));

/// Whether the reading loop should carry on.
enum Flow {
    Continue,
    Stop,
}

/// What may start an invocation. Only the reading loop holds one: a handler that pumps a frame
/// while it waits refuses a further invocation rather than nesting one inside itself.
struct Admit<'scope, 'env: 'scope, R, W> {
    plugin: &'env Plugin,
    shared: &'env Shared<R, W>,
    scope: &'scope std::thread::Scope<'scope, 'env>,
}

/// Serves one envelope: a response goes to the call that made it, a request is answered.
fn serve<'scope, 'env: 'scope, R: Read + Send, W: Write + Send>(
    shared: &Shared<R, W>,
    envelope: Envelope,
    admit: Option<&Admit<'scope, 'env, R, W>>,
) -> Flow {
    let Envelope::Request {
        seq,
        method: method_name,
        params,
    } = envelope
    else {
        if let Envelope::Response { seq, result, error } = envelope {
            shared.route_answer(seq, result, error);
        }
        return Flow::Continue;
    };
    let served = match method_name.as_str() {
        method::LIFECYCLE_INIT => {
            let contract: Option<PluginContract> = params
                .get("contract")
                .cloned()
                .and_then(|contract| serde_json::from_value(contract).ok());
            let disabled: Vec<String> = match (contract.as_ref(), admit) {
                (Some(contract), Some(admit)) => admit
                    .plugin
                    .features
                    .iter()
                    .filter(|(_, capability)| {
                        contract
                            .denied
                            .iter()
                            .any(|denied| denied.capability == *capability)
                    })
                    .map(|(feature, _)| feature.clone())
                    .collect(),
                _ => Vec::new(),
            };
            if let Some(contract) = contract {
                let _ = shared.contract.set(contract);
            }
            let result = InitResult {
                ready: true,
                disabled_features: disabled,
                error: None,
            };
            shared.reply(seq, serde_json::to_value(result).unwrap_or(Json::Null))
        }
        method::LIFECYCLE_SHUTDOWN => {
            let answered = shared.reply(seq, Json::Null);
            // Cancellation is what a handler already knows how to observe, so a shutdown
            // reaches every open invocation as one.
            for invocation in lock(&shared.live).invocations.values() {
                lock(&invocation.state).cancelled = true;
                invocation.wake.notify_all();
            }
            let _ = answered;
            return Flow::Stop;
        }
        method::HEALTH_PROBE => {
            let busy = !lock(&shared.live).invocations.is_empty();
            let probe = ProbeResult {
                state: if busy {
                    HealthState::Busy
                } else {
                    HealthState::Ready
                },
                detail: None,
            };
            shared.reply(seq, serde_json::to_value(probe).unwrap_or(Json::Null))
        }
        method::COMMAND_INVOKE => {
            let Ok(invoke) = serde_json::from_value::<InvokeParams>(params) else {
                return Flow::Stop;
            };
            let started = Started {
                name: invoke.command,
                arguments: invoke.arguments,
                output: invoke.output,
                credit: invoke.credit,
            };
            match admit {
                Some(admit) => {
                    let handler = admit.plugin.commands.get(&started.name);
                    start(admit, seq, handler, started)
                }
                None => shared.refuse(seq, &started.name, 1),
            }
        }
        method::PROVIDER_QUERY => {
            let Ok(query) = serde_json::from_value::<QueryParams>(params) else {
                return Flow::Stop;
            };
            let started = Started {
                name: query.target,
                arguments: query.options,
                output: query.output,
                credit: query.credit,
            };
            match admit {
                Some(admit) => {
                    let handler = admit.plugin.providers.get(&started.name);
                    start(admit, seq, handler, started)
                }
                None => shared.refuse(seq, &started.name, 1),
            }
        }
        method::STREAM_DEMAND => {
            if let Ok(demand) = serde_json::from_value::<DemandParams>(params)
                && let Some(invocation) = shared.invocation(demand.handle)
            {
                {
                    let mut state = lock(&invocation.state);
                    state.credit = state.credit.saturating_add(demand.credit);
                }
                invocation.wake.notify_all();
            }
            shared.reply(seq, Json::Null)
        }
        method::STREAM_CANCEL => {
            if let Ok(cancel) = serde_json::from_value::<CancelParams>(params)
                && let Some(invocation) = shared.invocation(cancel.handle)
            {
                lock(&invocation.state).cancelled = true;
                invocation.wake.notify_all();
            }
            shared.reply(seq, Json::Null)
        }
        method::VIEW_MOUNT | method::VIEW_EVENT | method::VIEW_UNMOUNT => {
            // Queued for the invocation that opened the view; the host does not wait for the
            // answer.
            let answered = shared.reply(seq, Json::Null);
            let mut event = params;
            if let Some(object) = event.as_object_mut() {
                let kind = match method_name.as_str() {
                    method::VIEW_MOUNT => "mount",
                    method::VIEW_UNMOUNT => "unmount",
                    _ => "",
                };
                if !kind.is_empty() {
                    object.insert("event".to_owned(), json!({"kind": kind}));
                }
            }
            shared.deliver_view_event(event);
            answered
        }
        _ => shared.reply_error(
            seq,
            KuangError::new(
                KuangErrorCode::RuntimeProtocolViolation,
                format!("the plugin does not implement `{method_name}`"),
            )
            .into(),
        ),
    };
    if served.is_err() {
        return Flow::Stop;
    }
    Flow::Continue
}

/// The effective ceiling: the smaller of what the package is willing to do and what the
/// operator agreed to. One is the floor — an instance that may run nothing serves nothing.
fn ceiling<R, W>(plugin: &Plugin, shared: &Shared<R, W>) -> usize {
    if !WORKERS {
        return 1;
    }
    let negotiated = shared
        .contract
        .get()
        .map_or(plugin.concurrency, |contract| {
            contract.limits.max_concurrent_invocations
        });
    usize::try_from(plugin.concurrency.min(negotiated).max(1)).unwrap_or(1)
}

/// What one `command.invoke` or `provider.query` carried.
struct Started {
    name: String,
    arguments: JsonMap<String, Json>,
    output: u64,
    credit: u32,
}

/// Answers an invocation: on a worker of its own, or with the refusal that says why not.
fn start<'scope, 'env: 'scope, R: Read + Send, W: Write + Send>(
    admit: &Admit<'scope, 'env, R, W>,
    seq: u64,
    handler: Option<&'env Handler>,
    started: Started,
) -> Result<(), EmitError> {
    let shared = admit.shared;
    let Some(handler) = handler else {
        return shared.answer(
            seq,
            InvokeStatus::Failed,
            Some(
                KuangError::new(
                    KuangErrorCode::RuntimeProtocolViolation,
                    format!("no handler for `{}`", started.name),
                )
                .into(),
            ),
        );
    };
    let ceiling = ceiling(admit.plugin, shared);
    let invocation = Arc::new(Invocation {
        output: started.output,
        state: Mutex::new(InvocationState {
            credit: started.credit,
            cancelled: false,
            events: VecDeque::new(),
        }),
        wake: Condvar::new(),
    });
    {
        let mut live = lock(&shared.live);
        if live.invocations.len() >= ceiling {
            drop(live);
            return shared.refuse(seq, &started.name, ceiling);
        }
        live.invocations
            .insert(started.output, Arc::clone(&invocation));
    }
    let output = started.output;
    let name = started.name;
    let arguments = started.arguments;
    let work = move || {
        // A handler that panics fails its own invocation and nothing else: spec §31.34 asks
        // that a plugin's failure degrade the plugin, and a sibling invocation is not it.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut ctx = Ctx {
                io: shared,
                invocation: &invocation,
                arguments,
            };
            handler(&mut ctx)
        }))
        .unwrap_or_else(|_| {
            Outcome::Failed(
                KuangError::new(
                    KuangErrorCode::RuntimeTrap,
                    format!("the handler for `{name}` panicked"),
                )
                .into(),
            )
        });
        // Every handle belongs to exactly one invocation and dies with it
        // (`protocol.v1.yaml` → handles).
        {
            let mut live = lock(&shared.live);
            live.invocations.remove(&output);
            live.views.retain(|_, owner| *owner != output);
        }
        let (status, error) = match outcome {
            Outcome::Completed => (InvokeStatus::Completed, None),
            Outcome::Cancelled => (InvokeStatus::Cancelled, None),
            Outcome::Failed(error) => (InvokeStatus::Failed, Some(error)),
        };
        let _ = shared.answer(seq, status, error);
    };
    if WORKERS {
        admit.scope.spawn(work);
    } else {
        // No thread to run it on: the handler runs here and serves its own frames while it
        // waits, which is the only shape the component tier has.
        work();
    }
    Ok(())
}

/// A mutex guard that survives a panicking handler: the data behind it is the transport's, and
/// a poisoned lock would take the siblings down with the one that failed.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What one invocation owns.
struct Invocation {
    output: u64,
    state: Mutex<InvocationState>,
    wake: Condvar,
}

struct InvocationState {
    /// The credit window of this invocation's output stream, and no other's.
    credit: u32,
    cancelled: bool,
    /// View events for views this invocation opened, in the order the host sent them.
    events: VecDeque<Json>,
}

/// The invocations open right now, and who owns which view.
#[derive(Default)]
struct Live {
    invocations: HashMap<u64, Arc<Invocation>>,
    views: HashMap<u64, u64>,
}

/// Why a response is being routed, so the transport applies what is its before the handler
/// that asked sees the answer.
enum CallKind {
    /// Nothing but the answer.
    Plain,
    /// A `streams.emit`: the answer carries the stream's new credit, and applying it here keeps
    /// credit accounting in frame order rather than in worker-wakeup order.
    Emit(u64),
    /// A `views.open`: the answer names the view handle this invocation now owns.
    ViewOpen(u64),
}

struct PendingCall {
    kind: CallKind,
    answer: mpsc::Sender<(Option<Json>, Option<WireError>)>,
}

/// Why a host call did not answer.
enum CallFailure {
    /// The host is gone.
    Transport,
    /// The host answered, with a refusal.
    Refused(WireError),
}

/// The transport, shared by the reading loop and every worker.
struct Shared<R, W> {
    reader: Mutex<R>,
    writer: Mutex<W>,
    limits: FrameLimits,
    seq: AtomicU64,
    contract: OnceLock<PluginContract>,
    calls: Mutex<HashMap<u64, PendingCall>>,
    live: Mutex<Live>,
    ended: AtomicBool,
}

impl<R: Read + Send, W: Write + Send> Shared<R, W> {
    /// Reads one frame, holding the reader only while it is being read: a handler that pumps
    /// while it waits reads through the same door.
    fn read_one(&self) -> Option<Envelope> {
        let mut reader = lock(&self.reader);
        ono_kuang_protocol::read_frame(&mut *reader, self.limits)
            .ok()
            .flatten()
    }

    /// Serves one frame on the caller's own stack. Only a handler on a threadless target calls
    /// this; everywhere else the reading loop is a thread of its own.
    fn pump_once(&self) -> Flow {
        match self.read_one() {
            Some(envelope) => serve(self, envelope, None),
            None => Flow::Stop,
        }
    }

    /// Waits for whatever the reading loop will deliver, or reads it here when there is no
    /// reading loop but this one.
    fn idle(&self, invocation: &Invocation, state: MutexGuard<'_, InvocationState>) {
        if WORKERS {
            drop(
                invocation
                    .wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            );
        } else {
            drop(state);
            if matches!(self.pump_once(), Flow::Stop) {
                self.ended.store(true, Ordering::SeqCst);
            }
        }
    }

    /// The answer to an invocation the package has no room for.
    fn refuse(&self, seq: u64, name: &str, ceiling: usize) -> Result<(), EmitError> {
        self.answer(
            seq,
            InvokeStatus::Failed,
            Some(
                KuangError::new(
                    KuangErrorCode::RuntimeConcurrencyLimit,
                    format!(
                        "`{name}` was refused: the package already has {ceiling} invocation(s) \
                         open, which is its negotiated ceiling"
                    ),
                )
                .into(),
            ),
        )
    }

    fn send(&self, envelope: &Envelope) -> Result<(), EmitError> {
        let mut writer = lock(&self.writer);
        ono_kuang_protocol::write_frame(&mut *writer, envelope, self.limits)
            .map_err(|_| EmitError::Transport)
    }

    fn reply(&self, seq: u64, result: Json) -> Result<(), EmitError> {
        self.send(&Envelope::Response {
            seq,
            result: Some(result),
            error: None,
        })
    }

    fn reply_error(&self, seq: u64, error: WireError) -> Result<(), EmitError> {
        self.send(&Envelope::Response {
            seq,
            result: None,
            error: Some(error),
        })
    }

    /// Answers a `command.invoke` or `provider.query`.
    fn answer(
        &self,
        seq: u64,
        status: InvokeStatus,
        error: Option<WireError>,
    ) -> Result<(), EmitError> {
        let result = InvokeResult { status, error };
        self.reply(seq, serde_json::to_value(result).unwrap_or(Json::Null))
    }

    fn invocation(&self, output: u64) -> Option<Arc<Invocation>> {
        lock(&self.live).invocations.get(&output).map(Arc::clone)
    }

    /// Routes one response to the call that made it, applying first whatever the transport owns.
    fn route_answer(&self, seq: u64, result: Option<Json>, error: Option<WireError>) {
        let Some(pending) = lock(&self.calls).remove(&seq) else {
            // An answer to a call nobody is waiting on: the invocation that made it has ended.
            return;
        };
        match pending.kind {
            CallKind::Plain => {}
            CallKind::Emit(output) => {
                if let Some(emit) = result
                    .clone()
                    .and_then(|result| serde_json::from_value::<EmitResult>(result).ok())
                    && let Some(invocation) = self.invocation(output)
                {
                    lock(&invocation.state).credit = emit.credit;
                    invocation.wake.notify_all();
                }
            }
            CallKind::ViewOpen(output) => {
                if let Some(view) = result
                    .as_ref()
                    .and_then(|result| result.get("handle"))
                    .and_then(Json::as_u64)
                {
                    lock(&self.live).views.insert(view, output);
                }
            }
        }
        let _ = pending.answer.send((result, error));
    }

    /// Hands a view event to the invocation that opened the view.
    fn deliver_view_event(&self, event: Json) {
        let view = event.get("view").and_then(Json::as_u64);
        let owner = view.and_then(|view| lock(&self.live).views.get(&view).copied());
        let Some(invocation) = owner.and_then(|owner| self.invocation(owner)) else {
            return;
        };
        lock(&invocation.state).events.push_back(event);
        invocation.wake.notify_all();
    }

    /// Sends a request and waits for its answer. The dispatcher keeps reading meanwhile, so a
    /// waiting handler blocks nothing but itself.
    fn call(
        &self,
        kind: CallKind,
        method_name: &str,
        params: Json,
    ) -> Result<Json, Box<CallFailure>> {
        let seq = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let (answer, waiting) = mpsc::channel();
        lock(&self.calls).insert(seq, PendingCall { kind, answer });
        if self.ended.load(Ordering::SeqCst) {
            // The dispatcher has stopped reading, so nothing will ever answer this.
            lock(&self.calls).remove(&seq);
            return Err(Box::new(CallFailure::Transport));
        }
        let request = Envelope::Request {
            seq,
            method: method_name.to_owned(),
            params,
        };
        if self.send(&request).is_err() {
            lock(&self.calls).remove(&seq);
            return Err(Box::new(CallFailure::Transport));
        }
        let answered = if WORKERS {
            waiting.recv().ok()
        } else {
            loop {
                match waiting.try_recv() {
                    Ok(answer) => break Some(answer),
                    Err(mpsc::TryRecvError::Disconnected) => break None,
                    Err(mpsc::TryRecvError::Empty) => {
                        if self.ended.load(Ordering::SeqCst) {
                            break None;
                        }
                        if matches!(self.pump_once(), Flow::Stop) {
                            self.ended.store(true, Ordering::SeqCst);
                        }
                    }
                }
            }
        };
        match answered {
            Some((result, None)) => Ok(result.unwrap_or(Json::Null)),
            Some((_, Some(error))) => Err(Box::new(CallFailure::Refused(error))),
            None => Err(Box::new(CallFailure::Transport)),
        }
    }
}

fn transport_error() -> WireError {
    KuangError::new(KuangErrorCode::RuntimeTrap, "the host connection ended").into()
}

/// Object-safe view of the transport so `Ctx` does not carry its type parameter.
trait IoDyn {
    fn emit_value(&self, invocation: &Invocation, value: Json) -> Result<(), EmitError>;
    fn call_host(
        &self,
        invocation: &Invocation,
        method_name: &str,
        params: Json,
    ) -> Result<Json, WireError>;
    fn contract(&self) -> Option<&PluginContract>;
    fn next_view_event(&self, invocation: &Invocation) -> Result<Json, WireError>;
}

impl<R: Read + Send, W: Write + Send> IoDyn for Shared<R, W> {
    fn emit_value(&self, invocation: &Invocation, value: Json) -> Result<(), EmitError> {
        loop {
            let mut state = lock(&invocation.state);
            if state.cancelled {
                return Err(EmitError::Cancelled);
            }
            if self.ended.load(Ordering::SeqCst) {
                return Err(EmitError::Transport);
            }
            if state.credit > 0 {
                state.credit -= 1;
                break;
            }
            // No credit: wait for the host's demand instead of outrunning it (spec §31.15).
            // The wait is this invocation's alone; a sibling's stream is untouched by it.
            self.idle(invocation, state);
        }
        let params = serde_json::to_value(EmitParams {
            handle: invocation.output,
            values: vec![value],
        })
        .unwrap_or(Json::Null);
        match self.call(
            CallKind::Emit(invocation.output),
            method::STREAMS_EMIT,
            params,
        ) {
            Ok(_) => Ok(()),
            Err(failure) => match *failure {
                CallFailure::Transport => Err(EmitError::Transport),
                CallFailure::Refused(error) => Err(EmitError::Refused(Box::new(error))),
            },
        }
    }

    fn call_host(
        &self,
        invocation: &Invocation,
        method_name: &str,
        params: Json,
    ) -> Result<Json, WireError> {
        let kind = if method_name == method::VIEWS_OPEN {
            CallKind::ViewOpen(invocation.output)
        } else {
            CallKind::Plain
        };
        self.call(kind, method_name, params)
            .map_err(|failure| match *failure {
                CallFailure::Transport => transport_error(),
                CallFailure::Refused(error) => error,
            })
    }

    fn next_view_event(&self, invocation: &Invocation) -> Result<Json, WireError> {
        loop {
            let mut state = lock(&invocation.state);
            if let Some(event) = state.events.pop_front() {
                return Ok(event);
            }
            if self.ended.load(Ordering::SeqCst) {
                return Ok(json!({"event": {"kind": "close"}}));
            }
            self.idle(invocation, state);
        }
    }

    fn contract(&self) -> Option<&PluginContract> {
        self.contract.get()
    }
}

/// The context a handler works in: its arguments, its output stream, and the host API
/// (spec §31.12) — every call subject to the capability broker on the other side.
pub struct Ctx<'io> {
    io: &'io dyn IoDyn,
    invocation: &'io Invocation,
    arguments: JsonMap<String, Json>,
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

    /// Whether the host has cancelled this invocation's stream. Cancelling one invocation says
    /// nothing about any other.
    #[must_use]
    pub fn cancelled(&self) -> bool {
        lock(&self.invocation.state).cancelled
    }

    /// Emits one typed value on the invocation's output stream, waiting for credit when the
    /// host has none to give.
    ///
    /// # Errors
    ///
    /// [`EmitError::Cancelled`] when the host cancelled the stream — return
    /// [`Outcome::Cancelled`]; [`EmitError::Refused`] when the host rejected the value.
    pub fn emit(&mut self, value: &Value) -> Result<(), EmitError> {
        self.io.emit_value(self.invocation, to_json(value))
    }

    /// Calls a host API method with raw parameters (spec §31.12). Every call answers with the
    /// structured denial of `docs/contracts/kuang/errors.v1.yaml` when a capability is missing.
    ///
    /// # Errors
    ///
    /// The structured error the host answered with.
    pub fn host_call(&mut self, method_name: &str, params: Json) -> Result<Json, WireError> {
        self.io.call_host(self.invocation, method_name, params)
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

    /// The next event of a view this invocation opened, in the order the host sent them:
    /// `mount`, `key`, `resize`, `focus`, `blur`, `cancel`, `close` and `unmount`. Blocks until
    /// one arrives; answers `close` once the host has asked the plugin to shut down.
    ///
    /// # Errors
    ///
    /// Transport failures only.
    pub fn next_view_event(&mut self) -> Result<ViewNotice, WireError> {
        let raw = self.io.next_view_event(self.invocation)?;
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
        serde_json::from_value(answer).map_err(|error| {
            KuangError::new(
                KuangErrorCode::RuntimeProtocolViolation,
                format!("the host's answer was not a check answer: {error}"),
            )
            .into()
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
