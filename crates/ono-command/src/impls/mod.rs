//! The native implementations behind the registry's ids, and the one call that assembles them.
//!
//! Spec §27.2 has native code register an implementation against a stable command id. This module
//! is the register, and [`builtin_commands`] is the whole of it: a host calls it once and has the
//! command set this build delivers.
//!
//! # What is registered, and what deliberately is not
//!
//! Only commands whose spec §37 phase this build has reached are registered. That is not a
//! euphemism for "missing": a command with no implementation is reported by
//! [`unbound_stable_commands`](crate::unbound_stable_commands) and answered with
//! `resolve.command_not_found` naming it, which is a better answer than a stub that fails halfway
//! through doing something. `help` still describes it, because the contract is the product surface
//! whether or not a phase has delivered it yet.
//!
//! Three groups are deliberately left out even inside a delivered phase, because their state does
//! not live here:
//!
//! - `get config` / `set config` — configuration is the session's, and ADR-0010 puts its layers
//!   and provenance in the evaluator;
//! - `enter` / `leave` — the context stack of spec §14.1 is the session's;
//! - `resolve command` — ADR-0011's resolution order runs through functions, aliases and `PATH`,
//!   none of which the registry can see. Answering from the registry alone would report a
//!   resolution the shell would not actually perform.

pub(crate) mod convert;
pub(crate) mod meta;
pub(crate) mod mutate;
pub(crate) mod producer;
mod trace;
pub(crate) mod transform;
mod watch;

use std::sync::Arc;

use crate::contract::{CommandContract, Phase};
use crate::invoke::CommandTable;
use crate::registry::CommandRegistry;

/// The spec §37 phases this build delivers.
///
/// A command scheduled for a later phase is not registered, so the honesty of
/// [`unbound_stable_commands`](crate::unbound_stable_commands) survives contact with a half-built
/// shell.
const DELIVERED: &[char] = &['A', 'B', 'C', 'D', 'E', 'F', 'G'];

/// The command table this build has: every implementation, registered against its contract id.
///
/// This is the one call a host makes.
///
/// ```
/// use ono_command::{CommandRegistry, builtin_commands};
///
/// let registry = CommandRegistry::embedded()?;
/// let table = builtin_commands(registry);
///
/// assert!(table.contains("ono.data.where"), "`where` is delivered by phase B");
/// assert!(table.contains("ono.process.get"), "`get process` is delivered by phase C");
/// assert!(
///     !table.contains("ono.plugin.install"),
///     "KUANG/11 arrives in phase I, and a stub would be worse than an honest absence"
/// );
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[must_use]
pub fn builtin_commands(registry: &'static CommandRegistry) -> CommandTable {
    let mut table = CommandTable::new();
    for contract in registry.commands() {
        if !delivered(contract) {
            continue;
        }
        if let Some(implementation) = implementation_of(contract, registry) {
            table.register(implementation);
        }
    }
    table
}

fn delivered(contract: &CommandContract) -> bool {
    matches!(contract.phase(), Phase::Delivered(letter) if DELIVERED.contains(&letter))
}

fn implementation_of(
    contract: &CommandContract,
    registry: &'static CommandRegistry,
) -> Option<Arc<dyn crate::invoke::CommandImpl>> {
    use convert::{ConversionCommand, Direction};
    use meta::MetaCommand;
    use mutate::ProviderMutation;
    use producer::ProviderProducer;
    use transform::{Kind, TransformCommand};

    let id = contract.id();
    let native: Arc<dyn crate::invoke::CommandImpl> = match id {
        // --- the transforms of spec §53 -------------------------------------------------------
        "ono.data.where" => Arc::new(TransformCommand::new(id, Kind::Where)),
        "ono.data.select" => Arc::new(TransformCommand::new(id, Kind::Select)),
        "ono.data.sort" => Arc::new(TransformCommand::new(id, Kind::Sort)),
        "ono.data.group" => Arc::new(TransformCommand::new(id, Kind::Group)),
        "ono.data.take" => Arc::new(TransformCommand::new(id, Kind::Take)),
        "ono.data.skip" => Arc::new(TransformCommand::new(id, Kind::Skip)),
        "ono.data.each" => Arc::new(TransformCommand::new(id, Kind::Each)),
        "ono.data.reduce" => Arc::new(TransformCommand::new(id, Kind::Reduce)),
        "ono.data.count" => Arc::new(TransformCommand::new(id, Kind::Count)),
        "ono.data.measure" => Arc::new(TransformCommand::new(id, Kind::Measure)),

        // --- the boundary of spec §12.3 -------------------------------------------------------
        "ono.data.to" => Arc::new(ConversionCommand::new(id, Direction::Serialize)),
        "ono.data.from" => Arc::new(ConversionCommand::new(id, Direction::Deserialize)),
        "ono.data.format" => Arc::new(ConversionCommand::new(id, Direction::Render)),

        // --- the commands that describe the shell ----------------------------------------------
        "ono.process.watch" => Arc::new(watch::WatchCommand::new(id)),
        "ono.process.trace" | "ono.service.trace" | "ono.socket.trace" | "ono.connection.trace" => {
            Arc::new(trace::TraceCommand::new(id))
        }
        "ono.context.get" => Arc::new(MetaCommand::new(id, meta::Kind::GetContext, registry)),
        "ono.meta.help" => Arc::new(MetaCommand::new(id, meta::Kind::Help, registry)),
        "ono.meta.explain" => Arc::new(MetaCommand::new(id, meta::Kind::Explain, registry)),
        "ono.meta.type" => Arc::new(MetaCommand::new(id, meta::Kind::Type, registry)),
        "ono.meta.inspect" => Arc::new(MetaCommand::new(id, meta::Kind::Inspect, registry)),
        "ono.command.get" => Arc::new(MetaCommand::new(id, meta::Kind::GetCommand, registry)),
        "ono.command.find" => Arc::new(MetaCommand::new(id, meta::Kind::FindCommand, registry)),

        // --- everything a provider answers ------------------------------------------------------
        _ => {
            let target = contract.target()?;
            contract.provider_capability()?;
            // `command` and `config` are answered above and by the evaluator; a generic producer
            // over them would ask a provider that does not and should not exist.
            if matches!(target, "command" | "config" | "context") {
                return None;
            }
            match contract.verb() {
                "get" | "find" => Arc::new(ProviderProducer::new(id)),
                // The verbs every provider's `act` already speaks. A mutating verb no provider
                // implements is left unregistered rather than registered to fail (spec §50).
                "start" | "stop" | "restart" | "kill" => Arc::new(ProviderMutation::new(id)),
                _ => return None,
            }
        }
    };
    Some(native)
}
