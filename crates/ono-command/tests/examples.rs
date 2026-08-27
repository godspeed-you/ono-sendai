//! Spec §50: documented examples are executable, not decorative.
//!
//! The parser check in `cargo xtask spec-check` already refuses an example that does not parse.
//! This suite covers the next failure the parser cannot see: an example whose arguments no
//! longer *bind* — a renamed selector, a dropped option, a spelling the grammar allows but the
//! contract refuses. `reduce @acc + @` parsed for a whole phase while being impossible to run;
//! this is the test that would have caught it on the day it was written.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod fixture;

use ono_command::CommandRegistry;
use ono_parser::{StageHead, Statement};

#[test]
fn should_bind_every_documented_example_against_its_contract() {
    let registry = CommandRegistry::embedded().expect("the embedded contracts load");
    let mut complaints = Vec::new();

    for contract in registry.commands() {
        for example in contract.examples() {
            let parsed = ono_parser::parse(example);
            for statement in &parsed.program().statements {
                let Some(pipeline) = Statement::as_pipeline(statement) else {
                    continue;
                };
                let lists = std::iter::once(&pipeline.head)
                    .chain(pipeline.tail.iter().map(|chained| &chained.list));
                for list in lists {
                    for stage in &list.stages {
                        let StageHead::Command(name) = &stage.head else {
                            continue;
                        };
                        // `enter`/`leave`, builtins and external programs are not the
                        // registry's to bind; a head it does not resolve is one of those.
                        let Ok(resolved) = registry.resolve(&name.name, &stage.arguments) else {
                            continue;
                        };
                        // `explain`, `help` and `type` take the rest of the line as their
                        // subject — the evaluator hands the source text over whole, pipes,
                        // options and all — so their arguments are not theirs to bind.
                        if matches!(
                            resolved.contract.id(),
                            "ono.meta.explain" | "ono.meta.help" | "ono.meta.type"
                        ) {
                            continue;
                        }
                        if let Err(error) = resolved.contract.bind(resolved.arguments) {
                            complaints.push(format!(
                                "`{}` documents `{example}`, whose `{}` stage no longer binds: {}",
                                contract.id(),
                                name.name,
                                error.message(),
                            ));
                        }
                    }
                }
            }
        }
    }

    assert!(
        complaints.is_empty(),
        "spec §50: every documented example must still bind against its contract —\n{}",
        complaints.join("\n"),
    );
}
