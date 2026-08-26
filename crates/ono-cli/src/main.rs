//! The `ono` binary.
//!
//! Scaffolding only. It answers `--version` and `--help` so that the workspace, the quality
//! gate and the containerised acceptance harness have something real to verify before phase A
//! of the specification begins. The interpreter itself is built test-first from there.

use std::process::ExitCode;

use ono_core::{PRODUCT_NAME, SHORT_NAME, VERSION};

/// Exit code used when the command line could not be understood (spec section 16).
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().map(String::as_str).collect();

    match flags.as_slice() {
        ["--version" | "-V"] => {
            println!("{SHORT_NAME} {VERSION}");
            ExitCode::SUCCESS
        }
        ["--help" | "-h"] => {
            print_help();
            ExitCode::SUCCESS
        }
        [] => {
            eprintln!(
                "{SHORT_NAME}: the interactive shell is not implemented yet; see docs/STATE.md"
            );
            ExitCode::from(EXIT_USAGE)
        }
        unknown => {
            eprintln!(
                "{SHORT_NAME}: unrecognised arguments: {}",
                unknown.join(" ")
            );
            eprintln!("try `{SHORT_NAME} --help`");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn print_help() {
    println!("{PRODUCT_NAME} - a typed, structured Unix shell");
    println!();
    println!("usage: {SHORT_NAME} [options]");
    println!();
    println!("options:");
    println!("  -V, --version    print the version and exit");
    println!("  -h, --help       print this help and exit");
}
