//! Binding a native implementation to a registry id, and running it (spec §27.2).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{CancelToken, ValueStream};
use ono_provider_api::{ActionOutcome, ProviderRegistry};
use ono_value::{ErrorValue, RecordValue, Value};

use crate::bind::BoundArguments;
use crate::contract::CommandContract;
use crate::expr::Scope;
use crate::registry::CommandRegistry;

/// One frame of the shell's context stack, as a command sees it (spec §14.1, ADR-0023).
///
/// A frame narrows; it never redirects. What it contributes here is exactly the implicit
/// selector of spec §14.3: `enter service nginx` makes `get process` ask for that service's
/// processes, and §14.5 requires the same query to be expressible without the context —
/// `get process --service nginx` — which is why the frame is a field name and a value, nothing
/// more.
#[derive(Debug, Clone, PartialEq)]
pub struct ContextFrame {
    kind: FrameKind,
    target: String,
    identity: Value,
    /// The handles the entered object answers to — `pid 1`, `name root`, `port 8080` — by the
    /// parameter names commands declare (ADR-0076). Empty for a frame that carries only its
    /// identity.
    handles: Vec<(String, Value)>,
}

/// What sort of frame this is (spec §14.1). Only an object frame narrows queries; a filesystem
/// frame's whole effect is the working directory it changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameKind {
    /// An entered object: `enter service nginx`.
    Object,
    /// An entered directory: `enter dir /etc` (spec §14.2).
    Filesystem,
    /// An entered remote link: `enter link prod-db` (spec §14.4). The frame decides *where*
    /// provider calls run; it narrows nothing.
    Link,
}

impl ContextFrame {
    /// A frame for the entered object: its target name and its identity.
    #[must_use]
    pub fn new(target: impl Into<String>, identity: Value) -> Self {
        Self {
            kind: FrameKind::Object,
            target: target.into(),
            identity,
            handles: Vec::new(),
        }
    }

    /// An object frame that knows the handles of the entered object (ADR-0076).
    ///
    /// The handles are the record's scalar fields by name, plus the scalar fields of its
    /// structural sub-records under their own names where nothing at the top level claims the
    /// name — so a socket's `local.port` answers to `port`, the parameter `get socket` and
    /// `trace socket --port` declare. A later command of the same target takes the first of its
    /// declared parameters that the frame has a handle for.
    #[must_use]
    pub fn of_record(target: impl Into<String>, identity: Value, record: &RecordValue) -> Self {
        let fields = |record: &RecordValue| -> Vec<(String, Value)> {
            record
                .schema()
                .fields()
                .iter()
                .filter_map(|field| {
                    Some((field.name().to_owned(), record.get(field.name())?.clone()))
                })
                .collect()
        };
        let top = fields(record);
        let mut handles: Vec<(String, Value)> = top
            .iter()
            .filter(|(_, value)| is_handle(value))
            .cloned()
            .collect();
        for (_, value) in &top {
            if let Value::Record(nested) = value {
                for (name, value) in fields(nested) {
                    if is_handle(&value) && !handles.iter().any(|(held, _)| *held == name) {
                        handles.push((name, value));
                    }
                }
            }
        }
        Self {
            kind: FrameKind::Object,
            target: target.into(),
            identity,
            handles,
        }
    }

    /// The value the entered object answers to under `parameter`, if it has one.
    #[must_use]
    pub fn handle(&self, parameter: &str) -> Option<&Value> {
        self.handles
            .iter()
            .find_map(|(name, value)| (name == parameter).then_some(value))
    }

    /// A frame for an entered directory (spec §14.2).
    #[must_use]
    pub fn filesystem(path: Value) -> Self {
        Self {
            kind: FrameKind::Filesystem,
            target: "dir".to_owned(),
            identity: path,
            handles: Vec::new(),
        }
    }

    /// A frame for an entered remote link (spec §14.4).
    #[must_use]
    pub fn link(host: Value) -> Self {
        Self {
            kind: FrameKind::Link,
            target: "link".to_owned(),
            identity: host,
            handles: Vec::new(),
        }
    }

    /// What sort of frame this is.
    #[must_use]
    pub fn kind(&self) -> FrameKind {
        self.kind
    }

