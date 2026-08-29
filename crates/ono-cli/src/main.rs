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
        Invocation::Agent(_) => {
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
