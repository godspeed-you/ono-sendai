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
                // v0.4.1 §9.5, §56.4: provider capabilities stay the canonical authorization
                // unit, so the agent learns which capability an action needs from the same
                // `provider_capability` field `docs/spec/commands/` already declares. A peer
                // never says which capability it needs; this side resolves it (§65.2).
                let config = with_action_capabilities(config);
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

/// The capability every action a command contract declares needs, as the agent resolves it.
///
/// `(target, verb) -> provider_capability`, read from the command registry: `stop process` needs
/// `process.signal`, `restart service` needs `service.manage`. An action the registry does not
/// map is denied under a policy, because Appendix C denies an unknown capability id always.
fn with_action_capabilities(config: ono_remote::AgentConfig) -> ono_remote::AgentConfig {
    let Ok(registry) = ono_cli::native::registry() else {
        // Without the registry no action can be named, so every action is denied. That is the
        // fail-closed direction (v0.4.1 §2.3), and the agent still serves reads.
        return config;
    };
    let mut config = config;
    for command in registry.commands() {
        if let (Some(target), Some(capability)) = (command.target(), command.provider_capability())
        {
            config = config.with_action_capability(target, command.verb(), capability);
        }
    }
    config
}

/// The authorization store a listening agent decides by (v0.4.1 §9.2).
///
/// §2.3 and §59.5 between them settle what happens when it cannot be read: "if Ono claims that a
/// safety control is applied before an operation, failure to apply that control MUST prevent the
/// operation from starting", and "a malformed line in `authorized_clients` MUST cause
/// deterministic startup/configuration failure". So a corrupt store stops the agent before it
/// binds, rather than being read as an empty one — which would authorize nobody today and be one
/// edit away from authorizing everybody.
fn authorization_store() -> Result<ono_protocol::AuthorizedClients, ono_value::ErrorValue> {
    let environment: Vec<(String, String)> = std::env::vars().collect();
    let sources = ono_cli::hosts::HostSources::from_environment(
        environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );
    ono_cli::trust::open_authorized_clients(&sources)
}

/// Where a listening agent reads the clients it may serve (v0.4.1 §9.2).
fn authorization_store_path() -> Option<std::path::PathBuf> {
    let environment: Vec<(String, String)> = std::env::vars().collect();
    let sources = ono_cli::hosts::HostSources::from_environment(
        environment
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str())),
    );
    ono_cli::trust::authorized_clients_path(&sources)
}

/// Serves links over Ono's own authenticated transport (spec §21.5): one TLS 1.3 endpoint,
/// one agent per peer, under the ceilings of v0.4.1 §12.
///
/// Before the first peer is accepted the agent prints the startup summary §11.2 requires — the
/// bound address, the fingerprint peers must pin, the store that decides who is served, how many
/// clients that store holds and the ceilings the agent will enforce. An operator reads that block
/// to know what they have just put on a network, and the host's own console is the one channel
/// that makes a first pin worth anything.
async fn serve_authenticated(
    address: &str,
    identity: &ono_remote::PeerIdentity,
    config: ono_remote::AgentConfig,
) -> ExitCode {
    // Before the socket, not after: an agent that bound first and then discovered it had no
    // usable policy would have been reachable while it decided (§2.3, §59.5).
    let store = match authorization_store() {
        Ok(store) => store,
        Err(error) => {
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            if let Some(help) = error.help() {
                eprintln!("{}: {help}", ono_core::SHORT_NAME);
            }
            return ExitCode::from(ExitStatus::FAILURE);
        }
    };

    let listener = match ono_remote::TlsListener::bind(address, identity).await {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            return ExitCode::from(ExitStatus::FAILURE);
        }
    };
    let bound = match listener.local_addr() {
        Ok(bound) => bound,
        Err(error) => {
            eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
            return ExitCode::from(ExitStatus::FAILURE);
        }
    };

    // The agent enforces the ceilings the configuration layer declares, so a figure an operator
    // set is the figure the summary prints and the listener applies (§12.4, §52.2, ADR-0501).
    let limits = configured_limits();
    print_startup_summary(bound, identity, store.entries().len(), &limits);

    let agent = ono_remote::ListeningAgent::new(listener, config)
        .with_limits(limits)
        .with_audit(std::sync::Arc::new(ono_remote::StderrAudit))
        // Read once per accepted connection, so `add client-key` on this host reaches the next
        // connection without a restart, and read again by the revocation sweep of §12.5.
        .with_authorization_source(authorization_store);
    let error = agent.run().await;
    eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
    ExitCode::from(ExitStatus::FAILURE)
}

