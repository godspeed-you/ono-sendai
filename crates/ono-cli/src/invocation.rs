//! How `ono` was started (ADR-0010).
//!
//! Argument handling happens before any I/O, so that `--help` and `--version` cost nothing and a
//! non-interactive invocation never touches a terminal it was not given.

use std::path::PathBuf;

/// What the shell was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Read from the terminal until the user leaves.
    Interactive(Options),
    /// Run the given source, then exit with its status.
    Source(String, Options),
    /// Run a script file, then exit with its status.
    Script(PathBuf, Vec<String>, Options),
    /// Read a script from standard input.
    Stdin(Options),
    /// Serve this machine's providers over stdin/stdout (spec §21.2): the remote end of a link.
    Agent(Options),
    /// Print the version and exit.
    Version,
    /// Print usage and exit.
    Help,
    /// The command line could not be understood.
    Usage(String),
}

/// Settings taken from the command line rather than from configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// Skip every configuration layer below the command line (ADR-0010).
    pub no_config: bool,
    /// A configuration file replacing the system and user layers.
    pub config: Option<PathBuf>,
}

impl Invocation {
    /// Reads the command line, which is `args` without the program name.
    ///
    /// ```
    /// # use ono_cli::invocation::Invocation;
    /// let invocation = Invocation::from_args(["-c", "echo hi"].map(String::from));
    /// assert!(matches!(invocation, Invocation::Source(source, _) if source == "echo hi"));
    /// ```
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut options = Options::default();
        let mut rest = args.into_iter().peekable();

        while let Some(argument) = rest.peek().cloned() {
            match argument.as_str() {
                "--version" | "-V" => {
                    rest.next();
                    return Self::Version;
                }
                "--help" | "-h" => {
                    rest.next();
                    return Self::Help;
                }
                "--agent" => {
                    rest.next();
                    return Self::Agent(options);
                }
                "--no-config" => {
                    rest.next();
                    options.no_config = true;
                }
                "--config" => {
                    rest.next();
                    match rest.next() {
                        Some(path) => options.config = Some(PathBuf::from(path)),
                        None => return Self::Usage("--config needs a path".to_owned()),
                    }
                }
                "-c" => {
                    rest.next();
                    return match rest.next() {
                        Some(source) => Self::Source(source, options),
                        None => Self::Usage("-c needs a command to run".to_owned()),
                    };
                }
                "-" => {
                    rest.next();
                    return Self::Stdin(options);
                }
                "--" => {
                    rest.next();
                    break;
                }
                flag if flag.starts_with('-') && flag.len() > 1 => {
                    return Self::Usage(format!("unrecognised arguments: {flag}"));
                }
                _ => break,
            }
        }

        match rest.next() {
            Some(path) => Self::Script(PathBuf::from(path), rest.collect(), options),
            None => Self::Interactive(options),
        }
    }
}
