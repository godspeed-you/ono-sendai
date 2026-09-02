//! The `ono` binary.

#![forbid(unsafe_code)]

use std::io::IsTerminal;
use std::process::ExitCode;

use ono_cli::invocation::{Invocation, Options};
use ono_cli::report::Reporter;
use ono_cli::session::Session;
use ono_cli::{config, repl};
use ono_core::ExitStatus;
use ono_render::Presentation;

fn main() -> ExitCode {
    let invocation = Invocation::from_args(std::env::args().skip(1));

    let status = match invocation {
        Invocation::Version => {
            println!("{} {}", ono_core::SHORT_NAME, ono_core::VERSION);
            ExitStatus::SUCCESS
        }
        Invocation::Help => {
            println!("{}", ono_cli::usage_text());
            ExitStatus::SUCCESS
        }
        Invocation::Usage(message) => {
            eprintln!("{}: {message}", ono_core::SHORT_NAME);
            eprintln!("try `{} --help`", ono_core::SHORT_NAME);
            ExitStatus::USAGE
        }
        Invocation::PrintPeerKey => {
            // What a person pins this machine by, on stdout so it can be read by a script or
            // copied into `add host-key` on the machine that will link here (v0.4.1 §8.5). The
            // fingerprint is the public contract of §7.2; the key it names never leaves the file.
            match default_identity() {
                Ok(identity) => {
                    println!("{}", identity.fingerprint());
                    ExitStatus::SUCCESS
                }
                Err(error) => {
                    eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
                    if let Some(help) = error.help() {
                        eprintln!("{}: {help}", ono_core::SHORT_NAME);
                    }
                    ExitStatus::FAILURE
                }
            }
        }
        Invocation::Agent(_, agent_options) => {
            // The remote end of a link (spec §21.2): serve this machine's providers over
            // stdin/stdout. The transport already carried authentication (ADR-0037), and the
            // agent talks the protocol and nothing else — no prompt, no terminal, no config
            // execution surface.
            let environment: Vec<(String, String)> = std::env::vars().collect();
            let mut registry = ono_cli::providers::registry(environment);
            let runtime = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_name("ono-agent")
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    eprintln!(
                        "{}: cannot start the agent runtime: {error}",
                        ono_core::SHORT_NAME
                    );
                    return ExitCode::from(ExitStatus::FAILURE);
                }
            };
            runtime.block_on(ono_cli::providers::register_async(&mut registry));
            // The agent negotiates and runs adapters on its own side (spec v0.3 §1.54): the
            // bundled packs, probing versions the way the shell does.
            let adapters = std::sync::Arc::new(ono_adapter::Registry::bundled(Box::new(
                ono_cli::session::probe_version,
            )));
            let config = ono_remote::AgentConfig::new(std::sync::Arc::new(registry))
                .with_identity(ono_protocol::Identity::new(
                    std::env::var("USER").unwrap_or_else(|_| "ono".to_owned()),
                ))
                .with_adapters(adapters);
            let identity = match agent_identity(&agent_options) {
                Ok(identity) => identity,
                Err(error) => {
                    eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
                    return ExitCode::from(ExitStatus::FAILURE);
                }
            };
            if agent_options.print_host_key {
                // What a person pins this host by, on stdout so it can be read by a script or
                // copied into `add host-key` on the machine that will link here (spec §21.5).
                println!("{}", identity.fingerprint());
                return ExitCode::from(ExitStatus::SUCCESS);
            }
            if let Some(address) = &agent_options.listen {
                return runtime.block_on(serve_authenticated(address, &identity, config));
            }
            return runtime.block_on(ono_remote::agent_main(
                tokio::io::stdin(),
                tokio::io::stdout(),
                config,
            ));
        }
        Invocation::Source(source, options) => {
            let (mut session, reporter) = start(false, &options);
            let status = repl::run_source(&mut session, &source, &reporter);
            session.leaving().unwrap_or(status)
        }
        Invocation::Stdin(options) => {
            let (mut session, reporter) = start(false, &options);
            let status =
                repl::run_from_reader(&mut session, &reporter, &mut std::io::stdin().lock());
            session.leaving().unwrap_or(status)
        }
        Invocation::Script(path, arguments, options) => {
            let (mut session, reporter) = start(false, &options);
            match std::fs::read_to_string(&path) {
                Ok(source) => {
                    let values: Vec<ono_value::Value> = arguments
                        .into_iter()
                        .map(|argument| ono_value::Value::String(argument.into()))
                        .collect();
                    session.bind("args", ono_value::Value::List(values.into()));
                    let status = repl::run_source(&mut session, &source, &reporter);
                    session.leaving().unwrap_or(status)
                }
                Err(error) => {
                    reporter.error(&ono_cli::builtin::io_error(&path, &error));
                    ExitStatus::FAILURE
                }
            }
        }
        Invocation::Interactive(options) => {
            // `ono` with no arguments means "read commands from standard input". When standard
            // input is a terminal that is a conversation; when it is a pipe or a file it is a
            // script, and starting a prompt would read the terminal instead and silently ignore
            // what was piped in. Every other shell makes the same distinction.
            if std::io::stdin().is_terminal() {
                let (mut session, reporter) = start(true, &options);
                repl::run(&mut session, &options, &reporter)
            } else {
                let (mut session, reporter) = start(false, &options);
                let status =
                    repl::run_from_reader(&mut session, &reporter, &mut std::io::stdin().lock());
                session.leaving().unwrap_or(status)
            }
        }
    };

    ExitCode::from(status)
}

