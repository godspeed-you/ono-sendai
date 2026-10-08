//! Running pipelines, owning the terminal, and keeping the job table (spec §18.1, §29).

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::killpg;
use nix::sys::wait::{Id, WaitPidFlag, WaitStatus, waitid, waitpid};
use nix::unistd::Pid;
use ono_core::ExitStatus;

use crate::command::Command;
use crate::error::{Error, Result};
use crate::job::{Collector, Job, JobChange, JobId, JobState, Running, RunningStage};
use crate::pipeline::{Pipeline, PipelineOutcome, StageOutcome};
use crate::plan;
use crate::pty::PtySession;
use crate::resolve::{self, Resolution};
use crate::signals::Signal;
use crate::spawn::{self, SpawnRequest};
use crate::terminal::{Terminal, WindowSize};

/// How a foreground pipeline ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForegroundOutcome {
    /// Every stage finished.
    Completed(PipelineOutcome),
    /// A stage was stopped, so the pipeline became a job the user can resume.
    Stopped {
        /// The job the stopped pipeline became.
        job: JobId,
        /// The signal that stopped it.
        signal: Signal,
    },
}

impl ForegroundOutcome {
    /// The pipeline outcome, if the run finished rather than stopping.
    #[must_use]
    pub const fn completed(&self) -> Option<&PipelineOutcome> {
        match self {
            Self::Completed(outcome) => Some(outcome),
            Self::Stopped { .. } => None,
        }
    }

    /// The job a stopped run became, if it stopped.
    #[must_use]
    pub const fn stopped(&self) -> Option<JobId> {
        match self {
            Self::Completed(_) => None,
            Self::Stopped { job, .. } => Some(*job),
        }
    }

    /// The status the shell reports for this run (ADR-0008).
    ///
    /// A stopped pipeline reports `128 + N`, the same convention a terminated one uses.
    #[must_use]
    pub fn status(&self) -> ExitStatus {
        match self {
            Self::Completed(outcome) => outcome.status(),
            Self::Stopped { signal, .. } => {
                ExitStatus::from_signal(u8::try_from(signal.number()).unwrap_or(0))
            }
        }
    }
}

/// A handle for interrupting whatever is running in the foreground (spec §18.5).
///
/// Cancelling a shell-level operation becomes `SIGINT` to the foreground process group, which
/// is what the terminal itself would have sent. The handle is cheap to clone and safe to keep:
/// when nothing is in the foreground, cancelling does nothing.
#[derive(Debug, Clone)]
pub struct Canceller {
    foreground: Arc<ForegroundGroup>,
}

/// The process group in the foreground, or `0`.
///
/// A group is signalled only while the lock is held, and the executor clears it under the same
/// lock in the same step that collects the group's last member. Until then that member is a
/// zombie holding the group's id, so a signal can never reach a group whose id the kernel has
/// handed out again — however late a [`Canceller`] on another thread sends it.
#[derive(Debug, Default)]
struct ForegroundGroup(std::sync::Mutex<i32>);

impl ForegroundGroup {
    fn lock(&self) -> std::sync::MutexGuard<'_, i32> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set(&self, group: i32) {
        *self.lock() = group;
    }
}

/// Every group cell an executor of this process holds — each executor's foreground group and each
/// of its background jobs' — so a shell that leaves can reach everything it owns, whichever thread
/// started it (ADR-0959). Weak: a cell goes when its executor or its job does.
static OWNED: std::sync::Mutex<Vec<std::sync::Weak<ForegroundGroup>>> =
    std::sync::Mutex::new(Vec::new());

/// Records `cell` in `set`.
fn record(
    set: &std::sync::Mutex<Vec<std::sync::Weak<ForegroundGroup>>>,
    cell: &Arc<ForegroundGroup>,
) {
    let mut owned = set
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    owned.retain(|weak| weak.strong_count() > 0);
    owned.push(Arc::downgrade(cell));
}

/// The groups one shell session owns: those of its own executor and of every executor its jobs'
/// evaluators run, which share it (ADR-0959, review S4). A second session in the same process has
/// a set of its own, so ending one never reaches the other's processes.
#[derive(Debug, Clone, Default)]
pub struct Ownership(Arc<std::sync::Mutex<Vec<std::sync::Weak<ForegroundGroup>>>>);

impl Ownership {
    /// The groups in this set that still have a member.
    #[must_use]
    pub fn groups(&self) -> Vec<OwnedGroup> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter_map(std::sync::Weak::upgrade)
            .map(OwnedGroup)
            .filter(|group| group.group() != 0)
            .collect()
    }
}

