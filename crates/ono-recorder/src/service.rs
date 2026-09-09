//! The user service of §10.9, and the guarantee its unit carries (v0.5 §10.5, §10.9).
//!
//! §10.9: "the reference Linux implementation SHOULD support a user service named
//! `ono-recorder.service`. The command layer MAY start/stop this through the existing service
//! mechanisms or a dedicated recorder controller, but user-facing semantics remain
//! `start recorder` and `stop recorder`."
//!
//! Two consequences, and they are the whole of this module:
//!
//! - **The unit is an implementation of the verb, never a second interface to it.** Nothing here
//!   is user-facing; `start recorder` reaches [`ServiceControl`] when a user manager is there and
//!   [`crate::Recorder`] directly when it is not. A host without systemd still records — the
//!   recorder is a user-level collector, not a daemon requirement.
//! - **Every invocation is `--user`.** §10.5 forbids running "a privileged system daemon merely
//!   to increase visibility", so there is no code path in this module that can reach the system
//!   manager, and the unit itself says `NoNewPrivileges=yes` so the guarantee survives a
//!   `systemctl` a user runs by hand.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ono_value::ErrorValue;

/// The unit name §10.9 fixes.
pub const SERVICE_UNIT: &str = "ono-recorder.service";

/// The argument every invocation begins with (§10.5).
const USER_SCOPE: &str = "--user";

/// What the unit runs, and with what (§10.9).
#[derive(Debug, Clone)]
pub struct UnitOptions {
    executable: PathBuf,
    arguments: Vec<String>,
    description: String,
}

impl UnitOptions {
    /// A unit running `executable` with the argument that keeps the recorder in the foreground.
    #[must_use]
    pub fn new(executable: &Path) -> Self {
        Self {
            executable: executable.to_path_buf(),
            arguments: vec![DEFAULT_ARGUMENT.to_owned()],
            description: "Ono-Sendai temporal recorder".to_owned(),
        }
    }

    /// The same unit with arguments of the caller's own.
    #[must_use]
    pub fn with_arguments<'a>(mut self, arguments: impl IntoIterator<Item = &'a str>) -> Self {
        self.arguments = arguments.into_iter().map(str::to_owned).collect();
        self
    }

    /// The `ExecStart` line, as the unit spells it.
    #[must_use]
    pub fn exec_start(&self) -> String {
        let mut line = self.executable.display().to_string();
        for argument in &self.arguments {
            line.push(' ');
            line.push_str(argument);
        }
        line
    }
}

/// The argument the unit passes so the binary runs the recorder rather than a shell.
pub const DEFAULT_ARGUMENT: &str = "--recorder-service";

/// The systemd user unit of §10.9.
///
/// `NoNewPrivileges=yes` is §10.5 written where the service manager enforces it, and there is no
/// `User=`: a user unit runs as the user by construction, and naming one would be the beginning
/// of the privileged daemon §10.5 forbids.
#[must_use]
pub fn unit_file(options: &UnitOptions) -> String {
    format!(
        "[Unit]\n\
         Description={description}\n\
         Documentation=man:ono(1)\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec_start}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         NoNewPrivileges=yes\n\
         PrivateTmp=yes\n\
         ProtectKernelTunables=yes\n\
         ProtectControlGroups=yes\n\
         RestrictSUIDSGID=yes\n\
         MemoryMax=256M\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        description = options.description,
        exec_start = options.exec_start(),
    )
}

/// Where a user unit lives, resolved the way the rest of the tree resolves XDG.
#[must_use]
pub fn unit_path(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    let base = match env("XDG_CONFIG_HOME") {
        Some(config) if !config.is_empty() => PathBuf::from(config),
        _ => home?.join(".config"),
    };
    Some(base.join("systemd").join("user").join(SERVICE_UNIT))
}

/// Whether this host has a systemd user manager to talk to (§10.9).
///
/// The evidence is the user manager's own socket under `XDG_RUNTIME_DIR`. A host without one is
/// not an error: the recorder runs in process, which is what §10.9's "MAY" leaves open.
#[must_use]
pub fn systemd_available(env: impl Fn(&str) -> Option<String>) -> bool {
    let Some(runtime) = env("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) else {
        return false;
    };
    Path::new(&runtime).join("systemd").join("private").exists()
}

/// What running one service command produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRun {
    /// The exit status the service manager gave.
    pub status: i32,
    /// What it said, trimmed.
    pub output: String,
}