    /// The target the frame narrows to, such as `service`.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// The identity of the entered object.
    #[must_use]
    pub fn identity(&self) -> &Value {
        &self.identity
    }

    /// The explicit spelling of what this frame contributes (spec §14.5).
    #[must_use]
    pub fn spelling(&self) -> String {
        format!("{} {}", self.target, self.identity)
    }
}

/// Whether a field value can stand for the object in a selector: a scalar, never a structure,
/// a null or a carried error.
fn is_handle(value: &Value) -> bool {
    !matches!(
        value,
        Value::Null | Value::Record(_) | Value::List(_) | Value::Map(_) | Value::Error(_)
    )
}

/// What a command produced.
///
/// A query answers with values; a mutation answers with one outcome per target, because spec
/// §16.5 forbids collapsing `97 succeeded, 3 failed` into a single ambiguous answer.
#[derive(Debug)]
pub enum Outcome {
    /// A stream of values, which may still be producing.
    Values(ValueStream),
    /// One outcome per object the command acted on (spec §11.5).
    Actions(Vec<ActionOutcome>),
}

/// Everything one command needs while it runs.
///
/// The invocation is built by the evaluator and handed to the implementation: the arguments are
/// already resolved against the declared types, the input stream is already connected, and the
/// cancellation token is the pipeline's, so one `Ctrl-C` stops every stage at its next await
/// (spec §18.5).
pub struct Invocation<'a> {
    contract: &'a CommandContract,
    arguments: &'a BoundArguments,
    providers: &'a ProviderRegistry,
    input: Option<ValueStream>,
    cancel: CancelToken,
    scope: Arc<Scope>,
    context: Vec<ContextFrame>,
    adapters: Option<Arc<ono_adapter::Registry>>,
    resolver: Option<Resolver>,
    displays: bool,
}

/// Resolves a program name to the path the shell would run, for planning (ADR-0056).
pub type Resolver = Arc<dyn Fn(&str) -> Option<std::path::PathBuf> + Send + Sync>;