/// Whether any process of group `pgid` is alive, a zombie not counting.
fn group_runs(pgid: i32) -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        // Without `/proc`, a signal of `0` says whether anything of the group is left at all.
        return killpg(Pid::from_raw(pgid), None).is_ok();
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
            && std::fs::read_to_string(entry.path().join("stat"))
                .ok()
                .and_then(|stat| {
                    // `pid (comm) state ppid pgrp …`: the command may hold spaces and parentheses,
                    // so the fields are read after its last `)`.
                    let rest = stat.rsplit_once(')')?.1;
                    let mut fields = rest.split_whitespace();
                    let state = fields.next()?;
                    let _ppid = fields.next()?;
                    let group = fields.next()?.parse::<i32>().ok()?;
                    Some(group == pgid && state != "Z" && state != "X")
                })
                .unwrap_or(false)
    })
}

/// Every process group this process still owns: a foreground group some executor is waiting on,
/// or a background job's that has a member left to collect (ADR-0959).
#[must_use]
pub fn owned_groups() -> Vec<OwnedGroup> {
    OWNED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter_map(std::sync::Weak::upgrade)
        .map(OwnedGroup)
        .filter(|group| group.group() != 0)
        .collect()
}

/// A process group this process owns, held as the cell that names it.
///
/// The cell names the group while one of its members is uncollected, and is cleared under its
/// lock in the same step that collects the last one (review R10 of v0.6.3), so a signal sent
/// through it never reaches a group whose id the kernel has handed out again.
#[derive(Debug, Clone)]
pub struct OwnedGroup(Arc<ForegroundGroup>);

impl OwnedGroup {
    /// The group's id, or `0` once it is gone.
    #[must_use]
    pub fn group(&self) -> i32 {
        *self.0.lock()
    }

    /// Sends `signal` to the group while it is still owned; answers whether it was.
    pub fn send(&self, signal: Signal) -> bool {
        let group = self.0.lock();
        if *group == 0 {
            return false;
        }
        let _ = signal_group(*group, signal);
        true
    }

    /// Whether nothing of the group is still running: it has no member, or every member has
    /// exited and waits only to be collected.
    ///
    /// Nothing is collected here: collecting is the owner's, whose wait would otherwise find the
    /// child gone and record a status it never saw (review M1). A member that has exited is no
    /// longer running, whoever collects it — and when the process ends, nobody need.
    #[must_use]
    pub fn is_gone(&self) -> bool {
        let group = self.0.lock();
        *group == 0 || !group_runs(*group)
    }
}

impl Canceller {
    /// The process group it would signal right now, or `0`.
    #[must_use]
    pub fn group(&self) -> i32 {
        *self.foreground.lock()
    }

    /// The same group as an [`OwnedGroup`], for the shutdown that ends what the shell owns.
    #[must_use]
    pub fn owned(&self) -> OwnedGroup {
        OwnedGroup(Arc::clone(&self.foreground))
    }

    /// Whether there is a foreground job to cancel right now.
    #[must_use]
    pub fn is_active(&self) -> bool {
        *self.foreground.lock() != 0
    }

    /// Interrupts the foreground job, as `Ctrl-C` would.
    ///
    /// # Errors
    ///
    /// Returns an error if the signal cannot be sent for a reason other than the job having
    /// already finished.
    pub fn cancel(&self) -> Result<()> {
        self.send(Signal::INT)
    }

    /// Sends an arbitrary signal to the foreground process group.
    ///
    /// # Errors
    ///
    /// Returns an error if the signal cannot be sent for a reason other than the job having
    /// already finished.
    pub fn send(&self, signal: Signal) -> Result<()> {
        let group = self.foreground.lock();
        if *group == 0 {
            return Ok(());
        }
        signal_group(*group, signal)
    }
}

/// The shell's execution layer: it runs pipelines and remembers the jobs it started.
///
/// ```no_run
/// use ono_process::{Command, Executor, ForegroundOutcome};
///
/// let mut executor = Executor::new()?;
/// let id = executor.run_background(&Command::new("sleep").arg("30").into())?;
/// for job in executor.jobs() {
///     println!("{job}");
/// }
/// executor.foreground(id)?;
/// # Ok::<(), ono_process::Error>(())
/// ```
#[derive(Debug)]
pub struct Executor {
    terminal: Terminal,
    jobs: Vec<Tracked>,
    foreground: Arc<ForegroundGroup>,
    /// The session's set of owned groups, which this executor's groups join.
    ownership: Ownership,
    /// Numbers handed to jobs this executor does not run — the shell's native pipelines — so
    /// one sequence covers both kinds (spec §18.4).
    reserved: std::collections::BTreeSet<u32>,
}