/// The block v0.4.1 §11.2 requires a listening agent to print before it accepts anybody.
///
/// Five fields, in §11.2's order, on stderr: an agent carried over stdio owns stdout for the wire
/// and never writes diagnostics there, and a listening one keeps the same discipline so the two
/// modes are not two contracts (§14.1).
fn print_startup_summary(
    bound: std::net::SocketAddr,
    identity: &ono_remote::PeerIdentity,
    authorized: usize,
    limits: &ono_protocol::Limits,
) {
    let name = ono_core::SHORT_NAME;
    eprintln!("{name}: listening on {bound}");
    eprintln!("{name}: host key {}", identity.fingerprint());
    eprintln!(
        "{name}: authorization store {}",
        authorization_store_path().map_or_else(
            || "none — this shell keeps no configuration directory".to_owned(),
            |path| path.display().to_string()
        )
    );
    eprintln!("{name}: authorized clients {authorized}");
    if authorized == 0 {
        // §11.2 lets an agent listen for nobody and requires it to refuse everybody; §54.1 and
        // §2.3 say it must be legible rather than discovered from a refused client (ADR-0504).
        eprintln!(
            "{name}: no client is authorized yet, so every connection will be refused after the \
             cryptographic handshake (v0.4.1 section 11.2). `add client-key <fingerprint>` \
             authorizes one to observe (section 9.4)"
        );
    }
    eprintln!("{name}: maximum connections {}", limits.max_connections());
    eprintln!(
        "{name}: maximum connections per client {}",
        limits.max_connections_per_client()
    );
    eprintln!(
        "{name}: maximum pending handshakes {}",
        limits.max_pending_handshakes()
    );
    eprintln!("{name}: handshake timeout {:?}", limits.handshake_timeout());
}

/// The ceilings this agent will enforce, from the one place they are declared (§12.4, §55.1).
///
/// `ono_protocol::Limits::default()` is Appendix A, and every `limits.remote_*` key an operator
/// set moves it — through the same catalogue `Settings::assign` range-checks and `inspect limits`
/// reports, so the figure a user reads is the figure the listener applies (ADR-0456, ADR-0461).
///
/// Agent mode reads the environment layer and no file, which is what agent mode does for every
/// other setting: `ono --agent` is a protocol endpoint and has never had a configuration file
/// execution surface. Honouring `config.ono` here as well is recorded for the board.
fn configured_limits() -> ono_protocol::Limits {
    let mut settings = ono_cli::settings::Settings::new();
    let variables: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> =
        std::env::vars_os().collect();
    settings.apply_environment(&variables, &mut |error| {
        // §55.2: a security-sensitive agent limit never silently becomes unlimited because a
        // value failed to parse. Nothing was stored, so the declared default stays in force, and
        // the operator is told which variable they have to fix.
        eprintln!("{}: {}", ono_core::SHORT_NAME, error.message());
    });
    let read =
        |key: &str| u32::try_from(ono_cli::limits::magnitude(&settings, key)).unwrap_or(u32::MAX);
    ono_protocol::Limits::default()
        .with_max_connections(read("limits.remote_connections"))
        .with_max_pending_handshakes(read("limits.remote_pending_handshakes"))
        .with_max_connections_per_client(read("limits.remote_connections_per_client"))
        .with_handshake_timeout(std::time::Duration::from_millis(
            ono_cli::limits::magnitude(&settings, "limits.remote_handshake_timeout_ms"),
        ))
}