impl std::fmt::Debug for Invocation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Invocation")
            .field("contract", &self.contract.id())
            .field("arguments", &self.arguments)
            .field("context", &self.context)
            .field("adapters", &self.adapters.is_some())
            .field("resolver", &self.resolver.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> Invocation<'a> {
    /// An invocation of `contract` with `arguments`, with no input and a fresh cancellation
    /// scope.
    #[must_use]
    pub fn new(
        contract: &'a CommandContract,
        arguments: &'a BoundArguments,
        providers: &'a ProviderRegistry,
    ) -> Self {
        Self {
            contract,
            arguments,
            providers,
            input: None,
            cancel: CancelToken::new(),
            scope: Arc::new(Scope::new()),
            context: Vec::new(),
            adapters: None,
            resolver: None,
            displays: false,
        }
    }

    /// States that what this stage produces will be shown to the user rather than consumed.
    ///
    /// Only the evaluator can know it: a command sees neither the stages after it nor where the
    /// statement's output goes. It is true for the last stage of a foreground statement with no
    /// redirection, when the shell's own output is a terminal — and false for `map | to json`,
    /// for `map > file`, for a captured substitution and for a background job.
    ///
    /// The one thing that turns on it is whether a full-screen view may open (spec v0.4 §23.3,
    /// §29.1): a value that is about to be consumed by another stage is a value, not a screen.
    #[must_use]
    pub const fn with_display(mut self, displays: bool) -> Self {
        self.displays = displays;
        self
    }

    /// Whether this stage's values will be shown to the user (spec v0.4 §29.1).
    #[must_use]
    pub const fn displays(&self) -> bool {
        self.displays
    }

    /// Makes the adapter registry and `PATH` resolution available to commands that plan —
    /// `type` — so an adapted stage's schema is known (spec v0.3 §1.61, ADR-0067).
    #[must_use]
    pub fn with_adapters(
        mut self,
        adapters: Arc<ono_adapter::Registry>,
        resolver: Resolver,
    ) -> Self {
        self.adapters = Some(adapters);
        self.resolver = Some(resolver);
        self
    }

    /// The adapter registry, when the caller made one available.
    #[must_use]
    pub fn adapters(&self) -> Option<&ono_adapter::Registry> {
        self.adapters.as_deref()
    }

    /// The `PATH` resolver, when the caller made one available.
    #[must_use]
    pub fn resolver(&self) -> Option<&Resolver> {
        self.resolver.as_ref()
    }

    /// The context frames in force, innermost last (spec §14.1).
    #[must_use]
    pub fn with_context(mut self, context: Vec<ContextFrame>) -> Self {
        self.context = context;
        self
    }

    /// Connects the stage's input stream.
    #[must_use]
    pub fn with_input(mut self, input: ValueStream) -> Self {
        self.input = Some(input);
        self
    }

    /// Joins the invocation to the pipeline's cancellation scope.
    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    /// Gives the command the shell bindings its expressions can see.
    ///
    /// `$name`, `@-1` and `@3` come from the session, and a command must not reach for a global to
    /// find them: the scope is passed in, so what an expression can see is exactly what the caller
    /// handed over (spec §6.4, §10.3).
    #[must_use]
    pub fn with_scope(mut self, scope: Arc<Scope>) -> Self {
        self.scope = scope;
        self
    }

    /// The contract the implementation was registered against.
    #[must_use]
    pub fn contract(&self) -> &'a CommandContract {
        self.contract
    }

    /// The resolved selectors and options.
    #[must_use]
    pub fn arguments(&self) -> &'a BoundArguments {
        self.arguments
    }

    /// The providers the command may ask.
    #[must_use]
    pub fn providers(&self) -> &'a ProviderRegistry {
        self.providers
    }

    /// Whether a stream is still waiting to be consumed.
    #[must_use]
    pub fn has_input(&self) -> bool {
        self.input.is_some()
    }

    /// Takes the input stream. A stream is consumed once: it is moved into the implementation,
    /// never cloned, so backpressure stays end to end.
    pub fn take_input(&mut self) -> Option<ValueStream> {
        self.input.take()
    }

    /// The cancellation token of the pipeline this stage belongs to.
    #[must_use]
    pub fn cancel_token(&self) -> &CancelToken {
        &self.cancel
    }

    /// The shell bindings the command's expressions can see.
    #[must_use]
    pub fn scope(&self) -> &Arc<Scope> {
        &self.scope
    }

    /// The context frames in force, outermost first.
    #[must_use]
    pub fn context(&self) -> &[ContextFrame] {
        &self.context
    }

    /// The same invocation over `arguments` instead of the ones it was built with.
    ///
    /// The input stream moves — a stream is consumed once — and everything else is shared, so
    /// the implementation that runs sees exactly the pipeline it belonged to, with the arguments
    /// the context filled in (ADR-0076).
    fn rebind<'b>(&mut self, arguments: &'b BoundArguments) -> Invocation<'b>
    where
        'a: 'b,
    {
        Invocation {
            contract: self.contract,
            arguments,
            providers: self.providers,
            input: self.input.take(),
            cancel: self.cancel.clone(),
            scope: Arc::clone(&self.scope),
            context: self.context.clone(),
            adapters: self.adapters.clone(),
            resolver: self.resolver.clone(),
            displays: self.displays,
        }
    }
}

/// The result of a command that had to await something, boxed so the trait stays object-safe.
pub type OutcomeFuture<'a> = Pin<Box<dyn Future<Output = Result<Outcome, ErrorValue>> + Send + 'a>>;

/// A native implementation of one registry command.
///
/// Spec §27.2: native code registers an implementation against a stable command id. The contract
/// is data and lives in `docs/spec/commands/`; this is the code behind it.
pub trait CommandImpl: Send + Sync {
    /// The registry id this implements, such as `ono.process.get`.
    fn id(&self) -> &str;