#[derive(Debug)]
struct Tracked {
    id: JobId,
    running: Running,
    /// The job's group while one of its members is uncollected, cleared in the step that
    /// collects the last (ADR-0959).
    group: Arc<ForegroundGroup>,
}

impl Tracked {
    /// Collects what `waitpid` has to say about the job, clearing its group under the same lock
    /// when nothing of it is left.
    fn reap(&mut self, block: bool) -> Result<()> {
        let mut group = self.group.lock();
        let reaped = reap(&mut self.running, block);
        if self.running.is_finished() {
            *group = 0;
        }
        reaped
    }
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("pgid", &self.pgid)
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

impl Executor {
    /// An executor attached to the controlling terminal, if this process has one.
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal cannot be inspected.
    pub fn new() -> Result<Self> {
        Ok(Self::with_terminal(Terminal::open()?))
    }

    /// An executor that never touches a terminal, for scripts and non-interactive runs.
    #[must_use]
    pub fn detached() -> Self {
        Self::with_terminal(Terminal::detached())
    }

    fn with_terminal(terminal: Terminal) -> Self {
        Self::within(terminal, Ownership::default())
    }

    /// An executor that never touches a terminal and whose groups join `ownership` — a job's
    /// evaluator's, which the session that started the job owns (ADR-0959).
    #[must_use]
    pub fn detached_within(ownership: Ownership) -> Self {
        Self::within(Terminal::detached(), ownership)
    }

    fn within(terminal: Terminal, ownership: Ownership) -> Self {
        let foreground = Arc::default();
        let executor = Self {
            terminal,
            jobs: Vec::new(),
            foreground,
            ownership,
            reserved: std::collections::BTreeSet::new(),
        };
        executor.own(&executor.foreground);
        executor
    }

    /// Records `cell` as owned by this process and by this executor's session.
    fn own(&self, cell: &Arc<ForegroundGroup>) {
        record(&OWNED, cell);
        record(&self.ownership.0, cell);
    }

    /// The set of groups this executor's session owns.
    #[must_use]
    pub fn ownership(&self) -> Ownership {
        self.ownership.clone()
    }

    /// The process groups this executor owns: the one it waits on in the foreground, if any, and
    /// each background job's with a member left to collect (ADR-0959).
    #[must_use]
    pub fn owned_groups(&self) -> Vec<OwnedGroup> {
        std::iter::once(OwnedGroup(Arc::clone(&self.foreground)))
            .chain(
                self.jobs
                    .iter()
                    .map(|tracked| OwnedGroup(Arc::clone(&tracked.group))),
            )
            .filter(|group| group.group() != 0)
            .collect()
    }

    /// The terminal this executor hands to foreground jobs, if there is one.
    #[must_use]
    pub const fn terminal(&self) -> &Terminal {
        &self.terminal
    }

    /// A handle for cancelling whatever is in the foreground (spec §18.5).
    #[must_use]
    pub fn canceller(&self) -> Canceller {
        Canceller {
            foreground: Arc::clone(&self.foreground),
        }
    }

    /// Runs `pipeline` in the foreground, giving it the terminal and waiting for it.
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline could not be started at all. A stage that could not be
    /// started, or that failed, is reported in the outcome rather than as an error, because the
    /// remaining stages still ran.
    pub fn run_foreground(&mut self, pipeline: &Pipeline) -> Result<ForegroundOutcome> {
        let foreground = self.start_foreground(pipeline)?;
        self.finish_foreground(foreground)
    }

    /// Starts `pipeline` in the foreground and returns before it finishes, so the caller can
    /// read a stage's [`Output::Pipe`](crate::Output::Pipe) while it runs; [`Executor::finish_foreground`] waits.
    ///
    /// # Errors
    ///
    /// As [`Executor::run_foreground`].
    pub fn start_foreground(&mut self, pipeline: &Pipeline) -> Result<Foreground> {
        // Remembered before the spawn: the child claims the terminal before `exec` (spawn.rs),
        // so after `start` returns it may already have changed the attributes being saved.
        self.terminal.remember_attributes();
        let running = self.start(pipeline, true)?;
        if running.pgid != 0 {
            self.terminal.give_to(running.pgid)?;
            self.foreground.set(running.pgid);
        }
        Ok(Foreground {
            running,
            owns_terminal: true,
            cell: None,
        })
    }

