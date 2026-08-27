//! Running the shell the way a user runs it, and seeing what a user sees.

use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use ono_core::ExitStatus;

/// The default budget for a single run. A shell test that hangs stops the whole suite, so every
/// run has a deadline even when the test does not ask for one.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);

/// A shell invocation under construction.
#[derive(Debug, Clone)]
pub struct Shell {
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, Option<String>)>,
    cwd: Option<PathBuf>,
    stdin: Vec<u8>,
    timeout: Duration,
    clear_env: bool,
}

impl Shell {
    /// An invocation of the `ono` binary of the current build profile.
    #[must_use]
    pub fn new() -> Self {
        Self::program(crate::ono_binary())
    }

    /// An invocation of an arbitrary program, used to verify the harness itself.
    #[must_use]
    pub fn program(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            stdin: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            clear_env: false,
        }
    }

    /// Appends command-line arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Sets an environment variable for the run.
    #[must_use]
    pub fn env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((name.into(), Some(value.into())));
        self
    }

    /// Removes an environment variable from the run.
    #[must_use]
    pub fn env_remove(mut self, name: impl Into<String>) -> Self {
        self.env.push((name.into(), None));
        self
    }

    /// Starts the run with an empty environment, so a test cannot accidentally depend on the
    /// developer machine's settings.
    #[must_use]
    pub fn clear_env(mut self) -> Self {
        self.clear_env = true;
        self
    }

    /// Runs in `directory`.
    #[must_use]
    pub fn cwd(mut self, directory: impl Into<PathBuf>) -> Self {
        self.cwd = Some(directory.into());
        self
    }

    /// Feeds `input` to the run's standard input, then closes it.
    #[must_use]
    pub fn stdin(mut self, input: impl AsRef<[u8]>) -> Self {
        self.stdin = input.as_ref().to_vec();
        self
    }

    /// Overrides the run's deadline.
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs, and returns the outcome.
    ///
    /// # Panics
    ///
    /// Panics if the program cannot be started or does not finish within its budget. A test that
    /// wants to observe either uses [`Shell::try_run`].
    #[must_use]
    pub fn run(self) -> Run {
        match self.try_run() {
            Ok(run) => run,
            Err(error) => panic!("{error}"),
        }
    }

    /// Runs, reporting a failure to start or a run that overran its budget as an error.
    ///
    /// # Errors
    ///
    /// Returns [`RunError::Spawn`] if the program could not be started and [`RunError::Timeout`]
    /// if it did not finish within the configured budget.
    pub fn try_run(self) -> Result<Run, RunError> {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if self.clear_env {
            command.env_clear();
        }
        for (name, value) in &self.env {
            match value {
                Some(value) => command.env(name, value),
                None => command.env_remove(name),
            };
        }
        if let Some(directory) = &self.cwd {
            command.current_dir(directory);
        }

        let mut child = command.spawn().map_err(|error| RunError::Spawn {
            program: self.program.clone(),
            message: error.to_string(),
        })?;

        if let Some(mut pipe) = child.stdin.take() {
            let input = self.stdin.clone();
            // Writing on this thread would deadlock against a child that fills its output pipe
            // before reading all of its input.
            std::thread::spawn(move || {
                let _ = pipe.write_all(&input);
            });
        }

        // `wait_with_output` has no deadline of its own, so the wait happens on a worker and the
        // test thread enforces the budget.
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(child.wait_with_output());
        });

        let output = match receiver.recv_timeout(self.timeout) {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                return Err(RunError::Spawn {
                    program: self.program.clone(),
                    message: error.to_string(),
                });
            }
            Err(_) => {
                return Err(RunError::Timeout {
                    program: self.program.clone(),
                    args: self.args.clone(),
                    timeout: self.timeout,
                });
            }
        };

        let status = match output.status.code() {
            Some(code) => ExitStatus::from_code(u8::try_from(code).unwrap_or(255)),
            None => {
                let signal = signal_of(&output.status);
                ExitStatus::from_signal(signal)
            }
        };

        Ok(Run {
            status,
            stdout: output.stdout,
            stderr: output.stderr,
            command: format!("{} {}", self.program.display(), self.args.join(" ")),
        })
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(unix)]
fn signal_of(status: &std::process::ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    u8::try_from(status.signal().unwrap_or(0)).unwrap_or(0)
}

