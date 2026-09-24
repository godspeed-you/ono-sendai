//! `kuang-compile`: compiles a KUANG/11 component once, for the shell that will load it
//! (spec §31.10, §31.36; ADR-0870).
//!
//! ```text
//! kuang-compile <component.wasm>...                 into the operator's store
//! kuang-compile --store <directory> <component.wasm>...
//! ```
//!
//! The shell carries the WebAssembly runtime and no compiler. A `wasm-component` package ships
//! `runtime/component.wasm`, and this tool writes the engine's own compiled form of it into an
//! artifact store, named by the component's SHA-256: `$XDG_CACHE_HOME/ono/kuang/compiled/` (or
//! `~/.cache/ono/kuang/compiled/`) by default, and `/usr/lib/ono-sendai/kuang-compiled/` when an
//! administrator or an image build passes it as `--store`. `install plugin` runs it; a package
//! placed by hand, and every package after a shell upgrade that changes the engine, needs it run
//! once. Running it again is harmless: the artifact is replaced.
//!
//! It prints the path of each artifact it wrote.

#![allow(
    clippy::print_stderr,
    clippy::print_stdout,
    reason = "a command-line tool speaks on stdout and stderr"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use ono_kuang_supervisor::compiled;

fn main() -> ExitCode {
    let mut store = None;
    let mut components = Vec::new();
    let mut arguments = std::env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--store" {
            match arguments.next() {
                Some(directory) => store = Some(PathBuf::from(directory)),
                None => return usage(),
            }
        } else if argument == "--help" || argument == "-h" {
            let _ = usage();
            return ExitCode::SUCCESS;
        } else {
            components.push(PathBuf::from(argument));
        }
    }
    if components.is_empty() {
        return usage();
    }
    let Some(store) = store.or_else(compiled::user_store) else {
        eprintln!(
            "kuang-compile: neither XDG_CACHE_HOME nor HOME names a directory to keep the \
             artifact in; pass --store <directory>"
        );
        return ExitCode::FAILURE;
    };
    let mut failed = false;
    for component in &components {
        match compiled::compile(component, &store) {
            Ok(artifact) => println!("{}", artifact.display()),
            Err(why) => {
                eprintln!("kuang-compile: {why}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "kuang-compile: compile a KUANG/11 component for the shell that loads it ({})\n  \
         kuang-compile <component.wasm>...\n  \
         kuang-compile --store <directory> <component.wasm>...\n\
         Without --store, the artifact goes to the operator's store ($XDG_CACHE_HOME/ono/kuang/compiled).",
        compiled::engine_identity()
    );
    ExitCode::from(2)
}
