//! The state one shell session carries: where it is, what it knows, and what it is running.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use ono_core::ExitStatus;
use ono_pipeline::Budget;
use ono_process::Executor;
use ono_provider_api::ProviderRegistry;
use ono_value::{ErrorValue, Value};

/// What the evaluator is allowed to do.
///
/// Config mode is the restricted evaluation of ADR-0010: reading `config.ono` must not execute a
/// program, reach the network or load a plugin, so the restriction lives in the context rather
/// than in each command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Ordinary evaluation.
    Normal,
    /// Reading a configuration file (ADR-0010).
    Config,
}

/// One lexical scope's bindings.
type Scope = BTreeMap<String, Value>;

/// A function the user declared with `fn` (spec §19.3, ADR-0070).
#[derive(Debug)]
pub struct Function {
    /// The declaration, whose spans index `source`.
    pub declaration: ono_parser::FnDecl,
    /// The whole source the declaration was read from, so its body can be run later.
    pub source: std::sync::Arc<str>,
}

/// An alias the user declared with `alias` (spec §6.5, ADR-0070).
#[derive(Debug)]
pub struct Alias {
    /// The pipeline text the alias stands for, exactly as written after the `=`.
    pub expansion: String,
}

/// What a name can be defined as, beside the values `let` binds.
#[derive(Debug, Clone)]
pub enum Definition {
    /// A user function, resolution step 2 (ADR-0011).
    Function(std::sync::Arc<Function>),
    /// An alias, resolution step 3.
    Alias(std::sync::Arc<Alias>),
}

/// How long a link's agent is given to notice the hang-up before it is signalled, and again
/// before it is killed (ADR-0161).
///
/// Closing the agent's input ends it in about a millisecond, so this bound is never reached in
/// practice; it exists so that a far end which ignores end of input cannot make the shell wait
/// for it forever.
pub(crate) const AGENT_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// The deepest `@-N` the language admits, whatever the configured retention is.
///
/// v0.4.1 §55.1 makes the retained count configurable through `limits.history_results`, so the
/// number of slots is no longer a constant. This one bounds how far a *reference* may reach when
/// the scope is built, which has to be a compile-time figure and is the largest the setting's
/// range permits nothing beyond: a `@-N` past what is retained answers nothing, as it always did.
pub const DEEPEST_REFERENCE: u32 = 4096;

/// Everything a running shell knows.
/// Everything a running shell knows.
///
/// v0.4.1 §31.1: the shell is not stateless and does not try to be; the goal is that the
/// categories of state are explicit. §31.2's eight groups are the fields below, each owning the
/// invariants for its own data (§31.3), and the methods stay on `Session` so no caller has to
/// know the split.
pub struct Session {
    environment: EnvironmentState,
    scope: ScopeState,
    execution: ExecutionState,
    navigation: NavigationState,
    history: ResultHistoryState,
    jobs: JobState,
    provider: ProviderState,
    presentation: PresentationState,
}

/// Where the session is, and what it hands to a child process.
///
/// v0.4.1 §31.3: this group owns the invariant that binds `cwd` and `PWD` together, and the one
/// that tells an inherited binding from a bound one — `set env` writes here and `get env` reads
/// what was written, through the provider below.
struct EnvironmentState {
    cwd: PathBuf,
    env: BTreeMap<OsString, OsString>,
    /// The environment as the process that started the shell handed it over — what tells an
    /// `inherited` binding from one the session bound (`ono.env-var/1`'s `source`).
    inherited_env: BTreeMap<OsString, OsString>,
    /// The `env` provider registered in `providers()`, handed the session's bindings before
    /// each pipeline runs so `get env` sees what `set env` bound.
    env_provider: std::sync::Arc<ono_provider_linux::EnvProvider>,
}

/// What a name means here: values, functions, aliases, and the expansion in flight.
///
/// Three stacks that are pushed and popped together, which is the invariant this group owns: a
/// scope's bindings and its definitions have the same lifetime, and an alias being expanded is
/// never expanded again (ADR-0011 step 3, ADR-0070).
struct ScopeState {
    scopes: Vec<Scope>,
    /// Functions and aliases, one map per lexical scope, innermost last (ADR-0070).
    definitions: Vec<BTreeMap<String, Definition>>,
    /// The aliases being expanded right now, so an expansion is never expanded again
    /// (ADR-0011 step 3, ADR-0070).
    expanding: Vec<String>,
}

/// What the session is running, under which rules, and what it has captured.
///
/// The capture stack and the budget that bounds it are one thing and live together: v0.4.1
/// §23.1 forbids the buffer being "an invisible unlimited vector", and it is not one because
/// every value pushed is charged to the `Budget` beside it (§23.4, ADR-0453, ADR-0457).
struct ExecutionState {
    status: ExitStatus,
    executor: Executor,
    mode: Mode,
    interactive: bool,
    /// Set by `exit`, so the evaluator can unwind without unwinding the process.
    leaving: Option<ExitStatus>,
    /// Built on first use. A shell that runs `echo hi` should not have paid for a thread pool to
    /// do it, and spec §34's cold-start budget is measured on exactly that command.
    runtime: Option<tokio::runtime::Runtime>,
    /// Sub-pipelines being captured as values, innermost last: while one is open, a finished
    /// native pipeline hands its values here instead of to the terminal (ADR-0072 §4).
    captures: Vec<Vec<Value>>,
    /// What every capture inside the current shell command may retain together (v0.4.1 §23.4).
    capture_budget: Budget,
}

/// Where the session has gone: the context stack, the links it stands on, what it selected.
///
/// Appendix I.3 protects the trail's semantics, and they are the ones this group holds: a frame
/// remembers what leaving it restores, a link's frames go when the link does, and bare `@` is
/// whatever the last interactive view left selected.
struct NavigationState {
    /// The context stack of spec §14.1, above the implicit ground frame. Each entry pairs the
    /// frame every command sees with what popping it must restore.
    frames: Vec<ShellFrame>,
    /// The remote links this session holds (spec §21.1), by the name the user gave them.
    links: Vec<SessionLink>,
    /// What the last interactive view left selected — the referent of bare `@` (spec §6.4,
    /// ADR-0033, ADR-0050).
    selection: Option<Value>,
}

/// The retained results of spec §20.2, and the ceilings v0.4.1 §24.1 bounds them by.
///
/// v0.4.1 §31.3 names this group by name: "result-history byte-budget enforcement belongs in
/// `ResultHistoryState`, not scattered across evaluator call sites". It is not scattered —
/// [`Session::retain`] is the one door, it applies the configured limits before it retains, and
/// `ono_history` enforces all four dimensions inside. No evaluator call site decides what fits.
struct ResultHistoryState {
    /// Recent structured results, newest last (spec §20.2). Bounded, so a long session cannot
    /// hold every table it ever printed.
    results: ono_history::ResultHistory,
}