    /// Starts `pipeline` in its own process group without handing it the terminal, and
    /// returns before it finishes.
    ///
    /// For a child whose output the shell itself consumes (an adapted command streaming
    /// records, ADR-0059): the terminal stays with the shell, so Ctrl-C reaches the shell's
    /// own pipeline and the child is told to stop through [`Foreground::terminate`].
    ///
    /// # Errors
    ///
    /// As [`Executor::run_foreground`].
    pub fn start_piped(&mut self, pipeline: &Pipeline) -> Result<Foreground> {
        let running = self.start(pipeline, false)?;
        // Owned like any foreground group, though not given the terminal (review R5) — in a cell
        // of its own, so a program the same line starts later cannot take its place in the
        // executor's foreground cell (review M3). The cell is cleared in the step that collects
        // the group's last member.
        let cell = Arc::new(ForegroundGroup(std::sync::Mutex::new(running.pgid)));
        self.own(&cell);
        Ok(Foreground {
            running,
            owns_terminal: false,
            cell: Some(cell),
        })
    }

    /// Waits for a started pipeline and turns it into its outcome.
    ///
    /// # Errors
    ///
    /// As [`Executor::run_foreground`].
    pub fn finish_foreground(&mut self, mut foreground: Foreground) -> Result<ForegroundOutcome> {
        let outcome = match &foreground.cell {
            Some(cell) => {
                let outcome = wait_foreground(&mut foreground.running, cell);
                cell.set(0);
                outcome
            }
            None => {
                let outcome = wait_foreground(&mut foreground.running, &self.foreground);
                // A pipeline that stopped keeps no claim on the cell: it becomes a job.
                self.foreground.set(0);
                outcome
            }
        };
        if foreground.owns_terminal {
            let reclaimed = self.terminal.reclaim();
            outcome?;
            reclaimed?;
        } else {
            outcome?;
        }
        self.settle(foreground.running)
    }

    /// Runs `pipeline` in the background as a new job (`&`).
    ///
    /// # Errors
    ///
    /// Returns an error if the pipeline could not be started at all.
    pub fn run_background(&mut self, pipeline: &Pipeline) -> Result<JobId> {
        let running = self.start(pipeline, false)?;
        Ok(self.register(running))
    }

    /// Runs one command under a pseudoterminal of its own (spec §29.3).
    ///
    /// # Errors
    ///
    /// Returns an error if the terminal cannot be allocated or the program cannot be started.
    pub fn run_pty(&mut self, command: &Command, size: WindowSize) -> Result<PtySession> {
        PtySession::start(command, size)
    }

    /// Reaps whatever the operating system is willing to tell us, without blocking.
    ///
    /// This is what a prompt calls before it draws: it turns stops, continuations and exits
    /// into job-table transitions. Nothing is lost by calling it rarely — the kernel keeps each
    /// child in its stopped or zombie state until it is asked about.
    ///
    /// # Errors
    ///
    /// Returns an error if waiting fails for a reason other than the job having disappeared.
    pub fn poll_jobs(&mut self) -> Result<Vec<JobChange>> {
        let mut changes = Vec::new();
        for tracked in &mut self.jobs {
            let previous = tracked.running.state();
            tracked.reap(false)?;
            let current = tracked.running.state();
            if current != previous {
                changes.push(JobChange {
                    id: tracked.id,
                    previous,
                    current,
                });
            }
        }
        Ok(changes)
    }

    /// Every job the shell is still responsible for, in job-number order.
    #[must_use]
    pub fn jobs(&self) -> Vec<Job> {
        self.jobs
            .iter()
            .map(|tracked| tracked.running.snapshot(tracked.id))
            .collect()
    }

    /// One job, if the shell still has it.
    #[must_use]
    pub fn job(&self, id: JobId) -> Option<Job> {
        self.jobs
            .iter()
            .find(|tracked| tracked.id == id)
            .map(|tracked| tracked.running.snapshot(id))
    }

    /// Brings a job back to the foreground (`fg`), continuing it if it was stopped.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such job, or if the terminal cannot be moved.
    pub fn foreground(&mut self, id: JobId) -> Result<ForegroundOutcome> {
        let index = self.locate(id)?;
        let tracked = self.jobs.remove(index);
        let mut running = {
            // The group moves from the job's cell to the foreground's under both locks, so no
            // moment exists in which a leaving shell could not reach it.
            let mut foreground = self.foreground.lock();
            let mut group = tracked.group.lock();
            if !tracked.running.is_finished() && tracked.running.pgid != 0 {
                *foreground = tracked.running.pgid;
            }
            *group = 0;
            drop(group);
            drop(foreground);
            tracked.running
        };
        self.terminal.remember_attributes();
        // The terminal is handed over before the group is woken: a job continued first could
        // read in the instant before the handover and be stopped again by `SIGTTIN`.
        if !running.is_finished() && running.pgid != 0 {
            self.terminal.give_to(running.pgid)?;
        }
        if running.is_stopped() {
            continue_group(&mut running)?;
        }
        let outcome = wait_foreground(&mut running, &self.foreground);
        self.foreground.set(0);
        let reclaimed = self.terminal.reclaim();
        outcome?;
        reclaimed?;
        self.settle(running)
    }

