//! Pipelines of external commands and the outcome of running one.

use std::fmt;

use ono_core::ExitStatus;

use crate::command::Command;
use crate::error::Error;

/// A sequence of external commands connected by real pipes (spec §11, §12.5).
///
/// ```
/// use ono_process::{Command, Pipeline};
/// let pipeline = Pipeline::new()
///     .stage(Command::new("journalctl").arg("-o").arg("json"))
///     .stage(Command::new("head").arg("-20"));
/// assert_eq!(pipeline.len(), 2);
/// assert_eq!(pipeline.to_string(), "journalctl -o json | head -20");
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pipeline {
    stages: Vec<Command>,
}

impl Pipeline {
    /// An empty pipeline, which runs nothing and succeeds.
    #[must_use]
    pub const fn new() -> Self {
        Self { stages: Vec::new() }
    }

    /// Appends a stage.
    #[must_use]
    pub fn stage(mut self, command: Command) -> Self {
        self.stages.push(command);
        self
    }

    /// The stages, in order.
    #[must_use]
    pub fn stages(&self) -> &[Command] {
        &self.stages
    }

    /// How many stages the pipeline has.
    #[must_use]
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    /// Whether the pipeline has no stages at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }
}

impl FromIterator<Command> for Pipeline {
    fn from_iter<T: IntoIterator<Item = Command>>(iter: T) -> Self {
        Self {
            stages: iter.into_iter().collect(),
        }
    }
}

impl fmt::Display for Pipeline {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, stage) in self.stages.iter().enumerate() {
            if index > 0 {
                f.write_str(" | ")?;
            }
            write!(f, "{stage}")?;
        }
        Ok(())
    }
}

/// What one stage of a pipeline did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageOutcome {
    /// The process id the stage ran as, or `0` if it never started.
    pub pid: u32,
    /// The stage's exit status, passed through unchanged from the child (spec §16.4).
    pub status: ExitStatus,
    /// The stage's standard output, if it was captured.
    pub stdout: Vec<u8>,
    /// The stage's standard error, if it was captured.
    pub stderr: Vec<u8>,
    /// Why the stage could not be started, if it could not be.
    pub failure: Option<Error>,
}

/// What a whole pipeline did.
///
/// Every stage's status is retained. The pipeline's own status is the last stage's, and the
/// vector is kept rather than collapsed, so no hidden `pipefail` mode is needed (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOutcome {
    stages: Vec<StageOutcome>,
    process_group: u32,
}

impl PipelineOutcome {
    pub(crate) fn new(stages: Vec<StageOutcome>, process_group: u32) -> Self {
        Self {
            stages,
            process_group,
        }
    }

    /// What each stage did, in pipeline order.
    #[must_use]
    pub fn stages(&self) -> &[StageOutcome] {
        &self.stages
    }

    /// The status of every stage, in pipeline order.
    #[must_use]
    pub fn statuses(&self) -> Vec<ExitStatus> {
        self.stages.iter().map(|stage| stage.status).collect()
    }

    /// The pipeline's status: the last stage's, or success for an empty pipeline.
    #[must_use]
    pub fn status(&self) -> ExitStatus {
        self.stages
            .last()
            .map_or(ExitStatus::SUCCESS, |stage| stage.status)
    }

    /// The last stage's captured standard output, or nothing if it was not captured.
    #[must_use]
    pub fn stdout(&self) -> &[u8] {
        self.stages.last().map_or(&[], |stage| &stage.stdout)
    }

    /// The last stage's captured standard error, or nothing if it was not captured.
    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        self.stages.last().map_or(&[], |stage| &stage.stderr)
    }

    /// The process group every stage ran in, or `0` if nothing started.
    #[must_use]
    pub const fn process_group(&self) -> u32 {
        self.process_group
    }

    /// The first stage that could not be started at all, if any.
    #[must_use]
    pub fn failure(&self) -> Option<&Error> {
        self.stages.iter().find_map(|stage| stage.failure.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_take_the_last_stage_status_as_the_pipeline_status() {
        let outcome = PipelineOutcome::new(vec![stage(1), stage(2), stage(3)], 42);
        assert_eq!(outcome.status(), ExitStatus::from_code(3));
        assert_eq!(
            outcome.statuses(),
            vec![
                ExitStatus::from_code(1),
                ExitStatus::from_code(2),
                ExitStatus::from_code(3)
            ]
        );
    }

    #[test]
    fn should_report_success_for_an_empty_pipeline() {
        let outcome = PipelineOutcome::new(Vec::new(), 0);
        assert_eq!(outcome.status(), ExitStatus::SUCCESS);
        assert!(outcome.stdout().is_empty());
    }

    #[test]
    fn should_render_a_pipeline_the_way_it_was_written() {
        let pipeline = Pipeline::new()
            .stage(Command::new("ps").arg("aux"))
            .stage(Command::new("grep").arg("ono"));
        assert_eq!(pipeline.to_string(), "ps aux | grep ono");
    }

    fn stage(code: u8) -> StageOutcome {
        StageOutcome {
            pid: u32::from(code),
            status: ExitStatus::from_code(code),
            stdout: Vec::new(),
            stderr: Vec::new(),
            failure: None,
        }
    }
}