/// Background work, and the tables that make it answerable.
///
/// Appendix I.3 protects job reaping, which is this group's invariant: a job leaves the table
/// when it is taken, the detach times are keyed by the same job number the executor issued, and
/// what `get job` answers is published from here before every pipeline (ADR-0090).
struct JobState {
    /// Backgrounded native pipelines (spec §18.4, ADR-0024): jobs in the same table as external
    /// commands, numbered from the executor's own sequence.
    native_jobs: Vec<NativeJob>,
    /// When each external job was detached, by job number; the executor's table does not
    /// record it, and `ono.job/1` requires it.
    job_started: BTreeMap<u32, Value>,
    /// The tables the session publishes for `ono.shell` to answer from — the job table today
    /// (spec §18.4, ADR-0090). Shared with the provider registered in `providers()`.
    tables: std::sync::Arc<std::sync::Mutex<crate::session_provider::SessionTables>>,
}

/// What answers a question: the provider registry, and the external command adapters.
///
/// Both are built on first use and neither is state a session could be reconstructed from —
/// v0.4.1 §31.4: segmentation "MUST not accidentally turn ephemeral handles, runtimes or jobs
/// into serializable state", and nothing here is serializable.
struct ProviderState {
    /// Built on first use, for the same reason: constructing it opens sockets and speaks D-Bus.
    providers: Option<ProviderRegistry>,
    /// The external command adapters (spec v0.3 §1.24), built on first use: the registry holds
    /// the version probe cache, which is per session by design (§1.46).
    adapters: Option<std::sync::Arc<ono_adapter::Registry>>,
    /// The adapters that shaped the statement being run, with the argv each one planned —
    /// what history records about it (spec v0.3 §1.62).
    adaptations: Vec<(String, String)>,
}

/// How the session behaves and how it looks.
///
/// The layered configuration of ADR-0010 with the provenance of every value (ADR-0094), and the
/// theme resolved once it has been read. §31.2 names no configuration group; the settings live
/// beside the theme because that is what a session's declaration of itself is read for, and
/// Appendix I.3's config precedence is `Settings`' own invariant, unchanged by where it sits.
struct PresentationState {
    /// The layered configuration of ADR-0010, with the provenance of every value (ADR-0094).
    settings: crate::settings::Settings,
    /// The theme every renderer paints with, resolved once configuration has been read
    /// (spec §44, §30; ADR-0332).
    theme: std::sync::Arc<ono_render::Theme>,
}

/// One remote link the session knows: a definition, established or not (spec §21.1, ADR-0103).
#[derive(Debug)]
pub struct SessionLink {
    /// The link's name, as the user gave it: the prompt's spelling, `enter link`'s argument.
    pub name: String,
    /// The host the link points at — the name itself for `link host`, whatever `--host` said
    /// for a definition.
    pub host: String,
    /// How the bytes travel, for `get link`.
    pub transport: String,
    /// Whether the agentless fallback of spec §21.3 was asked for.
    pub agentless: bool,
    /// Whether the link outlives its frame: `link host` and `add link` persist, `connect host`
    /// is one-shot and goes when its frame is left (ADR-0104).
    pub persistent: bool,
    /// The connection, once the handshake succeeded.
    pub connection: Option<LinkConnection>,
}

impl SessionLink {
    /// The row `get link` shows for this link (ADR-0090 §3).
    #[must_use]
    pub fn row(&self) -> crate::session_provider::LinkRow {
        let connection = self.connection.as_ref();
        crate::session_provider::LinkRow {
            name: self.name.clone(),
            host: self.host.clone(),
            transport: self.transport.clone(),
            // The mode a link *is* in, not the one it was asked for: a link that fell back
            // reports the fallback, and a definition that was never established reports what it
            // would be made in (spec §21.3).
            agentless: connection.map_or(self.agentless, LinkConnection::is_agentless),
            state: if connection.is_some() {
                "connected"
            } else {
                "defined"
            },
            targets: connection.map_or_else(Vec::new, LinkConnection::targets),
            protocol: connection.and_then(LinkConnection::protocol_version),
            providers: connection.map(LinkConnection::provider_ids),
            transport_fingerprint: connection.and_then(LinkConnection::transport_fingerprint),
            transport_trust: connection.and_then(LinkConnection::transport_trust),
            runtime_user: connection.and_then(LinkConnection::runtime_user),
            runtime_uid: connection.and_then(LinkConnection::runtime_uid),
            runtime_elevated: connection.and_then(LinkConnection::runtime_elevated),
        }
    }
}

/// What answers on the far side of an established link.
///
/// Spec §21 has two: the agent of §21.4, which speaks the link protocol, and the agentless
/// fallback of §21.3, which is a reduced set of providers reading the far side with standard
/// commands. Everything above the registry is identical for both — that is the point — so the
/// difference lives here, where the shell describes a link rather than where it uses one.
pub enum FarEnd {
    /// The Ono agent of spec §21.4, reached over the link protocol.
    Agent(ono_remote::RemoteLink),
    /// The reduced provider set of spec §21.3: no agent on the far side.
    Agentless(ono_remote::AgentlessLink),
}

/// An established link: the connection, and the registry its providers are mounted in.
pub struct LinkConnection {
    /// What is answering over there, kept so dropping the session hangs up.
    pub far_end: FarEnd,
    /// The mounted registry an active link frame answers from (spec §14.4).
    pub registry: std::sync::Arc<ProviderRegistry>,
    /// The process serving this link, where the shell started one: `ono --agent` under the
    /// `local` transport, `ssh` under `ssh`. A link's agent is a resource of the link
    /// (spec §21.4), so the session that started it is the one that ends it — see
    /// [`Session::hang_up`].
    pub agent: Option<ono_remote::ChildProcess>,
}

impl LinkConnection {
    /// What the link's context can answer, in mount order.
    ///
    /// For an agent link that is what the handshake negotiated (spec §21.2). For a reduced link
    /// nothing was negotiated, so the honest content of the field is the set of targets the
    /// fallback can actually read — which is what makes the reduction visible in the table
    /// itself, beside the targets it refuses (spec §21.3).
    #[must_use]
    pub fn targets(&self) -> Vec<String> {
        match &self.far_end {
            FarEnd::Agent(_) => self
                .registry
                .providers()
                .iter()
                .flat_map(|provider| provider.targets().iter().map(|target| (*target).to_owned()))
                .collect(),
            FarEnd::Agentless(link) => link.answered_targets(),
        }
    }

    /// The agent behind this link, where the far side has one (spec §21.4).
    ///
    /// `None` for the agentless fallback of §21.3: there is nobody over there to speak the link
    /// protocol, which is precisely what makes that link reduced.
    #[must_use]
    pub const fn agent_link(&self) -> Option<&ono_remote::RemoteLink> {
        match &self.far_end {
            FarEnd::Agent(link) => Some(link),
            FarEnd::Agentless(_) => None,
        }
    }

    /// Whether the far side is the reduced set of spec §21.3 rather than the agent of §21.4.
    #[must_use]
    pub const fn is_agentless(&self) -> bool {
        matches!(self.far_end, FarEnd::Agentless(_))
    }

    /// The link protocol version the handshake settled on, or `None` when no handshake happened.
    #[must_use]
    pub fn protocol_version(&self) -> Option<u16> {
        match &self.far_end {
            FarEnd::Agent(link) => Some(link.negotiated().version()),
            FarEnd::Agentless(_) => None,
        }
    }

