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
    /// Serve this machine's providers (spec §21.2): the remote end of a link, over stdin/stdout
    /// or over the authenticated transport of §21.5 when `--listen` says where.
    Agent(Options, AgentOptions),
    /// Print this shell's own peer fingerprint and exit (v0.4.1 §8.5).
    ///
    /// The non-secret half of the identity of §8.1, and the canonical spelling: the same
    /// fingerprint `--agent --print-host-key` prints, asked for without claiming to be an agent.
    PrintPeerKey,
    /// Print the version and exit.
    Version,
    /// Print usage and exit.
    Help,
    /// The command line could not be understood.
    Usage(String),
}

/// What `--agent` was asked to serve, and with which identity (spec §21.4, §21.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentOptions {
    /// The address to listen on for Ono's own authenticated transport; `None` serves
    /// stdin/stdout, which is what an ssh-carried agent reads.
    pub listen: Option<String>,
    /// The file the host identity lives in, overriding the configuration directory's.
    pub host_key: Option<PathBuf>,
    /// Print this host's key fingerprint and exit — what a person pins the host by.
    pub print_host_key: bool,
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
                "--print-peer-key" => {
                    rest.next();
                    return Self::PrintPeerKey;
                }
                "--agent" => {
                    rest.next();
                    let mut agent = AgentOptions::default();
                    while let Some(argument) = rest.next() {
                        match argument.as_str() {
                            "--listen" => match rest.next() {
                                Some(address) => agent.listen = Some(address),
                                None => {
                                    return Self::Usage("--listen needs an address".to_owned());
                                }
                            },
                            "--host-key" => match rest.next() {
                                Some(path) => agent.host_key = Some(PathBuf::from(path)),
                                None => return Self::Usage("--host-key needs a path".to_owned()),
                            },
                            "--print-host-key" => agent.print_host_key = true,
                            other => {
                                return Self::Usage(format!(
                                    "unrecognised arguments after --agent: {other}"
                                ));
                            }
                        }
                    }
                    return Self::Agent(options, agent);
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