    /// Continues a stopped job in the background (`bg`).
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such job, or if it cannot be continued.
    pub fn background(&mut self, id: JobId) -> Result<()> {
        let index = self.locate(id)?;
        continue_group(&mut self.jobs[index].running)
    }

    /// Sends a signal to every process in a job.
    ///
    /// A job that has already finished is not an error: the shell asked for something that has
    /// simply already happened.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such job, or if the signal cannot be sent.
    pub fn signal_job(&mut self, id: JobId, signal: Signal) -> Result<()> {
        let index = self.locate(id)?;
        {
            // Through the job's cell: a job whose last member was collected is not signalled,
            // because its group id may already be someone else's.
            let group = self.jobs[index].group.lock();
            if *group == 0 {
                return Ok(());
            }
            signal_group(*group, signal)?;
        }
        if signal == Signal::CONT {
            resume(&mut self.jobs[index].running);
        }
        Ok(())
    }

    /// Waits for a job to finish, giving up after `timeout`.
    ///
    /// Returns the finished job and drops it from the table, or `None` if it is still going.
    /// With no timeout it waits as long as the job takes.
    ///
    /// # Errors
    ///
    /// Returns an error if there is no such job, or if waiting fails.
    pub fn wait_job(&mut self, id: JobId, timeout: Option<Duration>) -> Result<Option<Job>> {
        let deadline = timeout.map(|timeout| Instant::now() + timeout);
        loop {
            let index = self.locate(id)?;
            self.jobs[index].reap(false)?;
            if self.jobs[index].running.is_finished() {
                let running = self.jobs.remove(index).running;
                let snapshot = running.snapshot(id);
                collect(running);
                return Ok(Some(snapshot));
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

/// A stage that has been resolved and had its descriptors opened, but not yet spawned.
///
/// Preparing every stage before spawning any is what lets a pipeline refuse to run at all when
/// one of its parts cannot be built (ADR-0008).
struct ReadyStage {
    program: std::path::PathBuf,
    args: Vec<std::ffi::OsString>,
    env: Option<Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>>,
    clear_env: bool,
    cwd: Option<std::path::PathBuf>,
    io: plan::StageIo,
}

/// The stage list for a pipeline that never ran, with the reason on the stage that caused it.
///
/// Every stage reports the same failing status, because none of them ran: reporting success for
/// the others would say a stage finished that was never started.
fn refuse(count: usize, at: usize, refusal: (ExitStatus, Error)) -> Vec<RunningStage> {
    let (status, error) = refusal;
    (0..count)
        .map(|index| RunningStage {
            pid: 0,
            state: JobState::Exited(status),
            failure: (index == at).then(|| error.clone()),
            stdout: None,
            stderr: None,
            pipe: None,
            stdin: None,
        })
        .collect()
}

impl Executor {
    /// Starts every stage of a pipeline in one new process group.
    fn start(&mut self, pipeline: &Pipeline, foreground: bool) -> Result<Running> {
        // A shell that has begun to end on a signal starts nothing more (ADR-0959, review B1).
        if crate::signals::terminating() {
            return Err(Error::new(
                ono_core::ErrorCode::StreamCancelled,
                "the shell is ending, and starts no more programs",
            ));
        }
        let mut running = Running {
            pgid: 0,
            command: pipeline.to_string(),
            stages: Vec::with_capacity(pipeline.len()),
        };

        // Every pipe is made before anything is spawned, so each stage can be handed its ends
        // and the parent's copies can be dropped the moment that stage has forked.
        let mut pipes: Vec<(OwnedFd, OwnedFd)> = Vec::new();
        for _ in 1..pipeline.len() {
            pipes.push(
                nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC)
                    .map_err(|errno| spawn::system("creating a pipeline", errno))?,
            );
        }
        let mut read_ends: Vec<Option<OwnedFd>> = Vec::with_capacity(pipeline.len());
        let mut write_ends: Vec<Option<OwnedFd>> = Vec::with_capacity(pipeline.len());
        read_ends.push(None);
        for (read_end, write_end) in pipes {
            read_ends.push(Some(read_end));
            write_ends.push(Some(write_end));
        }
        write_ends.push(None);

        // Everything that can fail without side effects is checked before anything is spawned:
        // every program is resolved and every redirection is opened. A pipeline with a name that
        // does not resolve, or a redirection that cannot be opened, therefore runs *nothing*
        // rather than running the stages that happened to come first (ADR-0008). Bash runs what
        // it can, so `nonesuch | cat` reports success having produced an empty result that looks
        // like a real one; this is the same principle as spec §11.6 — know what will happen
        // before any of it does — applied to the cheapest case there is.
        let mut prepared = Vec::with_capacity(pipeline.len());
        for (index, command) in pipeline.stages().iter().enumerate() {
            let piped_input = read_ends[index].take();
            let piped_output = write_ends[index].take();
            match self.prepare_stage(command, piped_input, piped_output) {
                Ok(ready) => prepared.push(ready),
                Err(refusal) => {
                    // Dropping the prepared stages closes every pipe end and every file opened so
                    // far, so nothing is left half-open behind a pipeline that never ran.
                    drop(prepared);
                    running.stages = refuse(pipeline.len(), index, refusal);
                    return Ok(running);
                }
            }
        }

        for ready in prepared {
            let stage = self.spawn_stage(ready, &mut running, foreground);
            running.stages.push(stage);
        }
        Ok(running)
    }

    /// Resolves a stage and opens its redirections, without spawning anything.
    fn prepare_stage(
        &mut self,
        command: &Command,
        piped_input: Option<OwnedFd>,
        piped_output: Option<OwnedFd>,
    ) -> std::result::Result<ReadyStage, (ExitStatus, Error)> {
        let env = command.resolved_env();
        let path = resolve::effective_path(env.as_deref(), command.clears_env());
        let resolved = resolve::resolve(command.program(), path.as_deref(), command.directory());
        let program = match resolved {
            Resolution::Found(program) => program,
            other => {
                // Dropping the pipe ends here means the stages around this one see a clean end of
                // input rather than a hang — though none of them will be spawned at all.
                drop(piped_input);
                drop(piped_output);
                return Err(other.failure().unwrap_or((
                    ExitStatus::NOT_FOUND,
                    Error::new(
                        ono_core::ErrorCode::ResolveCommandNotFound,
                        "command not found",
                    ),
                )));
            }
        };

        let io = plan::prepare(command, piped_input, piped_output)
            .map_err(|error| (ExitStatus::FAILURE, error))?;

        Ok(ReadyStage {
            program,
            args: command.args_slice().to_vec(),
            env,
            clear_env: command.clears_env(),
            cwd: command.directory().map(std::path::Path::to_path_buf),
            io,
        })
    }

    /// Spawns a stage that has already been resolved and had its descriptors opened.
    fn spawn_stage(
        &mut self,
        ready: ReadyStage,
        running: &mut Running,
        foreground: bool,
    ) -> RunningStage {
        let ReadyStage {
            program,
            args,
            env,
            clear_env,
            cwd,
            io,
        } = ready;

        let request = SpawnRequest {
            program: &program,
            args: &args,
            env: env.as_deref(),
            clear_env,
            cwd: cwd.as_deref(),
            process_group: Some(running.pgid),
            controlling_terminal: None,
            // The child claims the terminal itself before `exec`, and the parent hands it over
            // as well (`run_foreground`). Either alone leaves a window: a child that reads
            // before the parent's handover is stopped by `SIGTTIN`, which is exactly the flake
            // CI caught in `terminal_control.rs`. Both together close it, which is what every
            // job-control shell does.
            claim_foreground: (foreground && self.terminal.is_interactive())
                .then(|| self.terminal.descriptor())
                .flatten(),
        };
        let spawned = spawn::spawn(&request, &io.plan);
        drop(io.plan);

        let pid = match spawned {
            Ok(pid) => pid,
            Err(error) => {
                let (status, error) = spawn::exec_failure(&program, &error);
                return RunningStage {
                    pid: 0,
                    state: JobState::Exited(status),
                    failure: Some(error),
                    stdout: None,
                    stderr: None,
                    pipe: None,
                    stdin: None,
                };
            }
        };
        if running.pgid == 0 {
            running.pgid = pid;
        }

        if let Some((sink, bytes)) = io.feed {
            feed(sink, bytes);
        }
        RunningStage {
            pid,
            state: JobState::Running,
            failure: None,
            stdout: io
                .stdout
                .map(|source| Collector::start_bounded(source, io.stdout_limit)),
            stderr: io.stderr.map(Collector::start),
            pipe: io.pipe,
            stdin: io.stdin,
        }
    }

    /// Turns a finished or stopped pipeline into the right kind of outcome.
    fn settle(&mut self, running: Running) -> Result<ForegroundOutcome> {
        if running.is_stopped() {
            let signal = match running.state() {
                JobState::Stopped(signal) => signal,
                _ => Signal::TSTP,
            };
            let id = self.register(running);
            return Ok(ForegroundOutcome::Stopped { job: id, signal });
        }
        let group = u32::try_from(running.pgid).unwrap_or(0);
        Ok(ForegroundOutcome::Completed(PipelineOutcome::new(
            collect(running),
            group,
        )))
    }

    /// Adds a pipeline to the job table under the lowest free job number.
    fn register(&mut self, running: Running) -> JobId {
        let id = JobId::new(self.free_number());
        let group = Arc::new(ForegroundGroup(std::sync::Mutex::new(running.pgid)));
        self.own(&group);
        self.jobs.push(Tracked { id, running, group });
        id
    }

    /// The lowest job number neither tracked nor reserved.
    fn free_number(&self) -> u32 {
        let mut number = 1;
        while self
            .jobs
            .iter()
            .any(|tracked| tracked.id.number() == number)
            || self.reserved.contains(&number)
        {
            number += 1;
        }
        number
    }

    /// Reserves the lowest free job number for a job this executor does not run.
    ///
    /// A native pipeline is not a process group, but its job lives in the same table the user
    /// addresses with `fg %N` (spec §18.4) — so the numbers come from one sequence, and `fg 2`
    /// can never be ambiguous between kinds.
    pub fn reserve_job_number(&mut self) -> u32 {
        let number = self.free_number();
        self.reserved.insert(number);
        number
    }

    /// Releases a number [`Executor::reserve_job_number`] handed out.
    pub fn release_job_number(&mut self, number: u32) {
        self.reserved.remove(&number);
    }

    fn locate(&self, id: JobId) -> Result<usize> {
        self.jobs
            .iter()
            .position(|tracked| tracked.id == id)
            .ok_or_else(|| {
                Error::new(
                    ono_core::ErrorCode::ResolveTargetNotFound,
                    format!("there is no job {id}"),
                )
            })
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::detached()
    }
}

/// A pipeline that has been started and not yet waited for.
pub struct Foreground {
    running: Running,
    owns_terminal: bool,
    /// The group's own cell, for a pipeline started without the terminal.
    cell: Option<Arc<ForegroundGroup>>,
}

impl std::fmt::Debug for Foreground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Foreground")
            .field("pgid", &self.running.pgid)
            .field("owns_terminal", &self.owns_terminal)
            .finish_non_exhaustive()
    }
}

impl Foreground {
    /// The read end of the last stage's [`Output::Pipe`](crate::Output::Pipe), the first time it is asked for.
    pub fn take_pipe(&mut self) -> Option<std::os::fd::OwnedFd> {
        self.running
            .stages
            .last_mut()
            .and_then(|stage| stage.pipe.take())
    }

