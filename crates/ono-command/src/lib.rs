//! The Ono-Sendai command registry: public command contracts as data (spec §27).
//!
//! Spec §27's design goal is that "public command contracts are data". This crate is what makes
//! that true at runtime. `docs/contracts/commands/` declares every command — its verb, its target, its
//! selectors and options with their types, its input and output schemas, the provider capability
//! it needs, its privilege, its stability and its examples — and this crate turns those files
//! into the registry the shell consults for four things spec §15 and §42 require:
//!
//! - **binding** a parsed stage's arguments to the declared types ([`CommandContract::bind`]),
//!   which is where a word becomes an `int`, a `duration` or a `bytesize` and where every way
//!   that can fail becomes a structured error;
//! - **help** generated from metadata rather than written by hand ([`help`], spec §15.2);
//! - **completion** candidates that are lookups rather than searches ([`complete`], spec §15.1);
//! - **`explain`**, the plan of spec §42, produced without executing anything ([`plan`]).
//!
//! It also carries the binding between an id and native code ([`CommandImpl`], [`CommandTable`])
//! and the check spec §27.2 asks CI to run ([`unbound_stable_commands`]).
//!
//! # The contracts are in the binary
//!
//! [`CommandRegistry::embedded`] parses contract files that were embedded at compile time. Spec
//! §34 budgets a cold start under 100 ms with a target of 50 ms, which a dozen YAML reads would
//! spend before the prompt appears; and a shell whose command set depends on files being
//! installed correctly is a shell that breaks when they are not.
//!
//! ```
//! use ono_command::{CommandRegistry, StageContext};
//! use ono_value::Value;
//!
//! let registry = CommandRegistry::embedded()?;
//!
//! // The contract, straight out of `docs/contracts/commands/process.yaml`.
//! let command = registry.find("get", Some("process")).expect("`get process` is declared");
//! assert_eq!(command.output().text(), "stream<ono.process/1>");
//!
//! // Arguments bound against the declared types.
//! let parsed = ono_parser::parse("get process 4419");
//! let stage = &parsed.program().statements[0].as_pipeline().expect("a pipeline").head.stages[0];
//! let resolved = registry.resolve("get", &stage.arguments)?;
//! let bound = resolved.contract.bind(resolved.arguments)?;
//! assert_eq!(bound.selector("pid"), Some(&Value::Int(4419)));
//!
//! // Help and completion, from the same metadata.
//! assert!(ono_command::help(registry, None, "get process")?.render().contains("SYNOPSIS"));
//! let context = StageContext::from_line("get pro", 7);
//! assert_eq!(ono_command::complete(registry, &context, None)[0].text(), "process");
//! # Ok::<(), ono_value::ErrorValue>(())
//! ```

#![forbid(unsafe_code)]

mod bind;
mod check;
mod complete;
mod contract;
mod explain;
mod expr;
mod help;
mod impls;
mod invoke;
mod narrow;
mod registry;
mod suggest;

pub use bind::{Binding, BoundArguments};
pub use check::{check_pipeline, check_pipeline_with};
pub use complete::{Candidate, CandidateKind, StageContext, ValueCompleter, complete};
pub use contract::{
    ArgumentMode, CapabilitySpec, CommandContract, Confirmation, ContributedCommand, DeclaredType,
    Elevation, ExecutionClass, IoType, Origin, ParameterSpec, Phase, Privilege, Stability,
    TargetSpec, VerbSpec,
};
pub use explain::{
    Adaptation, ExecutionPlan, PlanContext, Resolution, StagePlan, adapt_program, is_adapt, is_raw,
    literal_arguments, plan, plan_for, plan_with, raw_program,
};
pub use expr::{
    Scope, check_fields, evaluate, evaluate_to_value, is_now_call, is_true, nested_pipelines,
};
pub use help::{
    CommandHelp, HelpPage, ParameterHelp, TargetHelp, TopicHelp, VerbHelp, help, topics,
};
pub use impls::meta::provenance_value;
pub use impls::watch::{is_watchable, watch_events};
pub use impls::{builtin_commands, builtin_commands_for};
pub use invoke::{
    CommandImpl, CommandTable, ContextFrame, FrameKind, Invocation, Outcome, OutcomeFuture,
    Resolver, must_be_awaited, unbound_stable_commands,
};
pub use registry::{CommandRegistry, Resolved};
pub use suggest::closest;
