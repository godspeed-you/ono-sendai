//! Blocks, and the one-item scope a block stage runs in.
//!
//! `run_each_item` is the evaluator half of ADR-0480's block bridge: the driver in `native` asks
//! for one item, this answers with what that one invocation produced, and §25.4 bounds the
//! capture to exactly that.

use ono_core::ExitStatus;
use ono_parser::{Argument, Block, Expr, StageHead, StageList};
use ono_value::Value;

use crate::session::Session;

use super::statement::run_statement;
use super::{Eval, Flow};

/// The index of the `each` stage whose body is a block, if `list` has one.
pub(super) fn each_block_stage(list: &StageList) -> Option<usize> {
    list.stages.iter().position(|stage| {
        let StageHead::Command(name) = &stage.head else {
            return false;
        };
        matches!(name.namespace.as_deref(), None | Some("ono"))
            && name.name == "each"
            && matches!(
                stage.arguments.as_slice(),
                [Argument::Value(Expr::Block(_))]
            )
    })
}

/// Runs one `each { … }` block over one item, as a stage of the pipeline it stands in.
///
/// v0.4.1 §25.1 requires the block to run for a value the source has already produced, before the
/// source has completed. The streaming stage in [`super::native`] calls this once per item while
/// the rest of the pipeline is still running, and it is the only place a block is ever run
/// (ADR-0480). What comes back is what the stage forwards, and whether the stage should keep
/// reading its input.
///
/// `capture` is ADR-0070 point 3: with stages after it the block's values stream into them, and
/// with nothing after it its statements show their results where they stand. The capture is one
/// item's result — §25.4's per-invocation scope — never a collection over all of them.
///
/// # Errors
///
/// The block's own `Flow` when it failed, returned or exited: those unwind the pipeline rather
/// than being answered.
pub(crate) fn run_each_item(
    session: &mut Session,
    block: &Block,
    source: &str,
    item: Value,
    capture: bool,
) -> Eval<(Vec<Value>, bool)> {
    session.push_scope();
    session.bind("@", item);
    if capture {
        session.begin_capture();
    }
    let outcome = run_block(session, block, source);
    let produced = if capture {
        session.end_capture()
    } else {
        Vec::new()
    };
    session.pop_scope();
    match outcome {
        // §25.5: `continue` skips the remainder of the current item and the next one is read;
        // `break` stops consuming upstream, which the stage does by dropping its input.
        Ok(_) | Err(Flow::Continue) => Ok((produced, true)),
        Err(Flow::Break) => Ok((produced, false)),
        Err(other) => Err(other),
    }
}

pub(super) fn run_block(session: &mut Session, block: &Block, source: &str) -> Eval<ExitStatus> {
    session.push_scope();
    let mut status = ExitStatus::SUCCESS;
    let mut outcome = Ok(());
    for statement in &block.statements {
        match run_statement(session, statement, source) {
            Ok(reached) => status = reached,
            Err(flow) => {
                outcome = Err(flow);
                break;
            }
        }
    }
    session.pop_scope();
    outcome.map(|()| status)
}