    /// How the far side named itself: `ono/<version>` for the agent of spec §21.4, and the
    /// reduced set naming itself and what `uname` said for the fallback of §21.3.
    #[must_use]
    pub fn far_end_name(&self) -> String {
        match &self.far_end {
            FarEnd::Agent(link) => link.negotiated().peer().agent().to_owned(),
            FarEnd::Agentless(link) => match link.system() {
                Some(system) => format!("agentless ({system})"),
                None => "agentless".to_owned(),
            },
        }
    }

    /// The fingerprint of the key the far side proved it holds during this link's handshake
    /// (v0.4.1 §7.3).
    ///
    /// `None` where the transport authenticated nobody to this process. That is the honest answer
    /// for `ssh`, where OpenSSH verified the host in its own `known_hosts` and will not say which
    /// key it accepted (§4.3, ADR-0037 §4), and for `local`, where the far side is this shell's
    /// own child — and it stays `None` rather than borrowing somebody else's verification.
    #[must_use]
    pub fn transport_fingerprint(&self) -> Option<String> {
        self.agent_link().and_then(|link| {
            link.negotiated()
                .fingerprint()
                .map(|fingerprint| fingerprint.to_string())
        })
    }

    /// What the trust store concluded about that key, as `ono.link/1` spells it.
    #[must_use]
    pub fn transport_trust(&self) -> Option<&'static str> {
        self.agent_link()
            .map(|link| match link.negotiated().trust() {
                ono_protocol::TrustDecision::Pinned => "pinned",
                ono_protocol::TrustDecision::NewlyPinned => "newly_pinned",
                ono_protocol::TrustDecision::Unauthenticated => "unauthenticated",
            })
    }

    /// The user the far side reports it runs as (v0.4.1 §7.3).
    ///
    /// Kept apart from [`transport_fingerprint`](Self::transport_fingerprint) on purpose: this is
    /// what the peer said about itself, and §2.1 forbids a self-reported field from satisfying
    /// the word authenticated.
    #[must_use]
    pub fn runtime_user(&self) -> Option<String> {
        self.agent_link()
            .map(|link| link.negotiated().peer().identity().user().to_owned())
    }

    /// The numeric user id the far side reports, where it reports one.
    #[must_use]
    pub fn runtime_uid(&self) -> Option<u32> {
        self.agent_link()
            .and_then(|link| link.negotiated().peer().identity().uid())
    }

    /// Whether the far side reports it is elevated.
    #[must_use]
    pub fn runtime_elevated(&self) -> Option<bool> {
        self.agent_link()
            .map(|link| link.negotiated().peer().identity().is_elevated())
    }

    /// The ids of the providers the far side offers (spec §21.2).
    #[must_use]
    pub fn provider_ids(&self) -> Vec<String> {
        match &self.far_end {
            FarEnd::Agent(link) => link
                .negotiated()
                .providers()
                .iter()
                .map(|descriptor| descriptor.id().to_owned())
                .collect(),
            FarEnd::Agentless(_) => vec![ono_remote::AGENTLESS_PROVIDER.to_owned()],
        }
    }

    /// Says goodbye to the far side, where there is somebody to say it to.
    pub fn hangup(&self) {
        match &self.far_end {
            FarEnd::Agent(link) => link.hangup(),
            // A reduced link holds nothing open: each query is one command that has already
            // ended by the time its records arrive.
            FarEnd::Agentless(_) => {}
        }
    }
}

impl std::fmt::Debug for LinkConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let host = match &self.far_end {
            FarEnd::Agent(link) => link.host(),
            FarEnd::Agentless(link) => link.host(),
        };
        f.debug_struct("LinkConnection")
            .field("host", &host)
            .field("agentless", &self.is_agentless())
            .finish_non_exhaustive()
    }
}

/// One backgrounded native pipeline.
#[derive(Debug)]
pub struct NativeJob {
    /// The number the user addresses it by, reserved from the executor's sequence.
    pub number: u32,
    /// The pipeline as it was typed.
    pub command: String,
    /// Rows keyed by identity — what a live view folds events into, and what `fg` repaints.
    pub model: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, Value>>>,
    /// Values a bounded pipeline produced, delivered when the job is foregrounded or reaped.
    pub values: std::sync::Arc<std::sync::Mutex<Vec<Value>>>,
    /// The failures the stream reported.
    pub failures: std::sync::Arc<std::sync::Mutex<Vec<ono_value::ErrorValue>>>,
    /// When the pipeline was detached.
    pub started: Value,
    /// The task driving the stream; aborting it drops every receiver, which stops the producers.
    pub handle: tokio::task::JoinHandle<()>,
}

/// One pushed frame, with the shell-side state `leave` restores.
#[derive(Debug, Clone)]
pub struct ShellFrame {
    /// The frame as commands see it (spec §14.3).
    pub frame: ono_command::ContextFrame,
    /// Where the session stood before a filesystem frame moved it (spec §14.2).
    pub restore_cwd: Option<PathBuf>,
}

impl ShellFrame {
    /// Whether this frame stands on the link named `name` (spec §14.4).
    #[must_use]
    pub fn is_link(&self, name: &str) -> bool {
        matches!(self.frame.kind(), ono_command::FrameKind::Link)
            && self.frame.identity().to_string() == name
    }
}

impl EnvironmentState {
    /// Moves the session to `directory`, or leaves it where it was.
    ///
    /// The process moves with it, and that pairing is this group's invariant (§31.3): a session
    /// `cwd` the process did not follow would leave every command that resolves a relative path
    /// through the kernel answering about wherever the shell happened to start. A kernel that
    /// refuses the move — the directory went away between the caller's check and here — leaves
    /// both where they were rather than splitting them.
    fn set_cwd(&mut self, directory: PathBuf) {
        if std::env::set_current_dir(&directory).is_err() {
            return;
        }
        self.cwd = directory;
    }
}

impl ExecutionState {
    /// Opens a capture buffer for the next finished pipeline.
    fn begin_capture(&mut self) {
        self.captures.push(Vec::new());
    }

    /// Ends the innermost capture and answers what it collected.
    fn end_capture(&mut self) -> Vec<Value> {
        self.captures.pop().unwrap_or_default()
    }

    /// Whether a pipeline's result is being captured rather than shown.
    fn capturing(&self) -> bool {
        !self.captures.is_empty()
    }

    /// Starts one shell command's capture accounting afresh, at `bytes` (v0.4.1 §23.4).
    fn begin_command_captures(&mut self, bytes: u64) {
        self.capture_budget = Budget::of(
            "this command's captures",
            ono_pipeline::COMMAND_CAPTURE_MAX_ITEMS,
            bytes,
        )
        .for_settings("limits.materialize_items", "limits.command_capture_bytes");
    }

    /// Hands finished values to the innermost capture, charging every one of them.
    ///
    /// The invariant this group owns (§31.3): a value is in a capture buffer only if it was
    /// charged to the budget beside it, so §23.1's "invisible unlimited vector" cannot exist
    /// here however deeply captures nest (§23.4, ADR-0453, ADR-0457).
    fn capture(&mut self, values: &[Value]) -> Result<bool, ErrorValue> {
        if self.captures.is_empty() {
            return Ok(false);
        }
        for value in values {
            self.capture_budget
                .charge(value)
                .map_err(ono_pipeline::Exceeded::into_error)?;
            if let Some(capture) = self.captures.last_mut() {
                capture.push(value.clone());
            }
        }
        Ok(true)
    }
}

