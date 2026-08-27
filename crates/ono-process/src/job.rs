//! The job table of spec §18.1: what the shell knows about the work it started.

use std::fmt;
use std::os::fd::OwnedFd;
use std::thread::JoinHandle;

use ono_core::ExitStatus;

use crate::error::Error;
use crate::signals::Signal;

/// The shell's handle on a job, the `%1` of `fg %1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(u32);

impl JobId {
    pub(crate) const fn new(number: u32) -> Self {
        Self(number)
    }

    /// The job's number, as the user types it.
    #[must_use]
    pub const fn number(self) -> u32 {
        self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

/// What a job, or one process in it, is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Running, in the foreground or the background.
    Running,
    /// Stopped by the given signal, and continuable.
    Stopped(Signal),
    /// Finished, with the status of ADR-0008.
    Exited(ExitStatus),
}

impl JobState {
    /// Whether the job has finished and will not change again.
    #[must_use]
    pub const fn is_final(self) -> bool {
        matches!(self, Self::Exited(_))
    }
}

impl fmt::Display for JobState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Running => f.write_str("running"),
            Self::Stopped(signal) => write!(f, "stopped ({signal})"),
            Self::Exited(status) => write!(f, "done ({status})"),
        }
    }
}

/// One process inside a job — that is, one stage of its pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobProcess {
    /// The process id, or `0` if the stage never started.
    pub pid: u32,
    /// What this process is doing.
    pub state: JobState,
    /// Why this stage could not be started, if it could not be.
    pub failure: Option<Error>,
}

/// A job as the shell renders it: everything `jobs` needs and nothing it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// The job's number.
    pub id: JobId,
    /// The process group every process in the job belongs to.
    pub pgid: u32,
    /// The command line the job was started from.
    pub command: String,
    /// The job's overall state.
    pub state: JobState,
    /// The processes making up the job, in pipeline order.
    pub processes: Vec<JobProcess>,
}

impl fmt::Display for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.id, self.state, self.command)
    }
}

/// A state change the job table observed while reaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobChange {
    /// The job that changed.
    pub id: JobId,
    /// The state it was in before.
    pub previous: JobState,
    /// The state it is in now.
    pub current: JobState,
}

/// A pipeline the shell has started and is still responsible for.
pub(crate) struct Running {
    pub(crate) pgid: i32,
    pub(crate) command: String,
    pub(crate) stages: Vec<RunningStage>,
}

/// One stage of a running pipeline, with whatever the parent still holds for it.
pub(crate) struct RunningStage {
    pub(crate) pid: i32,
    pub(crate) state: JobState,
    pub(crate) failure: Option<Error>,
    pub(crate) stdout: Option<Collector>,
    pub(crate) stderr: Option<Collector>,
    /// The read end of the pipe a caller reads standard output from, until it takes it.
    pub(crate) pipe: Option<OwnedFd>,
}

/// A thread draining one captured stream.
pub(crate) struct Collector(JoinHandle<Vec<u8>>);

impl Collector {
    /// Starts a thread that reads `source` to end of file.
    pub(crate) fn start(source: OwnedFd) -> Self {
        Self(std::thread::spawn(move || {
            let mut file = std::fs::File::from(source);
            let mut collected = Vec::new();
            // A capture that fails mid-stream keeps what it already read: losing the prefix
            // would be worse than reporting a short read.
            let _ = std::io::Read::read_to_end(&mut file, &mut collected);
            collected
        }))
    }

    /// Waits for the thread and returns the bytes.
    pub(crate) fn finish(self) -> Vec<u8> {
        self.0.join().unwrap_or_default()
    }
}

impl Running {
    /// The job's overall state: stopped if any process is, finished when all are.
    pub(crate) fn state(&self) -> JobState {
        if let Some(signal) = self.stages.iter().find_map(|stage| match stage.state {
            JobState::Stopped(signal) => Some(signal),
            _ => None,
        }) {
            return JobState::Stopped(signal);
        }
        if self.stages.iter().all(|stage| stage.state.is_final()) {
            return JobState::Exited(self.status());
        }
        JobState::Running
    }

    /// The pipeline status: the last stage's (ADR-0008).
    pub(crate) fn status(&self) -> ExitStatus {
        match self.stages.last().map(|stage| stage.state) {
            Some(JobState::Exited(status)) => status,
            _ => ExitStatus::SUCCESS,
        }
    }

    /// Whether every process has finished.
    pub(crate) fn is_finished(&self) -> bool {
        self.stages.iter().all(|stage| stage.state.is_final())
    }

    /// Whether any process is stopped.
    pub(crate) fn is_stopped(&self) -> bool {
        self.stages
            .iter()
            .any(|stage| matches!(stage.state, JobState::Stopped(_)))
    }

    /// Records what `waitpid` reported about one of the job's processes.
    pub(crate) fn record(&mut self, pid: i32, state: JobState) {
        for stage in &mut self.stages {
            if stage.pid == pid {
                stage.state = state;
            }
        }
    }

    /// The public snapshot of this job.
    pub(crate) fn snapshot(&self, id: JobId) -> Job {
        Job {
            id,
            pgid: u32::try_from(self.pgid).unwrap_or(0),
            command: self.command.clone(),
            state: self.state(),
            processes: self
                .stages
                .iter()
                .map(|stage| JobProcess {
                    pid: u32::try_from(stage.pid).unwrap_or(0),
                    state: stage.state,
                    failure: stage.failure.clone(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(states: &[JobState]) -> Running {
        Running {
            pgid: 100,
            command: "test".to_owned(),
            stages: states
                .iter()
                .enumerate()
                .map(|(index, state)| RunningStage {
                    pid: 100 + i32::try_from(index).unwrap_or(0),
                    state: *state,
                    failure: None,
                    stdout: None,
                    stderr: None,
                    pipe: None,
                })
                .collect(),
        }
    }

    #[test]
    fn should_report_a_job_as_stopped_when_any_process_is_stopped() {
        let job = running(&[
            JobState::Exited(ExitStatus::SUCCESS),
            JobState::Stopped(Signal::TSTP),
        ]);
        assert_eq!(job.state(), JobState::Stopped(Signal::TSTP));
        assert!(job.is_stopped());
        assert!(!job.is_finished());
    }

    #[test]
    fn should_report_the_last_stage_status_when_every_process_has_finished() {
        let job = running(&[
            JobState::Exited(ExitStatus::from_code(1)),
            JobState::Exited(ExitStatus::from_code(7)),
        ]);
        assert_eq!(job.state(), JobState::Exited(ExitStatus::from_code(7)));
        assert!(job.is_finished());
    }

    #[test]
    fn should_report_a_job_as_running_while_any_process_still_runs() {
        let job = running(&[JobState::Exited(ExitStatus::SUCCESS), JobState::Running]);
        assert_eq!(job.state(), JobState::Running);
    }

    #[test]
    fn should_render_a_job_the_way_a_prompt_would() {
        let job = running(&[JobState::Running]).snapshot(JobId::new(2));
        assert_eq!(job.to_string(), "%2 running test");
    }
}
