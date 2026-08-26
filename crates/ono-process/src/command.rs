//! The description of one external command, and the redirections applied to it.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::path::{Path, PathBuf};

use crate::fd::Fd;
use crate::pipeline::Pipeline;

/// Where a command's standard input comes from, before redirections are applied.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Input {
    /// The shell's own standard input, unchanged.
    #[default]
    Inherit,
    /// An immediate end of input.
    Null,
    /// Bytes the shell supplies, written into a pipe as the command reads them.
    Bytes(Vec<u8>),
}

/// Where one of a command's output streams goes, before redirections are applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Output {
    /// The shell's own stream, unchanged.
    #[default]
    Inherit,
    /// Discarded.
    Null,
    /// Collected into memory and returned on the [`StageOutcome`](crate::StageOutcome).
    Capture,
}

/// One change to the environment the command runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EnvChange {
    Set(OsString, OsString),
    Unset(OsString),
    Clear,
}

/// A redirection, in the forms spec §12.5 keeps familiar.
///
/// Redirections are applied left to right, and each one sees the state the earlier ones left
/// behind — so `> file 2>&1` sends both streams to the file, while `2>&1 > file` does not.
///
/// ```
/// use ono_process::{Fd, Redirect};
/// let both = [Redirect::write("out.log"), Redirect::duplicate(Fd::STDERR, Fd::STDOUT)];
/// assert_eq!(both.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redirect {
    /// `N< path` — open `path` for reading on descriptor `fd`.
    Read {
        /// The descriptor being redirected.
        fd: Fd,
        /// The file to read.
        path: PathBuf,
    },
    /// `N> path` — open `path` for writing on descriptor `fd`, truncating it.
    Write {
        /// The descriptor being redirected.
        fd: Fd,
        /// The file to write.
        path: PathBuf,
    },
    /// `N>> path` — open `path` for writing on descriptor `fd`, appending to it.
    Append {
        /// The descriptor being redirected.
        fd: Fd,
        /// The file to append to.
        path: PathBuf,
    },
    /// `N>&M` and `N<&M` — make descriptor `fd` a copy of descriptor `from`.
    Duplicate {
        /// The descriptor being redirected.
        fd: Fd,
        /// The descriptor it becomes a copy of.
        from: Fd,
    },
}

impl Redirect {
    /// `< path`.
    #[must_use]
    pub fn read(path: impl AsRef<Path>) -> Self {
        Self::read_from(Fd::STDIN, path)
    }

    /// `> path`.
    #[must_use]
    pub fn write(path: impl AsRef<Path>) -> Self {
        Self::write_to(Fd::STDOUT, path)
    }

    /// `>> path`.
    #[must_use]
    pub fn append(path: impl AsRef<Path>) -> Self {
        Self::append_to(Fd::STDOUT, path)
    }

    /// `N< path`.
    #[must_use]
    pub fn read_from(fd: Fd, path: impl AsRef<Path>) -> Self {
        Self::Read {
            fd,
            path: path.as_ref().to_path_buf(),
        }
    }

    /// `N> path`.
    #[must_use]
    pub fn write_to(fd: Fd, path: impl AsRef<Path>) -> Self {
        Self::Write {
            fd,
            path: path.as_ref().to_path_buf(),
        }
    }

    /// `N>> path`.
    #[must_use]
    pub fn append_to(fd: Fd, path: impl AsRef<Path>) -> Self {
        Self::Append {
            fd,
            path: path.as_ref().to_path_buf(),
        }
    }

    /// `N>&M` or `N<&M`.
    #[must_use]
    pub const fn duplicate(fd: Fd, from: Fd) -> Self {
        Self::Duplicate { fd, from }
    }

    /// The descriptor this redirection changes.
    #[must_use]
    pub const fn target(&self) -> Fd {
        match self {
            Self::Read { fd, .. }
            | Self::Write { fd, .. }
            | Self::Append { fd, .. }
            | Self::Duplicate { fd, .. } => *fd,
        }
    }
}

/// One external command: the program to run and everything that shapes how it runs.
///
/// The builder takes and returns `self`, so a command reads like the line it came from.
///
/// ```
/// use ono_process::{Command, Output};
/// let command = Command::new("grep")
///     .arg("-c")
///     .arg("warning")
///     .stdout(Output::Capture);
/// assert_eq!(command.to_string(), "grep -c warning");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    program: OsString,
    args: Vec<OsString>,
    env: Vec<EnvChange>,
    cwd: Option<PathBuf>,
    stdin: Input,
    stdout: Output,
    stderr: Output,
    redirects: Vec<Redirect>,
}

impl Command {
    /// A command that runs `program` with no arguments.
    ///
    /// A program name containing `/` is used as a path; anything else is looked up on `PATH`
    /// when the command runs.
    #[must_use]
    pub fn new(program: impl AsRef<OsStr>) -> Self {
        Self {
            program: program.as_ref().to_os_string(),
            args: Vec::new(),
            env: Vec::new(),
            cwd: None,
            stdin: Input::Inherit,
            stdout: Output::Inherit,
            stderr: Output::Inherit,
            redirects: Vec::new(),
        }
    }