    /// The write end of the first stage's [`Input::Pipe`](crate::Input::Pipe), the first time it
    /// is asked for. Dropping it is the end of the command's input.
    pub fn take_stdin(&mut self) -> Option<std::os::fd::OwnedFd> {
        self.running
            .stages
            .first_mut()
            .and_then(|stage| stage.stdin.take())
    }

    /// Why a stage could not be started, if one could not.
    #[must_use]
    pub fn failure(&self) -> Option<&Error> {
        self.running
            .stages
            .iter()
            .find_map(|stage| stage.failure.as_ref())
    }

    /// Whether a process of the pipeline has stopped — Ctrl-Z, `SIGTTIN` — without collecting
    /// the news: [`Executor::finish_foreground`] still sees the stop and files the pipeline as a
    /// stopped job, as it would have had the shell been waiting on it.
    #[must_use]
    pub fn has_stopped(&self) -> bool {
        if self.running.pgid <= 0 {
            return false;
        }
        matches!(
            waitid(
                Id::PGid(Pid::from_raw(self.running.pgid)),
                WaitPidFlag::WSTOPPED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT,
            ),
            Ok(WaitStatus::Stopped(..))
        )
    }

    /// Asks every process of the pipeline to stop (`SIGTERM` to the group).
    pub fn terminate(&self) {
        if self.running.pgid > 0 {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(self.running.pgid),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
    }
}

/// Waits for a foreground pipeline until it finishes or a stage stops.
///
/// News is waited for without being collected, and collected under the lock a [`Canceller`]
/// signals through: the group's last member stays a zombie, holding the group's id, until the
/// step that collects it also clears the foreground group (review R10).
fn wait_foreground(running: &mut Running, foreground: &ForegroundGroup) -> Result<()> {
    while !running.is_finished() && !running.is_stopped() {
        if running.pgid == 0 {
            break;
        }
        match waitid(
            Id::PGid(Pid::from_raw(running.pgid)),
            WaitPidFlag::WEXITED
                | WaitPidFlag::WSTOPPED
                | WaitPidFlag::WCONTINUED
                | WaitPidFlag::WNOWAIT,
        ) {
            Ok(_) | Err(Errno::EINTR | Errno::ECHILD) => {}
            Err(errno) => return Err(spawn::system("waiting for a job", errno)),
        }
        let mut group = foreground.lock();
        reap(running, false)?;
        if running.is_finished() && *group == running.pgid {
            *group = 0;
        }
    }
    Ok(())
}

/// Joins the capture threads and turns a finished pipeline into its stage outcomes.
fn collect(running: Running) -> Vec<StageOutcome> {
    running
        .stages
        .into_iter()
        .map(|stage| StageOutcome {
            pid: u32::try_from(stage.pid).unwrap_or(0),
            status: match stage.state {
                JobState::Exited(status) => status,
                JobState::Stopped(signal) => {
                    ExitStatus::from_signal(u8::try_from(signal.number()).unwrap_or(0))
                }
                JobState::Running => ExitStatus::SUCCESS,
            },
            stdout: stage.stdout.map(Collector::finish).unwrap_or_default(),
            stderr: stage.stderr.map(Collector::finish).unwrap_or_default(),
            failure: stage.failure,
        })
        .collect()
}

/// Writes the bytes a command was given as standard input, on a thread of its own.
fn feed(sink: OwnedFd, bytes: Vec<u8>) {
    std::thread::spawn(move || {
        let mut file = std::fs::File::from(sink);
        // A reader that stops early is normal — `head` does it — so a broken pipe is not an
        // error here; the child's own status already says what happened.
        let _ = std::io::Write::write_all(&mut file, &bytes);
    });
}

/// Collects everything `waitpid` has to say about a job's process group.
fn reap(running: &mut Running, block: bool) -> Result<()> {
    if running.pgid == 0 {
        return Ok(());
    }
    let mut flags = WaitPidFlag::WUNTRACED | WaitPidFlag::WCONTINUED;
    if !block {
        flags |= WaitPidFlag::WNOHANG;
    }
    loop {
        match waitpid(Pid::from_raw(-running.pgid), Some(flags)) {
            Ok(WaitStatus::StillAlive) => return Ok(()),
            Ok(WaitStatus::Exited(pid, code)) => {
                let status = ExitStatus::from_code(u8::try_from(code & 0xff).unwrap_or(1));
                running.record(pid.as_raw(), JobState::Exited(status));
            }
            Ok(WaitStatus::Signaled(pid, signal, _)) => {
                let status = ExitStatus::from_signal(u8::try_from(signal as i32).unwrap_or(0));
                running.record(pid.as_raw(), JobState::Exited(status));
            }
            Ok(WaitStatus::Stopped(pid, signal)) => {
                running.record(pid.as_raw(), JobState::Stopped(Signal::from_nix(signal)));
            }
            Ok(WaitStatus::Continued(pid)) => {
                running.record(pid.as_raw(), JobState::Running);
            }
            Ok(_) => {}
            Err(Errno::EINTR) => {}
            // Nothing left in the group: whatever we have not heard about is gone.
            Err(Errno::ECHILD) => {
                finish_unheard(running);
                return Ok(());
            }
            Err(errno) => return Err(spawn::system("waiting for a job", errno)),
        }
        if block && (running.is_finished() || running.is_stopped()) {
            return Ok(());
        }
        if !block && running.is_finished() {
            return Ok(());
        }
    }
}

/// A process the kernel no longer knows about cannot report a status of its own.
fn finish_unheard(running: &mut Running) {
    for stage in &mut running.stages {
        if !stage.state.is_final() {
            stage.state = JobState::Exited(ExitStatus::SUCCESS);
        }
    }
}

fn continue_group(running: &mut Running) -> Result<()> {
    if running.pgid == 0 {
        return Ok(());
    }
    signal_group(running.pgid, Signal::CONT)?;
    resume(running);
    Ok(())
}

fn resume(running: &mut Running) {
    for stage in &mut running.stages {
        if matches!(stage.state, JobState::Stopped(_)) {
            stage.state = JobState::Running;
        }
    }
}

fn signal_group(group: i32, signal: Signal) -> Result<()> {
    match killpg(Pid::from_raw(group), signal.to_nix()?) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        Err(errno) => Err(spawn::system("signalling a job", errno)),
    }
}
