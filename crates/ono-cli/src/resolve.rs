//! Name resolution, in the order ADR-0011 fixes and `explain` reports.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_value::ErrorValue;

use crate::session::Session;

/// What a head word resolved to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// A command the shell implements itself, because no child process could.
    Builtin(&'static str),
    /// An external program at this absolute path.
    External(PathBuf),
}

/// The namespaces a head may force (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Namespace {
    /// No namespace given: the full order applies.
    Any,
    /// `ono:` — native commands only.
    Native,
    /// `exec:` — external executables only.
    External,
    /// `fn:` — user functions only.
    Function,
}

impl Namespace {
    /// Reads a namespace prefix, or `None` if it names no known namespace.
    #[must_use]
    pub fn from_prefix(prefix: Option<&str>) -> Option<Self> {
        match prefix {
            None => Some(Namespace::Any),
            Some("ono") => Some(Namespace::Native),
            Some("exec") => Some(Namespace::External),
            Some("fn") => Some(Namespace::Function),
            Some(_) => None,
        }
    }
}

/// The commands the shell must implement itself.
///
/// Every one of these changes the shell's own state, which a child process cannot do: a `cd` in a
/// subprocess moves a directory nobody is standing in.
pub const BUILTINS: &[&str] = &[
    "cd", "exit", "set", "remove", "jobs", "fg", "bg", "help", "explain", "true", "false",
];

/// The builtin a head word names, given the word after it — or `None` when the stage is not the
/// shell's to run.
///
/// `set` and `remove` are builtins only for the state that lives in the shell: `set env`,
/// `set config` and `remove env` (ADR-0010, ADR-0020 §9). `set file`, `remove file`, `set
/// service` and every other target are native commands the registry answers for
/// (`docs/spec/commands/`), so they resolve like `stop process` does — a bound implementation, or
/// the honest E0101 — and they may stand in a pipeline (ADR-0068).
#[must_use]
pub fn builtin_for(name: &str, first_argument: Option<&str>) -> Option<&'static str> {
    let builtin = BUILTINS
        .iter()
        .find(|candidate| **candidate == name)
        .copied()?;
    let shell_owned = match builtin {
        "set" => matches!(first_argument, Some("env" | "config")),
        "remove" => first_argument == Some("env"),
        _ => true,
    };
    shell_owned.then_some(builtin)
}

/// Resolves `name` in `namespace`, following the order of ADR-0011.
///
/// A forced namespace that misses is never retried elsewhere: forcing a namespace is a statement
/// of intent, and quietly resolving it another way would defeat the purpose.
pub fn resolve(
    session: &Session,
    namespace: Namespace,
    name: &str,
) -> Result<Resolution, ErrorValue> {
    let builtin = BUILTINS
        .iter()
        .find(|candidate| **candidate == name)
        .copied();

    match namespace {
        Namespace::Native => builtin
            .map(Resolution::Builtin)
            .ok_or_else(|| not_found(name, "ono:")),
        Namespace::Function => Err(not_found(name, "fn:")),
        Namespace::External => find_on_path(session, name)
            .map(Resolution::External)
            .ok_or_else(|| not_found(name, "exec:")),
        Namespace::Any => {
            if let Some(builtin) = builtin {
                return Ok(Resolution::Builtin(builtin));
            }
            find_on_path(session, name)
                .map(Resolution::External)
                .ok_or_else(|| not_found(name, ""))
        }
    }
}

fn not_found(name: &str, namespace: &str) -> ErrorValue {
    let help = if namespace.is_empty() {
        "no keyword, native command or executable on PATH answers to this name".to_owned()
    } else {
        format!("`{namespace}` resolves in that namespace only, and is never retried elsewhere")
    };
    ErrorValue::new(
        ErrorCode::ResolveCommandNotFound,
        format!("command not found: {namespace}{name}"),
    )
    .with_help(help)
}

/// Finds `name` on `PATH`, or treats it as a path when it contains a separator.
///
/// A name containing `/` is never searched for, exactly as in every other shell: `./build.sh` is
/// a path, not a command that happens to look like one.
#[must_use]
pub fn find_on_path(session: &Session, name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let candidate = if Path::new(name).is_absolute() {
            PathBuf::from(name)
        } else {
            session.cwd().join(name)
        };
        return candidate.is_file().then_some(candidate);
    }

    find_in(session.env_var("PATH")?, session.cwd(), name)
}