/// Builds the session and reads configuration (ADR-0010).
fn start(interactive: bool, options: &Options) -> (Session, Reporter) {
    let mut session = Session::new(interactive);
    let presentation = Presentation::choose(
        std::io::stderr().is_terminal(),
        &[
            (
                "NO_COLOR",
                session
                    .env_var("NO_COLOR")
                    .and_then(|v| v.to_str())
                    .unwrap_or(""),
            ),
            (
                "TERM",
                session
                    .env_var("TERM")
                    .and_then(|v| v.to_str())
                    .unwrap_or(""),
            ),
        ],
    );
    let presentation = if session.env_var("NO_COLOR").is_none() {
        presentation
    } else {
        Presentation::Plain
    };
    let reporter = Reporter::new(presentation);
    config::load(&mut session, options, &reporter);
    // The theme is only known once the configuration has been read, and the reporter that read
    // it had to exist first — so the one the session keeps is themed afterwards (ADR-0332).
    let reporter = reporter.with_theme(session.theme());
    (session, reporter)
}

/// The identity a listening agent presents, from `--host-key` or the configuration directory.
///
/// It is generated on first use and kept, because a host whose identity changed on every start
/// would be refused by everyone who pinned it (spec §21.5, ADR-0353).
///
/// Without `--host-key` this is the same identity a direct link presents, resolved through the
/// same ladder: v0.4.1 §8.5 requires `--agent --print-host-key` and `--print-peer-key` to print
/// the same fingerprint when the default path is used, and they do because there is one default
/// path (§8.1, §8.2, ADR-0435).
fn agent_identity(
    options: &ono_cli::invocation::AgentOptions,
) -> Result<ono_remote::PeerIdentity, ono_value::ErrorValue> {
    match &options.host_key {
        Some(path) => ono_remote::PeerIdentity::open_or_create(path),
        None => default_identity(),
    }
}

/// This shell's own peer identity, from the configuration directory of ADR-0010 (v0.4.1 §8.1).
fn default_identity() -> Result<ono_remote::PeerIdentity, ono_value::ErrorValue> {
    let environment: Vec<(String, String)> = std::env::vars().collect();
    let sources = ono_cli::hosts::HostSources::from_environment(
        environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );
    ono_cli::trust::identity(&sources)
}

/// Serves links over Ono's own authenticated transport (spec §21.5): one TLS 1.3 endpoint,
/// one agent per peer.
///
/// The address actually bound and the fingerprint peers must pin are written to stderr before
/// the first peer is accepted, so an operator can read the fingerprint off the host's own
/// console — which is the one channel that makes a first pin worth anything — and so a caller
/// that asked for port 0 learns which port the system chose.
async fn serve_authenticated(
    address: &str,
    identity: &ono_remote::PeerIdentity,
    config: ono_remote::AgentConfig,
) -> ExitCode {
    let listener = match ono_remote::TlsListener::bind(address, identity).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            return ExitCode::from(ExitStatus::FAILURE);
        }
    };
    match listener.local_addr() {
        Ok(bound) => eprintln!("{}: listening on {bound}", ono_core::SHORT_NAME),
        Err(error) => {
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            return ExitCode::from(ExitStatus::FAILURE);
        }
    }
    eprintln!(
        "{}: host key {}",
        ono_core::SHORT_NAME,
        identity.fingerprint()
    );
    loop {
        match listener.accept().await {
            Ok(transport) => {
                let config = config.clone();
                tokio::spawn(async move {
                    let _ = ono_remote::serve_registry(transport, config).await;
                });
            }
            // One peer that could not complete a handshake is not a reason to stop serving the
            // rest: it is reported and the listener stays up (spec §16.5).
            Err(error) => eprintln!("{}: {}", ono_core::SHORT_NAME, error.message()),
        }
    }
}
