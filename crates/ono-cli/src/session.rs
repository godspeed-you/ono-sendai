//! The state one shell session carries: where it is, what it knows, and what it is running.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use ono_core::ExitStatus;
use ono_process::Executor;
use ono_provider_api::ProviderRegistry;
use ono_value::Value;

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

/// Everything a running shell knows.
pub struct Session {
    cwd: PathBuf,
    env: BTreeMap<OsString, OsString>,
    scopes: Vec<Scope>,
    status: ExitStatus,
    executor: Executor,
    mode: Mode,
    interactive: bool,
    /// Set by `exit`, so the evaluator can unwind without unwinding the process.
    leaving: Option<ExitStatus>,
    /// Built on first use. A shell that runs `echo hi` should not have paid for a thread pool to
    /// do it, and spec §34's cold-start budget is measured on exactly that command.
    runtime: Option<tokio::runtime::Runtime>,
    /// Built on first use, for the same reason: constructing it opens sockets and speaks D-Bus.
    providers: Option<ProviderRegistry>,
    /// The context stack of spec §14.1, above the implicit ground frame. Each entry pairs the
    /// frame every command sees with what popping it must restore.
    frames: Vec<ShellFrame>,
    /// Recent structured results, newest last (spec §20.2). Bounded, so a long session cannot
    /// hold every table it ever printed.
    results: std::collections::VecDeque<Vec<Value>>,
    /// Backgrounded native pipelines (spec §18.4, ADR-0024): jobs in the same table as external
    /// commands, numbered from the executor's own sequence.
    native_jobs: Vec<NativeJob>,
    /// What the last interactive view left selected — the referent of bare `@` (spec §6.4,
    /// ADR-0033, ADR-0050).
    selection: Option<Value>,
    /// The remote links this session holds (spec §21.1), by the name the user gave them.
    links: Vec<SessionLink>,
    /// The KUANG/11 packages this session loaded (spec §31.10), by their manifest ids.
    plugins: Vec<(String, ono_kuang_supervisor::LoadedPlugin)>,
    /// The external command adapters (spec v0.3 §1.24), built on first use: the registry holds
    /// the version probe cache, which is per session by design (§1.46).
    adapters: Option<ono_adapter::Registry>,
}

/// One held remote link: the connection, and the registry its providers are mounted in.
pub struct SessionLink {
    /// The host as the user named it.
    pub name: String,
    /// How the bytes travel, for `get link`.
    pub transport: String,
    /// The link itself, kept so dropping the session hangs up.
    pub link: ono_remote::RemoteLink,
    /// The mounted registry an active link frame answers from (spec §14.4).
    pub registry: std::sync::Arc<ProviderRegistry>,
}