impl ResultHistoryState {
    /// Retains a finished pipeline's values, answering what was kept and what was not.
    ///
    /// v0.4.1 §31.3 asks for exactly this: the byte-budget enforcement is here rather than at the
    /// evaluator call sites, so there is one place that decides what fits. The ceilings are read
    /// on every retention rather than only at startup, so a `set config limits.history_…` at the
    /// prompt takes effect on the next result — the same way the materialization limits are read
    /// per pipeline and the capture ceiling per command.
    fn retain(
        &mut self,
        values: &[Value],
        settings: &crate::settings::Settings,
    ) -> ono_history::Retained {
        self.apply_limits(settings);
        // Spec §20.2: "Retention policy must protect secrets". The policy that keeps a secret
        // out of history is the one that keeps it out of what `@-1` replays, or the shell would
        // redact the command that read a token and keep the token (spec §17.5, ADR-0262). It runs
        // inside retention, so only what is kept pays for it.
        let policy = redaction_policy();
        self.results.retain_mapped(values, |value| {
            value.map_text(&|text| {
                let redacted = policy.redact(text);
                (redacted != text).then(|| std::sync::Arc::from(redacted.as_str()))
            })
        })
    }

    /// Narrows the four retention dimensions to what the settings declare (v0.4.1 §24.1, §55.1).
    fn apply_limits(&mut self, settings: &crate::settings::Settings) {
        let limits = crate::limits::HistoryLimits::of(settings);
        self.results.set_limits(ono_history::RetentionLimits {
            results: limits.results,
            items_per_result: limits.items_per_result,
            bytes_per_result: limits.bytes_per_result,
            bytes_total: limits.bytes_total,
        });
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("cwd", &self.environment.cwd)
            .field("status", &self.execution.status)
            .field("mode", &self.execution.mode)
            .field("interactive", &self.execution.interactive)
            .finish_non_exhaustive()
    }
}