#[cfg(not(unix))]
fn signal_of(_status: &std::process::ExitStatus) -> u8 {
    0
}

/// What a run produced: exactly what a user would have seen.
#[derive(Debug, Clone)]
pub struct Run {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    command: String,
}

impl Run {
    /// The exit status, following the contract of ADR-0008.
    #[must_use]
    pub fn status(&self) -> ExitStatus {
        self.status
    }

    /// Standard output as text.
    ///
    /// # Panics
    ///
    /// Panics if the output is not valid UTF-8; use [`Run::stdout_bytes`] for byte output.
    #[must_use]
    pub fn stdout(&self) -> &str {
        std::str::from_utf8(&self.stdout).expect("standard output must be valid UTF-8")
    }

    /// Standard error as text.
    ///
    /// # Panics
    ///
    /// Panics if the output is not valid UTF-8; use [`Run::stderr_bytes`] for byte output.
    #[must_use]
    pub fn stderr(&self) -> &str {
        std::str::from_utf8(&self.stderr).expect("standard error must be valid UTF-8")
    }

    /// Standard output as raw bytes.
    #[must_use]
    pub fn stdout_bytes(&self) -> &[u8] {
        &self.stdout
    }

    /// Standard error as raw bytes.
    #[must_use]
    pub fn stderr_bytes(&self) -> &[u8] {
        &self.stderr
    }

    /// Both streams, in the order a terminal would have interleaved them well enough for an
    /// assertion about "what the user saw".
    #[must_use]
    pub fn output(&self) -> String {
        format!("{}{}", self.stdout(), self.stderr())
    }

    /// Asserts the run succeeded, reporting both streams when it did not.
    ///
    /// # Panics
    ///
    /// Panics if the exit status is not zero.
    pub fn assert_success(&self) -> &Self {
        assert!(
            self.status.is_success(),
            "`{}` failed with status {}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.command,
            self.status,
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        );
        self
    }

    /// Asserts the run failed with `code`, reporting both streams when it did not.
    ///
    /// # Panics
    ///
    /// Panics if the exit status differs.
    pub fn assert_status(&self, code: u8) -> &Self {
        assert_eq!(
            self.status.code(),
            code,
            "`{}` exited {} rather than {code}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.command,
            self.status,
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        );
        self
    }

    /// Asserts that standard output contains `needle`.
    ///
    /// # Panics
    ///
    /// Panics if it does not.
    pub fn assert_stdout_contains(&self, needle: &str) -> &Self {
        assert!(
            self.stdout().contains(needle),
            "`{}` did not print {needle:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.command,
            self.stdout(),
            self.stderr()
        );
        self
    }
}

/// Why a run could not be observed.
#[derive(Debug, Clone)]
pub enum RunError {
    /// The program could not be started.
    Spawn {
        /// The program that could not be started.
        program: PathBuf,
        /// What the operating system said.
        message: String,
    },
    /// The program did not finish within its budget.
    Timeout {
        /// The program that overran.
        program: PathBuf,
        /// The arguments it was given.
        args: Vec<String>,
        /// The budget it exceeded.
        timeout: Duration,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RunError::Spawn { program, message } => {
                write!(f, "cannot run {}: {message}", program.display())
            }
            RunError::Timeout {
                program,
                args,
                timeout,
            } => write!(
                f,
                "`{} {}` did not finish within {timeout:?}",
                program.display(),
                args.join(" ")
            ),
        }
    }
}

impl std::error::Error for RunError {}

impl AsRef<Path> for crate::Scratch {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}
