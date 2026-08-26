//! Binding a native implementation to a registry id, and running it (spec §27.2).

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{CancelToken, ValueStream};
use ono_provider_api::{ActionOutcome, ProviderRegistry};
use ono_value::ErrorValue;

use crate::bind::BoundArguments;
use crate::contract::CommandContract;
use crate::expr::Scope;
use crate::registry::CommandRegistry;

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
#[derive(Debug)]
pub struct Invocation<'a> {
    contract: &'a CommandContract,
    arguments: &'a BoundArguments,
    providers: &'a ProviderRegistry,
    input: Option<ValueStream>,
    cancel: CancelToken,
    scope: Arc<Scope>,
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
        }
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
}

/// The result of a command that had to await something, boxed so the trait stays object-safe.
pub type OutcomeFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Outcome, ErrorValue>> + Send + 'a>>;

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
pub(crate) fn must_be_awaited(spelling: &str) -> ErrorValue {
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
        implementation.invoke_async(ctx).await
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