/// A resolver that answers as [`find_on_path`] would, from a snapshot of `PATH` and the working
/// directory, for callers that outlive the borrow of the session: `type` planning inside a
/// running pipeline, and completion (ADR-0067).
#[must_use]
pub fn resolver(session: &Session) -> ono_command::Resolver {
    let path = session.env_var("PATH").map(std::ffi::OsStr::to_os_string);
    let cwd = session.cwd().to_path_buf();
    std::sync::Arc::new(move |name: &str| {
        if name.contains('/') {
            let candidate = if Path::new(name).is_absolute() {
                PathBuf::from(name)
            } else {
                cwd.join(name)
            };
            return candidate.is_file().then_some(candidate);
        }
        find_in(path.as_deref()?, &cwd, name)
    })
}

fn find_in(path: &std::ffi::OsStr, cwd: &Path, name: &str) -> Option<PathBuf> {
    for directory in std::env::split_paths(path) {
        let candidate = search_directory_in(cwd, &directory).join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// One `PATH` entry, resolved against the directory the shell is actually standing in.
///
/// An empty entry means the working directory, as everywhere else. A *relative* entry means a
/// directory relative to the working directory — and it is the shell's, not the process's. Those
/// differ the moment `cd` runs, and when they did, `explain foo` reported one binary while `foo`
/// ran another: the shell stat'd the entry against the process's directory and then spawned the
/// child in the session's. A resolution report that does not describe the resolution is worse
/// than none, because ADR-0015 T11 makes it the only defence against a shadowing binary.
fn search_directory(session: &Session, entry: &Path) -> PathBuf {
    search_directory_in(session.cwd(), entry)
}

fn search_directory_in(cwd: &Path, entry: &Path) -> PathBuf {
    if entry.as_os_str().is_empty() {
        return cwd.to_path_buf();
    }
    if entry.is_relative() {
        return cwd.join(entry);
    }
    entry.to_path_buf()
}

/// Whether the path is a file anyone could plausibly execute.
///
/// The executable bit is checked here so that resolution reports "not found" for a data file on
/// `PATH` rather than handing it to the executor to fail as "not executable" — the two mean
/// different things to a user and to ADR-0008's statuses.
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Every candidate a completion or a suggestion could offer for `prefix`.
#[must_use]
pub fn candidates(session: &Session, prefix: &str) -> Vec<String> {
    let mut found: Vec<String> = BUILTINS
        .iter()
        .filter(|name| name.starts_with(prefix))
        .map(|name| (*name).to_owned())
        .collect();

    if let Some(path) = session.env_var("PATH") {
        for entry in std::env::split_paths(path) {
            let directory = search_directory(session, &entry);
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with(prefix) && is_executable_file(&entry.path()) {
                    found.push(name.into_owned());
                }
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// The names closest to `name`, for the suggestion path of `resolve.command_not_found`.
///
/// Computed only when a command was not found, so it costs nothing when one was (ADR-0011).
#[must_use]
pub fn suggestions(session: &Session, name: &str) -> Vec<String> {
    let mut scored: Vec<(usize, String)> = candidates(session, "")
        .into_iter()
        .filter_map(|candidate| {
            let distance = edit_distance(name, &candidate);
            // A suggestion further than a third of the word away is noise, not help.
            (distance <= name.len().div_ceil(3).max(1)).then_some((distance, candidate))
        })
        .collect();
    scored.sort();
    scored.truncate(3);
    scored.into_iter().map(|(_, candidate)| candidate).collect()
}

/// Levenshtein distance, two rows at a time.
fn edit_distance(left: &str, right: &str) -> usize {
    let right_chars: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right_chars.len()).collect();
    let mut current = vec![0usize; right_chars.len() + 1];

    for (i, left_char) in left.chars().enumerate() {
        current[0] = i + 1;
        for (j, right_char) in right_chars.iter().enumerate() {
            let substitution = previous[j] + usize::from(left_char != *right_char);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right_chars.len()]
}

/// The program name an external resolution should be spawned as.
#[must_use]
pub fn program_of(resolution: &Resolution) -> &OsStr {
    match resolution {
        Resolution::External(path) => path.as_os_str(),
        Resolution::Builtin(name) => OsStr::new(name),
    }
}
