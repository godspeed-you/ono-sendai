//! Changing the account database: the shadow-utils tools, driven by exit status (ADR-0101).
//!
//! `useradd`, `usermod`, `userdel`, `groupadd`, `groupmod`, `groupdel` and `gpasswd` are the
//! programs every Linux distribution ships to change `/etc/passwd`, `/etc/shadow`, `/etc/group`
//! and `/etc/gshadow` together, under the lock the whole system honours, with the distribution's
//! defaults (`/etc/login.defs`, `/etc/default/useradd`) and its SELinux, PAM and subordinate-id
//! hooks applied. Writing the files directly would be none of those things, so the provider
//! runs the tools instead.
//!
//! What it never does is read what they print. Each tool documents its **exit status** in its
//! man page — `4` for a uid already in use, `6` for an account that does not exist, `9` for a
//! name already taken — and that status is the whole of what the provider reads (spec §50).
//! Whatever the tool wrote to stderr travels along as `metadata.stderr`, for the user, unparsed.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

/// Where the account tools live when `PATH` does not say: an unprivileged `PATH` rarely
/// carries `sbin`, and a root shell's often does not either.
const TOOL_DIRECTORIES: [&str; 4] = ["/usr/sbin", "/sbin", "/usr/bin", "/bin"];

/// One change to the account database, as the tool invocation that makes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountCommand {
    program: &'static str,
    arguments: Vec<OsString>,
}

impl AccountCommand {
    fn new(program: &'static str) -> Self {
        Self {
            program,
            arguments: Vec::new(),
        }
    }

    fn arg(mut self, argument: impl AsRef<OsStr>) -> Self {
        self.arguments.push(argument.as_ref().to_owned());
        self
    }

    fn flag(self, flag: &str, value: Option<impl AsRef<OsStr>>) -> Self {
        match value {
            Some(value) => self.arg(flag).arg(value),
            None => self,
        }
    }

    /// `useradd [--uid N] [--home-dir P] [--shell P] [--gid G] NAME`.
    #[must_use]
    pub fn add_user(
        name: &str,
        uid: Option<u32>,
        home: Option<&Path>,
        shell: Option<&Path>,
        group: Option<&str>,
    ) -> Self {
        Self::new("useradd")
            .flag("--uid", uid.map(|uid| uid.to_string()))
            .flag("--home-dir", home)
            .flag("--shell", shell)
            .flag("--gid", group)
            .arg(name)
    }

    /// `userdel [--remove] NAME`.
    #[must_use]
    pub fn remove_user(name: &str, remove_home: bool) -> Self {
        let command = Self::new("userdel");
        if remove_home {
            command.arg("--remove").arg(name)
        } else {
            command.arg(name)
        }
    }

    /// `usermod [--shell P] [--home P] [--gid G] NAME`.
    ///
    /// The home directory is re-pointed, not moved: moving a tree is a separate, destructive
    /// decision the contract does not offer.
    #[must_use]
    pub fn set_user(
        name: &str,
        shell: Option<&Path>,
        home: Option<&Path>,
        group: Option<&str>,
    ) -> Self {
        Self::new("usermod")
            .flag("--shell", shell)
            .flag("--home", home)
            .flag("--gid", group)
            .arg(name)
    }

    /// `groupadd [--gid N] NAME`.
    #[must_use]
    pub fn add_group(name: &str, gid: Option<u32>) -> Self {
        Self::new("groupadd")
            .flag("--gid", gid.map(|gid| gid.to_string()))
            .arg(name)
    }

    /// `gpasswd --add USER GROUP`.
    #[must_use]
    pub fn add_member(group: &str, member: &str) -> Self {
        Self::new("gpasswd").arg("--add").arg(member).arg(group)
    }

    /// `groupdel NAME`.
    #[must_use]
    pub fn remove_group(name: &str) -> Self {
        Self::new("groupdel").arg(name)
    }

    /// `gpasswd --delete USER GROUP`.
    #[must_use]
    pub fn remove_member(group: &str, member: &str) -> Self {
        Self::new("gpasswd").arg("--delete").arg(member).arg(group)
    }

    /// `groupmod --gid N NAME`.
    #[must_use]
    pub fn set_group(name: &str, gid: u32) -> Self {
        Self::new("groupmod")
            .arg("--gid")
            .arg(gid.to_string())
            .arg(name)
    }

    /// The program this command runs.
    #[must_use]
    pub fn program(&self) -> &str {
        self.program
    }

