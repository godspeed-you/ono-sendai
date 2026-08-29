//! `ono-fuzz`: runs the targets of spec §35.6, or reproduces one finding.
//!
//! ```text
//! ono-fuzz list                                  every target and its corpus size
//! ono-fuzz run [--target T] [--iterations N] [--seed S] [--journal <file>]
//! ono-fuzz repro <target> <file>                 execute one input, once
//! ```

use std::process::ExitCode;
use std::time::Duration;

use ono_fuzz::{Budget, TARGETS, target};

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.split_first() {
        None | Some((&"run", _)) => run(words.get(1..).unwrap_or_default()),
        Some((&"list", _)) => list(),
        Some((&"repro", rest)) => repro(rest),
        Some((other, _)) => {
            eprintln!("ono-fuzz: `{other}` is not a command; try `list`, `run` or `repro`");
            ExitCode::from(2)
        }
    }
}

fn list() -> ExitCode {
    for entry in TARGETS {
        println!(
            "{:<18} {:<28} {} seeds",
            entry.name,
            entry.area,
            ono_fuzz::load_for(entry.name).len()
        );
    }
    ExitCode::SUCCESS
}

fn run(words: &[&str]) -> ExitCode {
    let mut budget = Budget::default();
    let mut only = None;
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        match *word {
            "--target" => only = iter.next().map(|text| (*text).to_owned()),
            "--iterations" => {
                budget.iterations = iter
                    .next()
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(budget.iterations);
            }
            "--seed" => {
                budget.seed = iter
                    .next()
                    .and_then(|text| text.parse().ok())
                    .unwrap_or(budget.seed);
            }
            "--journal" => budget.journal = iter.next().map(std::path::PathBuf::from),
            "--per-input-ms" => {
                budget.per_input = iter
                    .next()
                    .and_then(|text| text.parse().ok())
                    .map_or(budget.per_input, Duration::from_millis);
            }
            other => {
                eprintln!("ono-fuzz: `{other}` is not an option of `run`");
                return ExitCode::from(2);
            }
        }
    }
    let selected: Vec<&'static ono_fuzz::Target> = match &only {
        None => TARGETS.iter().collect(),
        Some(name) => match target(name) {
            Some(one) => vec![one],
            None => {
                eprintln!("ono-fuzz: no target is called `{name}`; `ono-fuzz list` names them");
                return ExitCode::from(2);
            }
        },
    };
    let mut failed = false;
    for entry in selected {
        let corpus = ono_fuzz::load_for(entry.name);
        let report = ono_fuzz::run(entry, &corpus, &budget);
        println!(
            "{:<18} {} executions from {} seeds, slowest {:?}",
            report.target, report.executions, report.seeds, report.slowest
        );
        for finding in &report.findings {
            failed = true;
            let written = ono_fuzz::record(entry.name, &finding.input);
            eprintln!(
                "\nono-fuzz: {} {:?}\n  {}",
                entry.name, finding.fault, finding.detail
            );
            match written {
                Ok(path) => eprintln!(
                    "  reproduce with:\n    cargo run -p ono-fuzz -- repro {} {}",
                    entry.name,
                    path.display()
                ),
                Err(error) => eprintln!("  the input could not be written: {error}"),
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn repro(words: &[&str]) -> ExitCode {
    let (Some(name), Some(path)) = (words.first(), words.get(1)) else {
        eprintln!("ono-fuzz: `repro <target> <file>`");
        return ExitCode::from(2);
    };
    let Some(entry) = target(name) else {
        eprintln!("ono-fuzz: no target is called `{name}`; `ono-fuzz list` names them");
        return ExitCode::from(2);
    };
    let input = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("ono-fuzz: cannot read {path}: {error}");
            return ExitCode::from(2);
        }
    };
    // No panic hook and no catch: the point of `repro` is to get the backtrace.
    (entry.run)(&input);
    println!("{}: {} bytes, no fault", entry.name, input.len());
    ExitCode::SUCCESS
}