    /// Runs the command.
    ///
    /// Most commands answer here. A producer hands back the provider's stream and a transform
    /// hands back a stage over its input, so neither has to await anything: the awaiting happens
    /// inside the stream, where backpressure and cancellation already live (ADR-0013).
    ///
    /// # Errors
    ///
    /// A structured error when the command cannot run at all. A per-object failure belongs in an
    /// [`ActionOutcome`] or on the stream's error channel, so the objects that did work still
    /// arrive (spec §16.5).
    fn invoke(&self, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue>;

    /// Runs the command, awaiting whatever it has to await.
    ///
    /// **This is the entry point a host should call**, through [`CommandTable::run`]. The default
    /// answers with [`CommandImpl::invoke`], which is what every command that never awaits does.
    ///
    /// A mutation overrides it, because [`Provider::act`](ono_provider_api::Provider::act) and
    /// [`Provider::resolve`](ono_provider_api::Provider::resolve) are asynchronous and a mutation
    /// has to hold one [`ActionOutcome`] per target before it can answer (spec §11.5, §16.5). Its
    /// synchronous `invoke` therefore reports that it must be awaited rather than guessing.
    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(std::future::ready(self.invoke(ctx)))
    }
}

/// The error a command raises when it was run through the synchronous entry point but has to
/// reach a provider to answer.
pub fn must_be_awaited(spelling: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderUnsupported,
        format!("`{spelling}` reaches a provider, and reaching one is asynchronous"),
    )
    .with_help("run it through `CommandTable::run`, which awaits what has to be awaited")
}

/// The implementations this shell has, by command id.
#[derive(Clone, Default)]
pub struct CommandTable {
    implementations: BTreeMap<String, Arc<dyn CommandImpl>>,
}

impl CommandTable {
    /// An empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an implementation against the id it declares.
    ///
    /// A second implementation of the same id replaces the first, which is how a KUANG/11
    /// package or a test host substitutes one (spec §31.22).
    pub fn register(&mut self, implementation: Arc<dyn CommandImpl>) {
        let id = implementation.id().to_owned();
        self.implementations.insert(id, implementation);
    }

    /// The implementation of one id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Arc<dyn CommandImpl>> {
        self.implementations.get(id)
    }

    /// Whether an id has an implementation.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.implementations.contains_key(id)
    }

    /// Every implemented id, sorted.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.implementations.keys().map(String::as_str)
    }

    /// How many implementations are registered.
    #[must_use]
    pub fn len(&self) -> usize {
        self.implementations.len()
    }

    /// Whether nothing is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.implementations.is_empty()
    }

    /// Runs the implementation of `id`, awaiting whatever it has to await.
    ///
    /// This is the one call a host needs: it is correct for a producer, a transform and a
    /// mutation alike, because it goes through [`CommandImpl::invoke_async`].
    ///
    /// # Errors
    ///
    /// `resolve.command_not_found` when no implementation is registered for `id` — which is what
    /// a command whose spec §37 phase this build does not deliver looks like — and whatever the
    /// implementation itself reports.
    pub async fn run(&self, id: &str, ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        let implementation = self.get(id).cloned().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveCommandNotFound,
                format!("`{id}` is declared but this build implements nothing for it"),
            )
            .with_help("`help` lists what this shell can do; the rest is scheduled, not hidden")
        })?;
        // Spec §14.3: the context frames fill in the arguments the user did not type. That
        // happens here, at the one seam every implementation runs through, so a producer, a
        // trace, a watch and a mutation all see the same narrowed arguments (ADR-0076).
        let narrowed = crate::narrow::narrow(
            ctx.contract(),
            ctx.providers(),
            ctx.context(),
            ctx.arguments(),
        )?;
        match narrowed {
            Some(arguments) => {
                let mut inner = ctx.rebind(&arguments);
                implementation.invoke_async(&mut inner).await
            }
            None => implementation.invoke_async(ctx).await,
        }
    }
}

impl std::fmt::Debug for CommandTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandTable")
            .field("ids", &self.implementations.keys().collect::<Vec<_>>())
            .finish()
    }
}

/// The stable commands of `registry` that `table` has no implementation for.
///
/// Spec §27.2 asks CI to verify that every stable registry command has an implementation; this is
/// the list it fails on. Experimental and planned commands are not a compatibility promise and
/// are not reported, and a caller that also wants to ignore commands whose spec §37 phase has not
/// been reached filters the result by [`CommandContract::phase`].
#[must_use]
pub fn unbound_stable_commands<'a>(
    registry: &'a CommandRegistry,
    table: &CommandTable,
) -> Vec<&'a str> {
    registry.unbound_stable_ids(|id| table.contains(id))
}