impl std::fmt::Debug for SessionLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionLink")
            .field("name", &self.name)
            .field("transport", &self.transport)
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

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("cwd", &self.cwd)
            .field("status", &self.status)
            .field("mode", &self.mode)
            .field("interactive", &self.interactive)
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
        let env = std::env::vars_os().collect();
        // The executor attaches to the controlling terminal whenever there is one, whether or not
        // the session is interactive. `ono -c 'less file'` typed at a terminal must hand that
        // terminal to `less`, exactly as `bash -c` does — a child that is never given the
        // terminal is stopped by SIGTTOU the moment it tries to configure it (spec §18.1,
        // ADR-0013). With no controlling terminal there is nothing to hand over and the executor
        // detaches.
        let executor = Executor::new().unwrap_or_else(|_| Executor::detached());
        Self {
            cwd,
            env,
            scopes: vec![Scope::new()],
            status: ExitStatus::SUCCESS,
            executor,
            mode: Mode::Normal,
            interactive,
            leaving: None,
            runtime: None,
            providers: None,
            frames: Vec::new(),
            results: std::collections::VecDeque::new(),
            native_jobs: Vec::new(),
            selection: None,
            links: Vec::new(),
            plugins: Vec::new(),
            adapters: None,
        }
    }

    /// The async runtime native pipelines run on, built the first time one is needed.
    ///
    /// Returns `None` only if the operating system refuses to start it, which a caller reports as
    /// a structured error rather than treating as impossible.
    pub fn runtime(&mut self) -> Option<&tokio::runtime::Runtime> {
        if self.runtime.is_none() {
            self.runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("ono")
                .build()
                .ok();
        }
        self.runtime.as_ref()
    }

    /// A handle to the runtime, cloneable and borrow-free, once it exists.
    #[must_use]
    pub fn runtime_handle(&self) -> Option<tokio::runtime::Handle> {
        self.runtime
            .as_ref()
            .map(tokio::runtime::Runtime::handle)
            .cloned()
    }

    /// Keeps a loaded KUANG/11 package on the session (spec §31.10).
    pub fn add_plugin(&mut self, id: String, plugin: ono_kuang_supervisor::LoadedPlugin) {
        self.plugins.retain(|(held, _)| held != &id);
        self.plugins.push((id, plugin));
    }

    /// A loaded package by its manifest id.
    #[must_use]
    pub fn plugin(&self, id: &str) -> Option<&ono_kuang_supervisor::LoadedPlugin> {
        self.plugins
            .iter()
            .find(|(held, _)| held == id)
            .map(|(_, plugin)| plugin)
    }

    /// The ids of every loaded package.
    pub fn plugin_ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.iter().map(|(id, _)| id.as_str())
    }

    /// Adds a remote link to the session's table.
    pub fn add_link(&mut self, link: SessionLink) {
        self.links.retain(|held| held.name != link.name);
        self.links.push(link);
    }

    /// The links this session holds, oldest first.
    #[must_use]
    pub fn links(&self) -> &[SessionLink] {
        &self.links
    }

    /// The mounted registry of the named link, if the session holds it.
    #[must_use]
    pub fn link_registry(&self, name: &str) -> Option<std::sync::Arc<ProviderRegistry>> {
        self.links
            .iter()
            .find(|link| link.name == name)
            .map(|link| std::sync::Arc::clone(&link.registry))
    }

    /// Keeps `value` as the interactive selection bare `@` refers to (ADR-0050).
    pub fn select(&mut self, value: Value) {
        self.selection = Some(value);
    }

    /// The interactive selection, if a view has set one.
    #[must_use]
    pub fn selection(&self) -> Option<&Value> {
        self.selection.as_ref()
    }

    /// Adds a backgrounded native pipeline to the job table.
    pub fn push_native_job(&mut self, job: NativeJob) {
        self.native_jobs.push(job);
    }

    /// The backgrounded native pipelines, oldest first.
    #[must_use]
    pub fn native_jobs(&self) -> &[NativeJob] {
        &self.native_jobs
    }

    /// Removes and answers native job `number`, releasing its number.
    pub fn take_native_job(&mut self, number: u32) -> Option<NativeJob> {
        let index = self
            .native_jobs
            .iter()
            .position(|job| job.number == number)?;
        let job = self.native_jobs.remove(index);
        self.executor.release_job_number(number);
        Some(job)
    }

    /// Retains a finished pipeline's values for `@-1` and `@N` (spec §6.4, §20.2).
    ///
    /// Retention is bounded twice: by result count, and by values per result — a `get file /`
    /// that printed a million rows does not pin a million values in memory forever. A truncated
    /// retention is honest about being one: reusing it yields the rows that were kept, exactly
    /// as the screen showed only the rows that fit.
    pub fn retain_result(&mut self, mut values: Vec<Value>) {
        const KEEP_RESULTS: usize = 16;
        const KEEP_VALUES: usize = 10_000;
        if values.is_empty() {
            return;
        }
        values.truncate(KEEP_VALUES);
        if self.results.len() == KEEP_RESULTS {
            self.results.pop_front();
        }
        self.results.push_back(values);
    }

    /// The `n`th previous result, `1` for the most recent (spec §6.4 `@-1`).
    #[must_use]
    pub fn previous_result(&self, n: u32) -> Option<&[Value]> {
        let index = self.results.len().checked_sub(n as usize)?;
        self.results.get(index).map(Vec::as_slice)
    }

    /// The context stack above the ground frame, outermost first (spec §14.1).
    #[must_use]
    pub fn frames(&self) -> &[ShellFrame] {
        &self.frames
    }

    /// The frames as commands see them, for an [`ono_command::Invocation`].
    #[must_use]
    pub fn context(&self) -> Vec<ono_command::ContextFrame> {
        self.frames
            .iter()
            .map(|entry| entry.frame.clone())
            .collect()
    }

    /// Pushes a frame (spec §14.1: `enter` pushes).
    pub fn push_frame(&mut self, frame: ShellFrame) {
        self.frames.push(frame);
    }

    /// Pops the innermost frame, answering it so the caller can restore what it changed.
    pub fn pop_frame(&mut self) -> Option<ShellFrame> {
        self.frames.pop()
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
        // Spec §14.4: the active link frame decides where provider calls run. The innermost
        // link frame wins; without one, the local registry answers.
        let remote = self.frames.iter().rev().find_map(|frame| {
            matches!(frame.frame.kind(), ono_command::FrameKind::Link)
                .then(|| frame.frame.identity().to_string())
        });
        if let Some(host) = remote
            && let Some(index) = self.links.iter().position(|link| link.name == host)
        {
            let runtime = self.runtime.as_ref()?;
            return Some((runtime, &self.links[index].registry));
        }
        self.providers();
        match (self.runtime.as_ref(), self.providers.as_ref()) {
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
        if self.adapters.is_none() {
            self.adapters = Some(ono_adapter::Registry::bundled(Box::new(probe_version)));
        }
        self.adapters
            .as_ref()
            .unwrap_or_else(|| unreachable!("just constructed"))
    }

    /// The host of the innermost link frame, when the session is inside one (spec §21.2).
    #[must_use]
    pub fn link_host(&self) -> Option<String> {
        self.frames
            .iter()
            .rev()
            .find(|frame| matches!(frame.frame.kind(), ono_command::FrameKind::Link))
            .map(|frame| frame.frame.identity().to_string())
    }

    /// The link the innermost link frame stands on, when the session is inside one.
    #[must_use]
    pub fn remote_link(&self) -> Option<&SessionLink> {
        let host = self.link_host()?;
        self.links.iter().find(|link| link.name == host)
    }

    /// The adapter registry, to add a package's packs to (spec v0.3 §1.26).
    pub fn adapters_mut(&mut self) -> &mut ono_adapter::Registry {
        let _ = self.adapters();
        self.adapters
            .as_mut()
            .unwrap_or_else(|| unreachable!("just constructed"))
    }

    /// Both registries a plan consults, borrowed together.
    pub fn registries(&mut self) -> (&ProviderRegistry, &ono_adapter::Registry) {
        let _ = self.providers();
        let _ = self.adapters();
        match (&self.providers, &self.adapters) {
            (Some(providers), Some(adapters)) => (providers, adapters),
            _ => unreachable!("just constructed"),
        }
    }

    /// Building them opens sockets and speaks D-Bus, so it happens here rather than at startup.
    /// A provider that cannot be reached is still registered: it reports its own unavailability
    /// with a reason, which is a different answer from there being none of the thing asked for.
    pub fn providers(&mut self) -> &ProviderRegistry {
        if self.providers.is_none() {
            let environment: Vec<(String, String)> = self
                .env
                .iter()
                .map(|(name, value)| {
                    (
                        name.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
                .collect();
            let mut registry = crate::providers::registry(environment);
            if let Some(runtime) = self.runtime() {
                runtime.block_on(crate::providers::register_async(&mut registry));
            }
            self.providers = Some(registry);
        }
        self.providers
            .as_ref()
            .unwrap_or_else(|| unreachable!("just constructed"))
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
        &self.cwd
    }

    /// Moves the session to `directory`, which must exist and be a directory.
    pub fn set_cwd(&mut self, directory: PathBuf) {
        self.cwd = directory;
    }

    /// The environment external commands will inherit.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<OsString, OsString> {
        &self.env
    }

    /// Reads one environment variable.
    #[must_use]
    pub fn env_var(&self, name: &str) -> Option<&OsStr> {
        self.env.get(OsStr::new(name)).map(OsString::as_os_str)
    }

    /// Sets one environment variable.
    pub fn set_env(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) {
        self.env.insert(name.into(), value.into());
    }

    /// Removes one environment variable.
    pub fn remove_env(&mut self, name: &str) {
        self.env.remove(OsStr::new(name));
    }

    /// The home directory, from the environment.
    #[must_use]
    pub fn home(&self) -> Option<PathBuf> {
        self.env_var("HOME").map(PathBuf::from)
    }

    /// Looks a binding up, innermost scope first (ADR-0010).
    #[must_use]
    pub fn binding(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    /// Binds `name` in the innermost scope. A further `let` rebinds it (ADR-0009).
    pub fn bind(&mut self, name: impl Into<String>, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.into(), value);
        }
    }

    /// Enters a nested scope, for a block or a function body.
    pub fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    /// Leaves the innermost scope. The outermost scope is never popped.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// The status of the last statement.
    #[must_use]
    pub fn status(&self) -> ExitStatus {
        self.status
    }

    /// Records the status of a statement.
    pub fn set_status(&mut self, status: ExitStatus) {
        self.status = status;
    }

    /// The process executor.
    pub fn executor(&mut self) -> &mut Executor {
        &mut self.executor
    }

    /// What the evaluator is currently allowed to do.
    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Runs `body` under `mode`, restoring the previous mode afterwards.
    pub fn in_mode<T>(&mut self, mode: Mode, body: impl FnOnce(&mut Self) -> T) -> T {
        let previous = std::mem::replace(&mut self.mode, mode);
        let outcome = body(self);
        self.mode = previous;
        outcome
    }

    /// Whether the session is attached to a person rather than to a script.
    #[must_use]
    pub fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Asks the session to leave with `status` once the current statement finishes.
    pub fn leave(&mut self, status: ExitStatus) {
        self.leaving = Some(status);
    }

    /// The status the session was asked to leave with, if it was.
    #[must_use]
    pub fn leaving(&self) -> Option<ExitStatus> {
        self.leaving
    }

    /// Withdraws a request to leave.
    ///
    /// Used after reading configuration: a configuration file must not be able to end the session
    /// it is configuring. Without this, an `exit` in `config.ono` would replace the status of
    /// every command the shell ever ran and short-circuit every statement after the first
    /// (ADR-0010).
    pub fn stay(&mut self) {
        self.leaving = None;
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