    /// Appends one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl AsRef<OsStr>) -> Self {
        self.args.push(arg.as_ref().to_os_string());
        self
    }

    /// Appends several arguments.
    #[must_use]
    pub fn args(mut self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Self {
        self.args
            .extend(args.into_iter().map(|arg| arg.as_ref().to_os_string()));
        self
    }

    /// Sets a variable in the command's environment.
    #[must_use]
    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env.push(EnvChange::Set(
            key.as_ref().to_os_string(),
            value.as_ref().to_os_string(),
        ));
        self
    }

    /// Removes a variable from the command's environment, inherited or not.
    #[must_use]
    pub fn env_remove(mut self, key: impl AsRef<OsStr>) -> Self {
        self.env.push(EnvChange::Unset(key.as_ref().to_os_string()));
        self
    }

    /// Starts from an empty environment instead of the shell's.
    #[must_use]
    pub fn env_clear(mut self) -> Self {
        self.env.push(EnvChange::Clear);
        self
    }

    /// Runs the command in `dir`.
    #[must_use]
    pub fn current_dir(mut self, dir: impl AsRef<Path>) -> Self {
        self.cwd = Some(dir.as_ref().to_path_buf());
        self
    }

    /// Sets where standard input comes from, before redirections.
    #[must_use]
    pub fn stdin(mut self, input: Input) -> Self {
        self.stdin = input;
        self
    }

    /// Sets where standard output goes, before redirections.
    #[must_use]
    pub fn stdout(mut self, output: Output) -> Self {
        self.stdout = output;
        self
    }

    /// Sets where standard error goes, before redirections.
    #[must_use]
    pub fn stderr(mut self, output: Output) -> Self {
        self.stderr = output;
        self
    }

    /// Appends a redirection. Redirections apply in the order they were added.
    #[must_use]
    pub fn redirect(mut self, redirect: Redirect) -> Self {
        self.redirects.push(redirect);
        self
    }

    /// The program name as written.
    #[must_use]
    pub fn program(&self) -> &OsStr {
        &self.program
    }

    /// The arguments, not including the program name.
    #[must_use]
    pub fn args_slice(&self) -> &[OsString] {
        &self.args
    }

    /// The directory the command runs in, if it is not the shell's.
    #[must_use]
    pub fn directory(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// The redirections, in the order they apply.
    #[must_use]
    pub fn redirects(&self) -> &[Redirect] {
        &self.redirects
    }

    pub(crate) const fn input(&self) -> &Input {
        &self.stdin
    }

    pub(crate) const fn output(&self) -> Output {
        self.stdout
    }

    pub(crate) const fn error_output(&self) -> Output {
        self.stderr
    }

    /// The environment the command should run in, resolved against `inherited`.
    ///
    /// Returns `None` for "inherit everything" so the common case costs nothing.
    pub(crate) fn resolved_env(&self) -> Option<Vec<(OsString, Option<OsString>)>> {
        if self.env.is_empty() {
            return None;
        }
        let mut changes = Vec::new();
        for change in &self.env {
            match change {
                EnvChange::Clear => changes.clear(),
                EnvChange::Set(key, value) => {
                    changes.retain(|(existing, _)| existing != key);
                    changes.push((key.clone(), Some(value.clone())));
                }
                EnvChange::Unset(key) => {
                    changes.retain(|(existing, _)| existing != key);
                    changes.push((key.clone(), None));
                }
            }
        }
        Some(changes)
    }

    /// Whether the command starts from an empty environment.
    pub(crate) fn clears_env(&self) -> bool {
        self.env.contains(&EnvChange::Clear)
    }
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", quote(&self.program))?;
        for arg in &self.args {
            write!(f, " {}", quote(arg))?;
        }
        for redirect in &self.redirects {
            match redirect {
                Redirect::Read { fd, path } => write!(f, " {fd}<{}", path.display())?,
                Redirect::Write { fd, path } => write!(f, " {fd}>{}", path.display())?,
                Redirect::Append { fd, path } => write!(f, " {fd}>>{}", path.display())?,
                Redirect::Duplicate { fd, from } => write!(f, " {fd}>&{from}")?,
            }
        }
        Ok(())
    }
}

impl From<Command> for Pipeline {
    fn from(command: Command) -> Self {
        Self::new().stage(command)
    }
}

/// Renders one word the way a shell would have to write it to mean the same thing again.
fn quote(word: &OsStr) -> String {
    let text = word.to_string_lossy();
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || "-_./:=@+,".contains(c));
    if plain {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_render_a_command_as_a_line_that_means_the_same_thing() {
        let command = Command::new("grep").arg("-c").arg("a b").arg("f.txt");
        assert_eq!(command.to_string(), "grep -c 'a b' f.txt");
    }

    #[test]
    fn should_render_redirections_in_the_order_they_apply() {
        let command = Command::new("cmd")
            .redirect(Redirect::write("out"))
            .redirect(Redirect::duplicate(Fd::STDERR, Fd::STDOUT));
        assert_eq!(command.to_string(), "cmd 1>out 2>&1");
    }

    #[test]
    fn should_let_a_later_environment_change_replace_an_earlier_one() {
        let command = Command::new("cmd").env("A", "1").env_remove("A");
        assert_eq!(
            command.resolved_env(),
            Some(vec![(OsString::from("A"), None)])
        );
    }

    #[test]
    fn should_forget_earlier_changes_when_the_environment_is_cleared() {
        let command = Command::new("cmd").env("A", "1").env_clear().env("B", "2");
        assert_eq!(
            command.resolved_env(),
            Some(vec![(OsString::from("B"), Some(OsString::from("2")))])
        );
        assert!(command.clears_env());
    }

    #[test]
    fn should_inherit_the_whole_environment_when_nothing_was_changed() {
        assert_eq!(Command::new("cmd").resolved_env(), None);
    }
}