    /// Runs the tool and reads its exit status.
    ///
    /// # Errors
    ///
    /// `provider.unavailable` when the tool is not installed; otherwise the structured form of
    /// the documented exit status (see [`outcome_of`]).
    pub async fn run(&self) -> Result<(), ErrorValue> {
        let program = locate(self.program).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!(
                    "`{}` is not installed, so the account database cannot be changed here",
                    self.program
                ),
            )
            .with_help("the shadow-utils package provides it on every major distribution")
        })?;
        let output = tokio::process::Command::new(program)
            .args(&self.arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|error| {
                ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    format!("`{}` could not be started: {error}", self.program),
                )
            })?;
        let outcome = match output.status.code() {
            Some(0) => return Ok(()),
            Some(status) => outcome_of(self.program, status),
            None => ErrorValue::new(
                ErrorCode::ExternalSignal,
                format!(
                    "`{}` was killed by signal {}",
                    self.program,
                    output.status.signal().unwrap_or_default()
                ),
            ),
        };
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim();
        if said.is_empty() {
            Err(outcome)
        } else {
            Err(outcome.with_metadata("stderr", Value::string(said)))
        }
    }
}

impl fmt::Display for AccountCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.program)?;
        for argument in &self.arguments {
            write!(f, " {}", argument.to_string_lossy())?;
        }
        Ok(())
    }
}

/// The structured meaning of a non-zero exit status, as the shadow-utils man pages document it.
///
/// The statuses are shared across the tools: `1` and `10` are "can't update the password/group
/// file", `2` and `3` are a bad invocation or argument, `4` a uid/gid in use, `6` an account
/// that does not exist, `8` an account that is in use, `9` a name already taken, `12` a home
/// directory that could not be created or removed. Anything else is reported as the number it
/// is, never guessed at.
#[must_use]
pub fn outcome_of(program: &str, status: i32) -> ErrorValue {
    let (code, meaning) = match status {
        4 => (
            ErrorCode::IoAlreadyExists,
            "the numeric id is already in use",
        ),
        9 => (ErrorCode::IoAlreadyExists, "the name is already in use"),
        6 => (ErrorCode::IoNotFound, "the account does not exist"),
        2 | 3 => (
            ErrorCode::TypeMismatch,
            "the tool rejected an argument as invalid",
        ),
        1 | 10 => (
            ErrorCode::ExternalExitNonzero,
            "the account database could not be updated",
        ),
        8 => (
            ErrorCode::ExternalExitNonzero,
            "the account is in use and cannot be removed",
        ),
        12 => (
            ErrorCode::ExternalExitNonzero,
            "the home directory could not be created or removed",
        ),
        _ => (ErrorCode::ExternalExitNonzero, "the tool failed"),
    };
    ErrorValue::new(
        code,
        format!("`{program}` exited with status {status}: {meaning}"),
    )
    .with_metadata("program", Value::string(program))
    .with_metadata("exit_status", Value::Int(i128::from(status)))
}

/// The tool on `PATH`, or in the directories the distributions install it to.
fn locate(program: &str) -> Option<PathBuf> {
    let on_path = std::env::var_os("PATH").map(|path| {
        std::env::split_paths(&path)
            .map(|directory| directory.join(program))
            .collect::<Vec<_>>()
    });
    on_path
        .unwrap_or_default()
        .into_iter()
        .chain(
            TOOL_DIRECTORIES
                .iter()
                .map(|directory| Path::new(directory).join(program)),
        )
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_spell_the_shadow_utils_invocation_the_contract_options_map_to() {
        let command = AccountCommand::add_user(
            "deploy",
            Some(4242),
            Some(Path::new("/srv/deploy")),
            Some(Path::new("/usr/bin/ono")),
            Some("staff"),
        );
        assert_eq!(
            command.to_string(),
            "useradd --uid 4242 --home-dir /srv/deploy --shell /usr/bin/ono --gid staff deploy"
        );
        assert_eq!(
            AccountCommand::remove_user("deploy", true).to_string(),
            "userdel --remove deploy"
        );
        assert_eq!(
            AccountCommand::set_user("deploy", Some(Path::new("/bin/false")), None, None)
                .to_string(),
            "usermod --shell /bin/false deploy"
        );
        assert_eq!(
            AccountCommand::add_member("docker", "deploy").to_string(),
            "gpasswd --add deploy docker"
        );
        assert_eq!(
            AccountCommand::set_group("docker", 999).to_string(),
            "groupmod --gid 999 docker"
        );
    }

    #[test]
    fn should_read_the_documented_exit_statuses_as_taxonomy_codes() {
        assert_eq!(outcome_of("useradd", 9).code(), ErrorCode::IoAlreadyExists);
        assert_eq!(outcome_of("groupadd", 4).code(), ErrorCode::IoAlreadyExists);
        assert_eq!(outcome_of("userdel", 6).code(), ErrorCode::IoNotFound);
        assert_eq!(outcome_of("usermod", 3).code(), ErrorCode::TypeMismatch);
        assert_eq!(
            outcome_of("userdel", 8).code(),
            ErrorCode::ExternalExitNonzero
        );
        let unknown = outcome_of("gpasswd", 42);
        assert_eq!(unknown.code(), ErrorCode::ExternalExitNonzero);
        assert!(
            unknown.message().contains("42"),
            "an undocumented status is reported as the number it is"
        );
    }
}
