//! The Ono-Sendai shell.
//!
//! `ono-cli` is the integration point: it holds the evaluator, name resolution, the builtins and
//! the interactive loop, and wires together the parser, the value model, the process layer, the
//! renderer, the editor and the history (ADR-0005). Everything below it is a library that knows
//! nothing about the shell.

#![forbid(unsafe_code)]

// The build is `full` or `core` and nothing in between (#127, ADR-0910). Each tier has a feature
// of its own so a `cfg` says which tier it belongs to, but the tiers reach into one another — the
// temporal seam reads the spatial session, a change plan reads both, a package contributes places
// — so a partial set would be a program nobody has compiled, let alone tested.
#[cfg(all(
    any(
        feature = "adapter",
        feature = "change",
        feature = "container",
        feature = "graph",
        feature = "kuang",
        feature = "remote",
        feature = "spatial",
        feature = "systemd",
        feature = "temporal",
    ),
    not(all(
        feature = "adapter",
        feature = "change",
        feature = "container",
        feature = "graph",
        feature = "kuang",
        feature = "remote",
        feature = "spatial",
        feature = "systemd",
        feature = "temporal",
    ))
))]
compile_error!(
    "ono-cli builds as `full` (the default) or as `core` (`--no-default-features --features \
     core`); a partial set of tier features is not a supported build (ADR-0910)"
);

pub mod absent;
pub mod builtin;
#[cfg(feature = "change")]
pub mod change;
#[cfg(not(feature = "change"))]
#[path = "absent/change.rs"]
pub mod change;
pub mod complete;
pub mod config;
pub mod context;
pub mod context_jobs;
pub mod eval;
pub mod expand;
pub mod hosts;
pub mod invocation;
#[cfg(feature = "kuang")]
pub mod kuang_acquire;
#[cfg(feature = "kuang")]
mod kuang_catalog;
#[cfg(feature = "kuang")]
pub mod kuang_host;
#[cfg(feature = "kuang")]
pub mod kuang_install;
#[cfg(feature = "kuang")]
mod kuang_keyless;
#[cfg(feature = "kuang")]
pub mod kuang_permissions;
#[cfg(feature = "kuang")]
mod kuang_services;
#[cfg(feature = "kuang")]
pub mod kuang_trust;
#[cfg(feature = "kuang")]
pub mod kuang_views;
pub mod limits;
pub mod live;
pub mod meta;
pub mod piped;
#[cfg(feature = "kuang")]
pub mod plugin_provider;
#[cfg(feature = "kuang")]
pub mod plugin_registry;
#[cfg(not(feature = "kuang"))]
#[path = "absent/plugin_registry.rs"]
pub mod plugin_registry;
#[cfg(feature = "kuang")]
pub mod plugins;
#[cfg(not(feature = "kuang"))]
#[path = "absent/plugins.rs"]
pub mod plugins;
pub mod providers;
#[cfg(feature = "remote")]
pub mod remote;
#[cfg(not(feature = "remote"))]
#[path = "absent/remote.rs"]
pub mod remote;
pub mod repl;
pub mod report;
pub mod resolve;
pub mod session;
pub mod session_provider;
pub mod settings;
pub mod sink;
#[cfg(feature = "spatial")]
pub mod spatial;
#[cfg(not(feature = "spatial"))]
#[path = "absent/spatial.rs"]
pub mod spatial;
#[cfg(feature = "temporal")]
pub mod temporal;
#[cfg(not(feature = "temporal"))]
#[path = "absent/temporal.rs"]
pub mod temporal;
pub mod theme;
#[cfg(feature = "remote")]
pub mod trust;
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
         {peer_key}\
         \x20 --no-config      read no configuration at all\n\
         \x20 -V, --version    print the version and exit\n\
         \x20 -h, --help       print this help and exit\n\
         \n\
         With no script and no -c, {name} reads commands from the terminal.\n\
         \n\
         The command reference lives in docs/reference/, generated from the contracts in\n\
         docs/contracts/. `explain <command>` reports how a name resolves.",
        product = ono_core::PRODUCT_NAME,
        name = ono_core::SHORT_NAME,
        // A flag of the remote tier is offered only by a build that has it (ADR-0911).
        peer_key = if cfg!(feature = "remote") {
            "  --print-peer-key print this shell's own link fingerprint and exit\n"
        } else {
            ""
        },
    )
}
