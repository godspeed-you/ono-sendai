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
            let (mut session, reporter) = start(true, &options);
            repl::run(&mut session, &options, &reporter)
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
    (session, reporter)
}
