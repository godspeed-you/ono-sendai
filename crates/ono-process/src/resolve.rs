//! Resolving a program name to something the kernel can execute (spec §29, ADR-0008).

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use ono_core::{ErrorCode, ExitStatus};

use crate::error::Error;

/// What `PATH` means when nothing sets it, as POSIX describes the default.
const DEFAULT_PATH: &str = "/bin:/usr/bin";

/// The outcome of looking a program name up.
pub(crate) enum Resolution {
    /// The path to hand to `execve`.
    Found(PathBuf),
    /// Something with that name exists but cannot be executed: status 126.
    NotExecutable(Error),
    /// Nothing with that name exists: status 127.
    NotFound(Error),
}

impl Resolution {
    /// The status and structured error a failed resolution reports.
    pub(crate) fn failure(self) -> Option<(ExitStatus, Error)> {
        match self {
            Self::Found(_) => None,
            Self::NotExecutable(error) => Some((ExitStatus::NOT_EXECUTABLE, error)),
            Self::NotFound(error) => Some((ExitStatus::NOT_FOUND, error)),
        }
    }
}

/// Looks `program` up the way a shell does.
///
/// A name containing `/` is a path and is never searched for on `PATH`; a relative one is taken
/// relative to `cwd`, the directory the command will run in. Anything else is searched for on
/// `path`, and an entry that exists but cannot be executed is remembered so that "found but not
/// executable" (126) stays distinguishable from "no such command" (127).
pub(crate) fn resolve(program: &OsStr, path: Option<&OsStr>, cwd: Option<&Path>) -> Resolution {
    if program.is_empty() {
        return Resolution::NotFound(Error::new(
            ErrorCode::ResolveCommandNotFound,
            "no command was given",
        ));
    }

    if program.as_bytes().contains(&b'/') {
        let candidate = Path::new(program);
        let probe = match cwd {
            Some(dir) if candidate.is_relative() => dir.join(candidate),
            _ => candidate.to_path_buf(),
        };
        return match classify(&probe) {
            Candidate::Executable => Resolution::Found(candidate.to_path_buf()),
            Candidate::NotExecutable => Resolution::NotExecutable(not_executable(program)),
            Candidate::Missing => Resolution::NotFound(not_found(program)),
        };
    }

    let search = path.map_or_else(|| OsString::from(DEFAULT_PATH), OsStr::to_os_string);
    let mut found_but_unusable = false;
    for entry in search.as_bytes().split(|byte| *byte == b':') {
        let directory = if entry.is_empty() {
            PathBuf::from(".")
        } else {
            PathBuf::from(OsStr::from_bytes(entry))
        };
        let candidate = directory.join(Path::new(program));
        let probe = match cwd {
            Some(dir) if candidate.is_relative() => dir.join(&candidate),
            _ => candidate.clone(),
        };
        match classify(&probe) {
            Candidate::Executable => return Resolution::Found(candidate),
            Candidate::NotExecutable => found_but_unusable = true,
            Candidate::Missing => {}
        }
    }

    if found_but_unusable {
        Resolution::NotExecutable(not_executable(program))
    } else {
        Resolution::NotFound(not_found(program))
    }
}

enum Candidate {
    Executable,
    NotExecutable,
    Missing,
}

fn classify(path: &Path) -> Candidate {
    let Ok(metadata) = std::fs::metadata(path) else {
        return Candidate::Missing;
    };
    if !metadata.is_file() {
        return Candidate::NotExecutable;
    }
    match nix::unistd::access(path, nix::unistd::AccessFlags::X_OK) {
        Ok(()) if is_executable_format(path) => Candidate::Executable,
        _ => Candidate::NotExecutable,
    }
}