impl ServiceRun {
    /// Whether the command succeeded.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        self.status == 0
    }
}

/// Something that can run a service command (§10.9).
pub trait ServiceRunner: std::fmt::Debug {
    /// Runs the service manager with `arguments`.
    ///
    /// # Errors
    ///
    /// Returns a §34 refusal where the manager cannot be reached.
    fn run(&self, arguments: &[&str]) -> Result<ServiceRun, ErrorValue>;
}

/// The real `systemctl --user`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Systemctl;

impl ServiceRunner for Systemctl {
    fn run(&self, arguments: &[&str]) -> Result<ServiceRun, ErrorValue> {
        let output = std::process::Command::new("systemctl")
            .args(arguments)
            .output()
            .map_err(|error| {
                ono_temporal_core::error::store_unavailable(&format!(
                    "`systemctl {}` could not be run: {error}",
                    arguments.join(" ")
                ))
            })?;
        Ok(ServiceRun {
            status: output.status.code().unwrap_or(-1),
            output: String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        })
    }
}

/// A runner that records what it was asked to do, for a test with no service manager.
#[derive(Debug, Default)]
pub struct RecordingRunner {
    calls: Mutex<Vec<Vec<String>>>,
    answer: Option<String>,
}

impl RecordingRunner {
    /// A runner answering `answer` to every query.
    #[must_use]
    pub fn answering(answer: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            answer: Some(answer.to_owned()),
        }
    }

    /// Everything it has been asked to run, in order.
    #[must_use]
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl ServiceRunner for RecordingRunner {
    fn run(&self, arguments: &[&str]) -> Result<ServiceRun, ErrorValue> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(
                arguments
                    .iter()
                    .map(|argument| (*argument).to_owned())
                    .collect(),
            );
        }
        Ok(ServiceRun {
            status: 0,
            output: self.answer.clone().unwrap_or_else(|| "active".to_owned()),
        })
    }
}

/// Drives `ono-recorder.service` as the user's own service (§10.9).
#[derive(Debug)]
pub struct ServiceControl<'runner> {
    runner: &'runner dyn ServiceRunner,
}

impl<'runner> ServiceControl<'runner> {
    /// A control over `runner`.
    #[must_use]
    pub const fn new(runner: &'runner dyn ServiceRunner) -> Self {
        Self { runner }
    }

    /// `systemctl --user start ono-recorder.service`.
    ///
    /// # Errors
    ///
    /// Returns a §34 refusal where the user manager cannot be reached.
    pub fn start(&self) -> Result<ServiceRun, ErrorValue> {
        self.runner.run(&[USER_SCOPE, "start", SERVICE_UNIT])
    }

    /// `systemctl --user stop ono-recorder.service`.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub fn stop(&self) -> Result<ServiceRun, ErrorValue> {
        self.runner.run(&[USER_SCOPE, "stop", SERVICE_UNIT])
    }

    /// `systemctl --user is-active ono-recorder.service`.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub fn is_active(&self) -> Result<bool, ErrorValue> {
        let run = self.runner.run(&[USER_SCOPE, "is-active", SERVICE_UNIT])?;
        Ok(run.output == "active")
    }

    /// `systemctl --user enable ono-recorder.service`.
    ///
    /// # Errors
    ///
    /// As [`Self::start`].
    pub fn enable(&self) -> Result<ServiceRun, ErrorValue> {
        self.runner.run(&[USER_SCOPE, "enable", SERVICE_UNIT])
    }
}

/// Writes the user unit where a systemd user manager will read it (§10.9).
///
/// # Errors
///
/// Returns a §34 refusal where the file cannot be written.
pub fn install_unit(path: &Path, unit: &str) -> Result<(), ErrorValue> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            ono_temporal_core::error::store_unavailable(&format!(
                "the unit directory `{}` could not be created: {error}",
                parent.display()
            ))
        })?;
    }
    std::fs::write(path, unit).map_err(|error| {
        ono_temporal_core::error::store_unavailable(&format!(
            "the unit `{}` could not be written: {error}",
            path.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_reach_only_the_user_manager_when_the_service_is_driven() {
        let runner = RecordingRunner::default();
        let control = ServiceControl::new(&runner);
        control.start().expect("a start");
        control.enable().expect("an enable");

        for call in runner.calls() {
            assert_eq!(call.first().map(String::as_str), Some(USER_SCOPE));
        }
    }
}
