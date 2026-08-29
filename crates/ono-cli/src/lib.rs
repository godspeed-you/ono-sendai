//! The Ono-Sendai shell.
//!
//! `ono-cli` is the integration point: it holds the evaluator, name resolution, the builtins and
//! the interactive loop, and wires together the parser, the value model, the process layer, the
//! renderer, the editor and the history (ADR-0005). Everything below it is a library that knows
//! nothing about the shell.

#![forbid(unsafe_code)]

pub mod builtin;
pub mod complete;
pub mod config;
pub mod context;
pub mod context_jobs;
pub mod eval;
pub mod expand;
pub mod hosts;
pub mod invocation;
pub mod kuang_host;
pub mod kuang_trust;
pub mod live;
pub mod meta;
pub mod native;
pub mod piped;
pub mod plugins;
pub mod providers;
pub mod remote;
pub mod repl;
pub mod report;
pub mod resolve;
pub mod session;
pub mod session_provider;
pub mod settings;
pub mod sink;
pub mod spatial;
pub mod view;

/// The usage text, shown by `--help` and by the `help` builtin.
#[must_use]
pub fn usage_text() -> String {
    format!(
        "{product} - a typed, structured Unix shell\n\
         \n\
         usage: {name} [options] [script [arguments...]]\n\
         \n\
         options:\n\
         \x20 -c <source>      run <source>, then exit with its status\n\
         \x20 -                read a script from standard input\n\
         \x20 --config <path>  read this configuration file instead of the usual layers\n\
         \x20 --no-config      read no configuration at all\n\
         \x20 -V, --version    print the version and exit\n\
         \x20 -h, --help       print this help and exit\n\
         \n\
         With no script and no -c, {name} reads commands from the terminal.\n\
         \n\
         The command reference lives in docs/reference/, generated from the contracts in\n\
         docs/spec/. `explain <command>` reports how a name resolves.",
        product = ono_core::PRODUCT_NAME,
        name = ono_core::SHORT_NAME,
    )
}