/// Whether the kernel, or the `ENOEXEC` fallback to `/bin/sh`, can make sense of the file.
///
/// A file the execute bit says yes to can still be nothing the machine can run. `execvp` hands
/// such a file to `/bin/sh` rather than reporting `ENOEXEC`, which turns "cannot execute binary
/// file" into whatever the shell happens to say about its first line. ADR-0008 wants that case
/// to be status 126, so the header is checked here, in the parent, where it can be reported:
/// a shebang or an ELF header is for the kernel, text without either is for `/bin/sh`, and
/// anything binary is not executable at all.
fn is_executable_format(path: &Path) -> bool {
    use std::io::Read;

    let mut head = [0u8; 128];
    let Ok(mut file) = std::fs::File::open(path) else {
        // Unreadable but executable is a real combination; let the kernel have the last word.
        return true;
    };
    let Ok(read) = file.read(&mut head) else {
        return true;
    };
    let head = &head[..read];
    if head.starts_with(b"#!") || head.starts_with(b"\x7fELF") {
        return true;
    }
    !head.contains(&0)
}

fn not_found(program: &OsStr) -> Error {
    Error::new(
        ErrorCode::ResolveCommandNotFound,
        format!("{}: command not found", program.to_string_lossy()),
    )
}

fn not_executable(program: &OsStr) -> Error {
    Error::new(
        ErrorCode::IoPermissionDenied,
        format!("{}: not executable", program.to_string_lossy()),
    )
}

/// The effective `PATH` a command runs with, after its own environment changes.
pub(crate) fn effective_path(
    changes: Option<&[(OsString, Option<OsString>)]>,
    clears: bool,
) -> Option<OsString> {
    if let Some(changes) = changes {
        for (key, value) in changes {
            if key == "PATH" {
                return value.clone();
            }
        }
    }
    if clears {
        return None;
    }
    std::env::var_os("PATH")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_find_a_program_on_the_search_path() {
        let found = resolve(OsStr::new("sh"), Some(OsStr::new("/bin:/usr/bin")), None);
        assert!(matches!(found, Resolution::Found(path) if path == Path::new("/bin/sh")));
    }

    #[test]
    fn should_report_not_found_when_no_entry_holds_the_program() {
        let missing = resolve(
            OsStr::new("ono-no-such-program-42"),
            Some(OsStr::new("/bin:/usr/bin")),
            None,
        );
        let (status, error) = missing.failure().expect("resolution must fail");
        assert_eq!(status, ExitStatus::NOT_FOUND);
        assert_eq!(error.code(), ErrorCode::ResolveCommandNotFound);
    }

    #[test]
    fn should_report_not_executable_when_the_path_names_a_directory() {
        let directory = resolve(OsStr::new("/tmp"), None, None);
        let (status, error) = directory.failure().expect("resolution must fail");
        assert_eq!(status, ExitStatus::NOT_EXECUTABLE);
        assert_eq!(error.code(), ErrorCode::IoPermissionDenied);
    }

    #[test]
    fn should_never_search_the_path_for_a_name_containing_a_slash() {
        let relative = resolve(
            OsStr::new("./sh"),
            Some(OsStr::new("/bin:/usr/bin")),
            Some(Path::new("/bin")),
        );
        assert!(matches!(relative, Resolution::Found(path) if path == Path::new("./sh")));

        let absent = resolve(
            OsStr::new("./sh"),
            Some(OsStr::new("/bin:/usr/bin")),
            Some(Path::new("/")),
        );
        assert!(matches!(absent, Resolution::NotFound(_)));
    }

    #[test]
    fn should_fall_back_to_the_posix_default_when_there_is_no_path() {
        let found = resolve(OsStr::new("sh"), None, None);
        assert!(matches!(found, Resolution::Found(_)));
    }

    #[test]
    fn should_report_no_path_when_the_environment_removed_it() {
        let changes = vec![(OsString::from("PATH"), None)];
        assert_eq!(effective_path(Some(&changes), false), None);
    }

    #[test]
    fn should_use_the_assigned_path_when_the_environment_set_one() {
        let changes = vec![(OsString::from("PATH"), Some(OsString::from("/opt/bin")))];
        assert_eq!(
            effective_path(Some(&changes), false),
            Some(OsString::from("/opt/bin"))
        );
    }
}
