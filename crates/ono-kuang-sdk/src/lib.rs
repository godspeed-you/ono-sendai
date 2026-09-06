//! The KUANG/11 plugin SDK: the plugin side of the boundary (spec §31.59, §31.60).
//!
//! A plugin author declares who the package is, registers handlers for the commands and targets
//! it contributes, and calls [`Plugin::run`]. The SDK then speaks the protocol of
//! `docs/contracts/kuang/protocol.v1.yaml` over stdin/stdout: it sends the hello, answers
//! `lifecycle.init` with the features the negotiated contract forces off, dispatches
//! invocations, and enforces the plugin's side of the pull-based flow control — [`Ctx::emit`]
//! waits for credit rather than outrunning the host, because emitting beyond credit is a
//! protocol violation the supervisor quarantines (spec §31.15, ADR-0022 §8).
//!
//! ```no_run
//! use ono_kuang_sdk::{Outcome, Plugin};
//! use ono_value::Value;
//!
//! Plugin::new("dev.example.echo", "0.1.0")
//!     .command("dev.example.echo.command.emit", |ctx| {
//!         for n in 1..=3 {
//!             if ctx.emit(&Value::Int(n)).is_err() {
//!                 return Outcome::Cancelled;
//!             }
//!         }
//!         Outcome::Completed
//!     })
//!     .run();
//! ```
//!
//! A package answers **one invocation at a time** unless it says otherwise, and a second one
//! arriving while the first is open is refused with `runtime.concurrency_limit` — a visible
//! refusal rather than a wedged instance. [`Plugin::concurrent_invocations`] raises that: it is
//! a statement about the code, because each invocation then runs on a worker of its own and a
//! handler must be safe to run beside its siblings. The effective ceiling is the smaller of that
//! number and `max_concurrent_invocations` in the negotiated contract, which is the operator's
//! (spec §31.15, ADR-0586).

mod plugin;

pub use ono_kuang_protocol as protocol;
pub use plugin::{Ctx, EmitError, OpenedView, Outcome, Plugin, ViewNotice};