impl Session {
    /// A session inheriting this process's environment and working directory.
    ///
    /// A working directory that no longer exists is not fatal: the session starts at the root,
    /// because a shell that cannot start in a deleted directory is a shell that cannot be used to
    /// leave one.
    #[must_use]
    pub fn new(interactive: bool) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let mut env: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
        // v0.4 §30.4: "`PWD` remains the filesystem working directory." A shell started from a
        // parent that never updated it would hand every external command a `PWD` that is not
        // where they run, so the session states its own the moment it has one.
        env.insert(OsString::from("PWD"), cwd.as_os_str().to_owned());
        let env_provider = std::sync::Arc::new(ono_provider_linux::EnvProvider::new(
            env.iter().map(|(name, value)| {
                ono_provider_linux::EnvBinding::inherited(
                    name.to_string_lossy(),
                    value.to_string_lossy(),
                )
            }),
        ));
        // The executor attaches to the controlling terminal whenever there is one, whether or not
        // the session is interactive. `ono -c 'less file'` typed at a terminal must hand that
        // terminal to `less`, exactly as `bash -c` does — a child that is never given the
        // terminal is stopped by SIGTTOU the moment it tries to configure it (spec §18.1,
        // ADR-0013). With no controlling terminal there is nothing to hand over and the executor
        // detaches.
        let executor = Executor::new().unwrap_or_else(|_| Executor::detached());
        Self {
            environment: EnvironmentState {
                cwd,
                inherited_env: env.clone(),
                env,
                env_provider,
            },
            scope: ScopeState {
                scopes: vec![Scope::new()],
                definitions: vec![BTreeMap::new()],
                expanding: Vec::new(),
            },
            execution: ExecutionState {
                status: ExitStatus::SUCCESS,
                executor,
                mode: Mode::Normal,
                interactive,
                leaving: None,
                runtime: None,
                captures: Vec::new(),
                capture_budget: Budget::command_captures(),
            },
            navigation: NavigationState {
                frames: Vec::new(),
                links: Vec::new(),
                selection: None,
            },
            history: ResultHistoryState {
                results: ono_history::ResultHistory::new(ono_history::RetentionLimits::default()),
            },
            jobs: JobState {
                native_jobs: Vec::new(),
                job_started: BTreeMap::new(),
                tables: std::sync::Arc::default(),
            },
            provider: ProviderState {
                providers: None,
                adapters: None,
                adaptations: Vec::new(),
            },
            presentation: PresentationState {
                settings: crate::settings::Settings::new(),
                theme: std::sync::Arc::new(ono_render::Theme::default()),
            },
        }
    }

    /// The configuration settings, with the layer that set each one (spec §30).
    #[must_use]
    pub fn settings(&self) -> &crate::settings::Settings {
        &self.presentation.settings
    }

    /// The configuration settings, for a layer that sets one.
    pub fn settings_mut(&mut self) -> &mut crate::settings::Settings {
        &mut self.presentation.settings
    }

    /// The theme every renderer paints with (spec §44).
    #[must_use]
    pub fn theme(&self) -> &std::sync::Arc<ono_render::Theme> {
        &self.presentation.theme
    }

    /// Replaces the theme, which `config::load` does once the settings are in (ADR-0332).
    pub fn set_theme(&mut self, theme: ono_render::Theme) {
        self.presentation.theme = std::sync::Arc::new(theme);
    }

    /// Defines `name` as a function or an alias in the innermost scope (ADR-0070).
    pub fn define(&mut self, name: impl Into<String>, definition: Definition) {
        if let Some(scope) = self.scope.definitions.last_mut() {
            scope.insert(name.into(), definition);
        }
    }

    /// The user function `name`, from the innermost scope that defines one.
    #[must_use]
    pub fn function(&self, name: &str) -> Option<std::sync::Arc<Function>> {
        self.scope
            .definitions
            .iter()
            .rev()
            .find_map(|scope| match scope.get(name) {
                Some(Definition::Function(function)) => Some(std::sync::Arc::clone(function)),
                _ => None,
            })
    }

    /// The alias `name`, from the innermost scope that defines one — unless it is being
    /// expanded right now, in which case it is not an alias for the head it produced.
    #[must_use]
    pub fn alias(&self, name: &str) -> Option<std::sync::Arc<Alias>> {
        if self
            .scope
            .expanding
            .iter()
            .any(|expanding| expanding == name)
        {
            return None;
        }
        self.scope
            .definitions
            .iter()
            .rev()
            .find_map(|scope| match scope.get(name) {
                Some(Definition::Alias(alias)) => Some(std::sync::Arc::clone(alias)),
                _ => None,
            })
    }

    /// Marks `name` as being expanded, until [`Session::finish_expanding`].
    pub fn begin_expanding(&mut self, name: impl Into<String>) {
        self.scope.expanding.push(name.into());
    }

    /// Ends the innermost alias expansion.
    pub fn finish_expanding(&mut self) {
        self.scope.expanding.pop();
    }

    /// The async runtime native pipelines run on, built the first time one is needed.
    ///
    /// Returns `None` only if the operating system refuses to start it, which a caller reports as
    /// a structured error rather than treating as impossible.
    pub fn runtime(&mut self) -> Option<&tokio::runtime::Runtime> {
        if self.execution.runtime.is_none() {
            self.execution.runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("ono")
                .build()
                .ok();
        }
        self.execution.runtime.as_ref()
    }

    /// A handle to the runtime, cloneable and borrow-free, once it exists.
    #[must_use]
    pub fn runtime_handle(&self) -> Option<tokio::runtime::Handle> {
        self.execution
            .runtime
            .as_ref()
            .map(tokio::runtime::Runtime::handle)
            .cloned()
    }

    /// The tables the session shares with `ono.shell` — the job table and the KUANG/11 host
    /// (ADR-0090, ADR-0107).
    #[must_use]
    pub fn tables(
        &self,
    ) -> &std::sync::Arc<std::sync::Mutex<crate::session_provider::SessionTables>> {
        &self.jobs.tables
    }

    /// Runs `body` over the KUANG/11 host, locked for that one operation.
    pub fn with_kuang<T>(&self, body: impl FnOnce(&mut crate::kuang_host::Host) -> T) -> T {
        let mut tables = self
            .jobs
            .tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        body(&mut tables.kuang)
    }

    /// Tells the host where this session's plugin home and state directory are, so the tables
    /// it answers from follow the environment (spec §31.9, §31.31).
    pub fn publish_host(&mut self) {
        let plugin_path = crate::plugins::plugin_path(self);
        let state_dir = crate::config::state_dir(self);
        let config_dir = crate::config::user_config_dir(self);
        // The machine-wide trust store is the administrator's, so it comes from the environment
        // and not from the user's configuration directory (ADR-0312).
        let system_trust = crate::kuang_trust::system_path(self.env_var("ONO_KUANG_SYSTEM_TRUST"));
        self.with_kuang(|host| {
            host.configure(plugin_path, state_dir, config_dir, system_trust);
            // Spec §31.37: the trail outlives the process. Appending at the start of every
            // pipeline keeps a session that is killed from losing everything before it.
            host.persist_audit();
        });
    }

    /// Keeps a loaded KUANG/11 package on the session (spec §31.10), answering the instance it
    /// replaces so the caller can shut it down.
    pub fn add_plugin(
        &mut self,
        id: String,
        plugin: ono_kuang_supervisor::LoadedPlugin,
    ) -> Option<crate::kuang_host::Instance> {
        self.with_kuang(|host| host.add_instance(id, plugin))
    }

    /// A loaded package by its manifest id.
    #[must_use]
    pub fn plugin(&self, id: &str) -> Option<std::sync::Arc<ono_kuang_supervisor::LoadedPlugin>> {
        self.with_kuang(|host| host.plugin(id))
    }

    /// The ids of every loaded package.
    #[must_use]
    pub fn plugin_ids(&self) -> Vec<String> {
        self.with_kuang(|host| host.plugin_ids().map(str::to_owned).collect())
    }

    /// Adds a remote link to the session's table, ending whatever link held the name before it.
    pub fn add_link(&mut self, link: SessionLink) {
        while let Some(replaced) = self.remove_link(&link.name) {
            self.hang_up(replaced);
        }
        self.navigation.links.push(link);
    }

    /// The links this session holds, oldest first.
    #[must_use]
    pub fn links(&self) -> &[SessionLink] {
        &self.navigation.links
    }

    /// The named link, if the session knows it.
    #[must_use]
    pub fn link(&self, name: &str) -> Option<&SessionLink> {
        self.navigation.links.iter().find(|link| link.name == name)
    }

    /// The named link, to change its definition.
    pub fn link_mut(&mut self, name: &str) -> Option<&mut SessionLink> {
        self.navigation
            .links
            .iter_mut()
            .find(|link| link.name == name)
    }

    /// Forgets the named link and hands it back, so the caller decides when the connection
    /// drops — and with it, hangs up (ADR-0036 §8). A caller that is letting the link go for
    /// good passes it to [`hang_up`](Self::hang_up), which also ends the process serving it.
    pub fn remove_link(&mut self, name: &str) -> Option<SessionLink> {
        let index = self
            .navigation
            .links
            .iter()
            .position(|link| link.name == name)?;
        Some(self.navigation.links.remove(index))
    }

    /// Ends a link the session has let go of: it hangs up, and the process it started to serve
    /// the link is waited for (spec §21.4, §18.1, ADR-0161).
    ///
    /// A shell owns the processes it starts. `ono --agent` under the `local` transport, and the
    /// `ssh` that carries the agent under `ssh`, are started by `link host` and are therefore
    /// the shell's to end: hanging up closes their input, which is the ordinary way an agent
    /// loop finishes, and this waits for that to happen rather than leaving an orphan behind.
    pub fn hang_up(&self, link: SessionLink) {
        let Some(connection) = link.connection else {
            return;
        };
        // The hang-up is said explicitly rather than left to the drop, because a provider of
        // this link may still be mounted somewhere and would hold the connection open.
        connection.hangup();
        let agent = connection.agent.clone();
        drop(connection);
        let (Some(agent), Some(runtime)) = (agent, self.execution.runtime.as_ref()) else {
            return;
        };
        runtime.block_on(agent.end(AGENT_GRACE));
    }

    /// Ends every link the session still holds. Called when the session goes, so no agent it
    /// started can outlive the shell and reparent to init.
    fn hang_up_all(&mut self) {
        for link in std::mem::take(&mut self.navigation.links) {
            self.hang_up(link);
        }
    }

    /// How many frames on the stack stand on the named link.
    #[must_use]
    pub fn link_frames(&self, name: &str) -> usize {
        self.navigation
            .frames
            .iter()
            .filter(|frame| frame.is_link(name))
            .count()
    }

    /// Pops every frame standing on the named link, wherever it is in the stack, and answers
    /// how many went. Frames above it stay: an entered directory inside a link is still the
    /// directory (spec §14.1 nests frames; only the link's own are the link's).
    pub fn pop_link_frames(&mut self, name: &str) -> usize {
        let before = self.navigation.frames.len();
        self.navigation.frames.retain(|frame| !frame.is_link(name));
        before - self.navigation.frames.len()
    }

    /// The mounted registry of the named link, if the session holds it established.
    #[must_use]
    pub fn link_registry(&self, name: &str) -> Option<std::sync::Arc<ProviderRegistry>> {
        self.navigation
            .links
            .iter()
            .find(|link| link.name == name)
            .and_then(|link| link.connection.as_ref())
            .map(|held| std::sync::Arc::clone(&held.registry))
    }

    /// Where the host sources of spec §9.1 live for this session's environment (ADR-0103).
    #[must_use]
    pub fn host_sources(&self) -> crate::hosts::HostSources {
        let pairs: Vec<(String, String)> = self
            .environment
            .env
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.to_string_lossy().into_owned(),
                )
            })
            .collect();
        crate::hosts::HostSources::from_environment(
            pairs
                .iter()
                .map(|(name, value)| (name.as_str(), value.as_str())),
        )
    }

    /// Publishes the link table as it is now, for `get link` and `get host` (ADR-0103).
    pub fn publish_links(&mut self) {
        let rows: Vec<crate::session_provider::LinkRow> =
            self.navigation.links.iter().map(SessionLink::row).collect();
        // §19.1: the same table is what the local root's link map is built from, and it must
        // never quietly drop a link that is no longer connected (§35.2).
        crate::spatial::links::publish(&rows);
        self.jobs
            .tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publish_links(rows);
    }

    /// Keeps `value` as the interactive selection bare `@` refers to (ADR-0050).
    pub fn select(&mut self, value: Value) {
        self.navigation.selection = Some(value);
    }

    /// The interactive selection, if a view has set one.
    #[must_use]
    pub fn selection(&self) -> Option<&Value> {
        self.navigation.selection.as_ref()
    }

    /// Adds a backgrounded native pipeline to the job table.
    pub fn push_native_job(&mut self, job: NativeJob) {
        self.jobs.native_jobs.push(job);
    }

    /// The backgrounded native pipelines, oldest first.
    #[must_use]
    pub fn native_jobs(&self) -> &[NativeJob] {
        &self.jobs.native_jobs
    }

    /// Removes and answers native job `number`, releasing its number.
    pub fn take_native_job(&mut self, number: u32) -> Option<NativeJob> {
        let index = self
            .jobs
            .native_jobs
            .iter()
            .position(|job| job.number == number)?;
        let job = self.jobs.native_jobs.remove(index);
        self.execution.executor.release_job_number(number);
        Some(job)
    }

    /// Records that external job `number` was just detached, for `ono.job/1`'s `started`.
    pub fn note_job_started(&mut self, number: u32) {
        self.jobs
            .job_started
            .entry(number)
            .or_insert_with(Value::now);
    }

    /// Publishes the job table as it is now, for `get job` (spec §18.4, ADR-0090).
    ///
    /// Reaps first, so a job that finished since the last prompt is `done` here and not still
    /// `running`. Both halves of the table — the executor's process groups and the detached
    /// native pipelines — become rows of one list, in job-number order.
    pub fn publish_jobs(&mut self) {
        use crate::session_provider::JobRow;
        let _ = self.execution.executor.poll_jobs();
        let mut rows: Vec<JobRow> = Vec::new();
        for job in self.execution.executor.jobs() {
            let number = job.id.number();
            // A job that entered the table by being stopped rather than by `&` was never noted;
            // its first publication is the closest instant the shell has.
            let started = self
                .jobs
                .job_started
                .entry(number)
                .or_insert_with(Value::now)
                .clone();
            let (state, exit_status) = match job.state {
                ono_process::JobState::Running => ("running", None),
                ono_process::JobState::Stopped(_) => ("stopped", None),
                ono_process::JobState::Exited(status) => {
                    if job
                        .processes
                        .iter()
                        .any(|process| process.failure.is_some())
                    {
                        ("failed", None)
                    } else {
                        // A signal death has no exit status to report (job.v1): null, never
                        // 128 + n dressed up as one.
                        ("done", status.signal().is_none().then_some(status))
                    }
                }
            };
            rows.push(JobRow {
                number,
                kind: "external",
                state,
                command: job.command.clone(),
                process_group: Some(job.pgid),
                pids: Some(
                    job.processes
                        .iter()
                        .map(|process| process.pid)
                        .filter(|pid| *pid != 0)
                        .collect(),
                ),
                started,
                exit_status,
            });
        }
        for job in &self.jobs.native_jobs {
            let finished = job.handle.is_finished();
            let failed = finished
                && !job
                    .failures
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_empty();
            rows.push(JobRow {
                number: job.number,
                kind: "native",
                state: if finished { "done" } else { "running" },
                command: job.command.clone(),
                process_group: None,
                pids: None,
                started: job.started.clone(),
                exit_status: finished.then_some(if failed {
                    ExitStatus::FAILURE
                } else {
                    ExitStatus::SUCCESS
                }),
            });
        }
        rows.sort_by_key(|row| row.number);
        self.jobs
            .job_started
            .retain(|number, _| rows.iter().any(|row| row.number == *number));
        self.jobs
            .tables
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .publish_jobs(rows);
    }

    /// Retains a finished pipeline's values for `@-1` and `@N` (spec §6.4, §20.2), and answers
    /// how many values it could not keep.
    ///
    /// Retention is bounded in four dimensions by [`ono_history::RetentionLimits`], because spec
    /// §20.2 asks for *bounded* recent results and v0.4.1 §2.4 will not accept a count as a
    /// memory bound. The count of what was dropped is returned rather than swallowed: a caller
    /// shows it, so a later `@-1` that is short of what the screen held is never a surprise
    /// (ADR-0249, v0.4.1 §24.3).
    pub fn retain_result(&mut self, values: Vec<Value>) -> usize {
        self.retain(&values).dropped()
    }

    /// Retains a finished pipeline's values, answering what was kept and what was not.
    ///
    /// The values are borrowed and never edited: v0.4.1 §24.2 rule 1 is that *"the live pipeline
    /// result is never truncated merely to fit history"*, and a borrow is how that becomes a
    /// property of the type rather than a promise in a comment (§60.6).
    pub fn retain(&mut self, values: &[Value]) -> ono_history::Retained {
        self.history.retain(values, &self.presentation.settings)
    }

    /// The `n`th previous result, `1` for the most recent (spec §6.4 `@-1`).
    #[must_use]
    pub fn previous_result(&self, n: u32) -> Option<&[Value]> {
        self.history.results.previous(n)
    }

    /// What retaining the `n`th previous result did, so an inspection can say it is partial
    /// (v0.4.1 §24.3).
    #[must_use]
    pub fn previous_result_retention(&self, n: u32) -> Option<ono_history::Retained> {
        self.history.results.retention_of(n)
    }

    /// The retained result history, for a diagnostic that reports what it holds.
    #[must_use]
    pub const fn result_history(&self) -> &ono_history::ResultHistory {
        &self.history.results
    }

    /// Applies the configured retention limits to the result history (v0.4.1 §24.1, §55.1).
    ///
    /// Called once configuration has been read: the session exists before the config layers do,
    /// so it starts at Appendix A's defaults and narrows to the user's.
    pub fn apply_retention_limits(&mut self) {
        self.history.apply_limits(&self.presentation.settings);
    }

    /// The context stack above the ground frame, outermost first (spec §14.1).
    #[must_use]
    pub fn frames(&self) -> &[ShellFrame] {
        &self.navigation.frames
    }

    /// The frames as commands see them, for an [`ono_command::Invocation`].
    #[must_use]
    pub fn context(&self) -> Vec<ono_command::ContextFrame> {
        self.navigation
            .frames
            .iter()
            .map(|entry| entry.frame.clone())
            .collect()
    }

    /// Pushes a frame (spec §14.1: `enter` pushes).
    pub fn push_frame(&mut self, frame: ShellFrame) {
        self.navigation.frames.push(frame);
    }

    /// Pops the innermost frame, answering it so the caller can restore what it changed.
    pub fn pop_frame(&mut self) -> Option<ShellFrame> {
        self.navigation.frames.pop()
    }

    /// The runtime and the providers together, for a caller that needs both at once.
    ///
    /// Both are borrowed from the same `&mut self`, and a native pipeline needs to hold them for
    /// as long as it runs. Asking for them one at a time would mean two overlapping borrows of the
    /// session, so they are handed out together.
    ///
    /// Returns `None` only if the operating system refuses to start the runtime.
    pub fn pipeline_context(&mut self) -> Option<(&tokio::runtime::Runtime, &ProviderRegistry)> {
        self.runtime()?;
        // What `get job`, `get link` and `get plugin` answer is what is true when the pipeline
        // starts (ADR-0090, ADR-0103, ADR-0107).
        self.publish_jobs();
        self.publish_links();
        self.publish_host();
        self.publish_env();
        // Spec §14.4: the active link frame decides where provider calls run. The innermost
        // link frame wins; without one, the local registry answers.
        let remote = self
            .navigation
            .frames
            .iter()
            .rev()
            .find_map(|frame| {
                matches!(frame.frame.kind(), ono_command::FrameKind::Link)
                    .then(|| frame.frame.identity().to_string())
            })
            // v0.4 §19.2: standing on a linked host is the same statement about where provider
            // calls run, made by `jump` instead of by `enter link`. A link the session has
            // detached from is not reached: what is behind it is reported `stale` rather than
            // answered with local objects wearing a remote name (§35.2, §35.4).
            .or_else(|| {
                ono_spatial_core::space::standing_in()
                    .map(|scope| scope.host_scope().id().to_owned())
                    .filter(|name| crate::spatial::links::reachable(name))
            });
        if let Some(host) = remote
            && let Some(index) = self
                .navigation
                .links
                .iter()
                .position(|link| link.name == host && link.connection.is_some())
        {
            let runtime = self.execution.runtime.as_ref()?;
            let held = self.navigation.links[index].connection.as_ref()?;
            return Some((runtime, &held.registry));
        }
        self.providers();
        match (
            self.execution.runtime.as_ref(),
            self.provider.providers.as_ref(),
        ) {
            (Some(runtime), Some(providers)) => Some((runtime, providers)),
            _ => None,
        }
    }

    /// The providers this session can ask, built the first time one is needed.
    ///
    /// The adapter registry (spec v0.3 §1.24), built on first use.
    ///
    /// Version probes run through `probe_version`: a declared, bounded, non-interactive
    /// invocation with stdin closed and `LC_ALL=C`, whose output is read whole (ADR-0056).
    pub fn adapters(&mut self) -> &ono_adapter::Registry {
        self.shared_adapters();
        self.provider
            .adapters
            .as_deref()
            .unwrap_or_else(|| unreachable!("just constructed"))
    }

    /// The same registry, shared: commands that plan while they run (`type`), and the line
    /// editor's completion, hold it alongside the session (ADR-0067).
    pub fn shared_adapters(&mut self) -> std::sync::Arc<ono_adapter::Registry> {
        if self.provider.adapters.is_none() {
            self.provider.adapters = Some(std::sync::Arc::new(ono_adapter::Registry::bundled(
                Box::new(probe_version),
            )));
        }
        self.provider
            .adapters
            .clone()
            .unwrap_or_else(|| unreachable!("just constructed"))
    }

    /// Remembers that an adapter shaped the statement being run (spec v0.3 §1.62).
    pub fn note_adaptation(&mut self, adapter: String, plan: String) {
        self.provider.adaptations.push((adapter, plan));
    }

    /// The adaptations noted since the last call, for the history entry of the statement.
    pub fn take_adaptations(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.provider.adaptations)
    }

    /// The host of the innermost link frame, when the session is inside one (spec §21.2).
    #[must_use]
    pub fn link_host(&self) -> Option<String> {
        self.navigation
            .frames
            .iter()
            .rev()
            .find(|frame| matches!(frame.frame.kind(), ono_command::FrameKind::Link))
            .map(|frame| frame.frame.identity().to_string())
    }

    /// The connection the innermost link frame stands on, when the session is inside one.
    #[must_use]
    pub fn remote_link(&self) -> Option<&LinkConnection> {
        let host = self.link_host()?;
        self.navigation
            .links
            .iter()
            .find(|link| link.name == host)
            .and_then(|link| link.connection.as_ref())
    }

    /// Both registries a plan consults, borrowed together.
    pub fn registries(&mut self) -> (&ProviderRegistry, &ono_adapter::Registry) {
        let _ = self.providers();
        let _ = self.adapters();
        match (&self.provider.providers, self.provider.adapters.as_deref()) {
            (Some(providers), Some(adapters)) => (providers, adapters),
            _ => unreachable!("just constructed"),
        }
    }

    /// Building them opens sockets and speaks D-Bus, so it happens here rather than at startup.
    /// A provider that cannot be reached is still registered: it reports its own unavailability
    /// with a reason, which is a different answer from there being none of the thing asked for.
    pub fn providers(&mut self) -> &ProviderRegistry {
        if self.provider.providers.is_none() {
            let environment: Vec<(String, String)> = self
                .environment
                .env
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            let mut registry = crate::providers::registry_with_tables(
                environment,
                std::sync::Arc::clone(&self.jobs.tables),
                std::sync::Arc::clone(&self.environment.env_provider),
            );
            if let Some(runtime) = self.runtime() {
                runtime.block_on(crate::providers::register_async(&mut registry));
            }
            self.provider.providers = Some(registry);
        }
        self.provider
            .providers
            .as_ref()
            .unwrap_or_else(|| unreachable!("just constructed"))
    }

    /// Hands the `env` provider what the session holds now, so `get env` answers for this
    /// session rather than for the environment the shell was started with.
    pub fn publish_env(&self) {
        self.environment
            .env_provider
            .publish(self.environment.env.iter().map(|(name, value)| {
                let inherited = self.environment.inherited_env.get(name) == Some(value);
                let name = name.to_string_lossy();
                let value = value.to_string_lossy();
                if inherited {
                    ono_provider_linux::EnvBinding::inherited(name, value)
                } else {
                    ono_provider_linux::EnvBinding::shell(name, value, true)
                }
            }));
    }

    /// The environment pairs the presentation choice consults (spec §4.3, §4.6).
    #[must_use]
    pub fn presentation_environment(&self) -> Vec<(String, String)> {
        ["NO_COLOR", "TERM"]
            .into_iter()
            .filter_map(|name| {
                self.env_var(name)
                    .map(|value| (name.to_owned(), value.to_string_lossy().into_owned()))
            })
            .collect()
    }

    /// The current working directory.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.environment.cwd
    }

    /// Moves the session to `directory`, which must exist and be a directory.
    ///
    /// The process moves with it. A shell's working directory is the process's working directory:
    /// `find file .` and every other native command that takes a relative path resolves it
    /// through the kernel, so a session cwd the process did not follow would leave those commands
    /// answering about wherever the shell happened to start. A kernel that refuses the move —
    /// the directory went away between the caller's check and here — leaves the session where it
    /// was rather than splitting the two.
    pub fn set_cwd(&mut self, directory: PathBuf) {
        self.environment.set_cwd(directory);
    }

    /// The environment external commands will inherit.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<OsString, OsString> {
        &self.environment.env
    }

    /// Reads one environment variable.
    #[must_use]
    pub fn env_var(&self, name: &str) -> Option<&OsStr> {
        self.environment
            .env
            .get(OsStr::new(name))
            .map(OsString::as_os_str)
    }

    /// Sets one environment variable.
    pub fn set_env(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) {
        self.environment.env.insert(name.into(), value.into());
    }

    /// Removes one environment variable.
    pub fn remove_env(&mut self, name: &str) {
        self.environment.env.remove(OsStr::new(name));
    }

    /// The home directory, from the environment.
    #[must_use]
    pub fn home(&self) -> Option<PathBuf> {
        self.env_var("HOME").map(PathBuf::from)
    }

    /// Looks a binding up, innermost scope first (ADR-0010).
    #[must_use]
    pub fn binding(&self, name: &str) -> Option<&Value> {
        self.scope
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
    }

    /// Every visible binding, innermost scope winning, as a native stage's expressions see them.
    #[must_use]
    pub fn bindings(&self) -> BTreeMap<String, Value> {
        let mut visible = BTreeMap::new();
        for scope in &self.scope.scopes {
            visible.extend(
                scope
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone())),
            );
        }
        visible
    }

    /// Starts capturing: the next finished pipeline's values go to [`end_capture`] rather than
    /// to the terminal (ADR-0072 §4).
    ///
    /// [`end_capture`]: Self::end_capture
    pub fn begin_capture(&mut self) {
        self.execution.begin_capture();
    }

    /// Ends the innermost capture and returns what it collected.
    ///
    /// The bytes it held stay charged to the command's budget until the command ends. v0.4.1
    /// §23.4 bounds *"the total bytes retained by simultaneous evaluator captures"*, and a
    /// capture whose values were just handed to an enclosing scope, a variable or an argument
    /// list is still retained — a budget that refunded on `end_capture` would bound nothing that
    /// nesting does (ADR-0457).
    pub fn end_capture(&mut self) -> Vec<Value> {
        self.execution.end_capture()
    }

    /// Whether a pipeline's result is currently being captured rather than shown.
    #[must_use]
    pub fn capturing(&self) -> bool {
        self.execution.capturing()
    }

    /// Starts one shell command's capture accounting afresh (v0.4.1 §23.4).
    ///
    /// The ceiling is per command, so the statement loop calls this once per top-level statement.
    /// Nothing inside a command resets it: that is what makes nested captures share one allowance
    /// rather than each starting again at the global default.
    pub fn begin_command_captures(&mut self) {
        let bytes = crate::limits::command_capture_bytes(&self.presentation.settings);
        self.execution.begin_command_captures(bytes);
    }

    /// What this command's captures have retained so far, and what they may still retain.
    #[must_use]
    pub const fn capture_budget(&self) -> &Budget {
        &self.execution.capture_budget
    }

    /// Hands finished values to the innermost capture, if one is open.
    ///
    /// Returns whether they were taken; when they were not, they are the terminal's to show.
    ///
    /// Every value is charged to the one budget this command's captures share (v0.4.1 §23.1,
    /// §23.4), so a construction that nests captures cannot spend the ceiling once per level.
    ///
    /// # Errors
    ///
    /// The structured resource refusal of §21.4 when the command's capture ceiling is reached.
    /// §21.3 forbids the alternative: nothing is kept from the values that would not fit, and the
    /// capture is not silently truncated.
    pub fn capture(&mut self, values: &[Value]) -> Result<bool, ErrorValue> {
        self.execution.capture(values)
    }

    /// Declares `name` in the innermost scope, shadowing any outer binding of the name: what a
    /// loop variable, a parameter, a `catch` name and a block's `@` do.
    pub fn bind(&mut self, name: impl Into<String>, value: Value) {
        if let Some(scope) = self.scope.scopes.last_mut() {
            scope.insert(name.into(), value);
        }
    }

    /// What `let name = …` does (ADR-0009, ADR-0119): rebinds the innermost visible binding of
    /// `name` where there is one, so a block or a function body can advance a counter of the
    /// enclosing scope; otherwise declares it in the innermost scope, where it stays local.
    pub fn assign(&mut self, name: impl Into<String>, value: Value) {
        let name = name.into();
        match self
            .scope
            .scopes
            .iter_mut()
            .rev()
            .find(|scope| scope.contains_key(&name))
        {
            Some(scope) => {
                scope.insert(name, value);
            }
            None => self.bind(name, value),
        }
    }

    /// Enters a nested scope, for a block or a function body.
    pub fn push_scope(&mut self) {
        self.scope.scopes.push(Scope::new());
        self.scope.definitions.push(BTreeMap::new());
    }

    /// Leaves the innermost scope. The outermost scope is never popped.
    pub fn pop_scope(&mut self) {
        if self.scope.scopes.len() > 1 {
            self.scope.scopes.pop();
            self.scope.definitions.pop();
        }
    }

    /// The status of the last statement.
    #[must_use]
    pub fn status(&self) -> ExitStatus {
        self.execution.status
    }

    /// Records the status of a statement.
    pub fn set_status(&mut self, status: ExitStatus) {
        self.execution.status = status;
    }

    /// The process executor.
    pub fn executor(&mut self) -> &mut Executor {
        &mut self.execution.executor
    }

    /// What the evaluator is currently allowed to do.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.execution.mode
    }

    /// Runs `body` under `mode`, restoring the previous mode afterwards.
    pub fn in_mode<T>(&mut self, mode: Mode, body: impl FnOnce(&mut Self) -> T) -> T {
        let previous = std::mem::replace(&mut self.execution.mode, mode);
        let outcome = body(self);
        self.execution.mode = previous;
        outcome
    }

    /// Whether the session is attached to a person rather than to a script.
    #[must_use]
    pub fn is_interactive(&self) -> bool {
        self.execution.interactive
    }

    /// Asks the session to leave with `status` once the current statement finishes.
    pub fn leave(&mut self, status: ExitStatus) {
        self.execution.leaving = Some(status);
    }

    /// The status the session was asked to leave with, if it was.
    #[must_use]
    pub fn leaving(&self) -> Option<ExitStatus> {
        self.execution.leaving
    }

    /// Withdraws a request to leave.
    ///
    /// Used after reading configuration: a configuration file must not be able to end the session
    /// it is configuring. Without this, an `exit` in `config.ono` would replace the status of
    /// every command the shell ever ran and short-circuit every statement after the first
    /// (ADR-0010).
    pub fn stay(&mut self) {
        self.execution.leaving = None;
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Spec §31.37: the last pipeline's audit events are written before the session goes.
        self.with_kuang(crate::kuang_host::Host::persist_audit);
        // Fields are dropped after this runs, so the runtime is still here to wait on: a link
        // torn down after the runtime has gone could only abandon its agent (ADR-0161).
        self.hang_up_all();
    }
}

/// Runs an adapter's version probe and returns what the program wrote (spec v0.3 §1.46).
///
/// The probe is not a job: it has no terminal, no stdin and no place in the job table, so the
/// standard library's process API is the right tool rather than the executor of spec §18.
pub fn probe_version(executable: &std::path::Path, argv: &[String]) -> Option<String> {
    let output = std::process::Command::new(executable)
        .args(argv)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .ok()?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    Some(text)
}

/// The redaction policy the retention of spec §20.2 applies, which is history's own (§17.5).
///
/// One policy, built once: the patterns are the same ones `history` redacts by, so a secret
/// cannot be kept in one place because it was removed from the other (ADR-0262).
fn redaction_policy() -> &'static ono_history::Policy {
    static POLICY: std::sync::OnceLock<ono_history::Policy> = std::sync::OnceLock::new();
    POLICY.get_or_init(ono_history::Policy::default)
}
