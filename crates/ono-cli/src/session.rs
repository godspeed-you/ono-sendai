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
        self.providers();
        match (self.runtime.as_ref(), self.providers.as_ref()) {
            (Some(runtime), Some(providers)) => Some((runtime, providers)),
            _ => None,
        }
    }

    /// The providers this session can ask, built the first time one is needed.
    ///
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
