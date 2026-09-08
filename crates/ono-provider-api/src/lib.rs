//! The contract every Ono provider implements.
//!
//! Spec §31's preamble sets the requirement that shapes this crate: the extension runtime "MAY be
//! implemented after the shell foundation, but command metadata, value schemas, object identity,
//! provider capabilities, rendering and execution plans SHOULD already be shaped so that KUANG/11
//! can consume them without special cases."
//!
//! So everything a plugin will be handed later is defined here and used by the core providers
//! now: [`Query`] and [`Selector`] rather than an AST, [`ObjectId`] rather than a row number,
//! [`ObjectEvent`] in the envelope of spec §31.14, [`Capability`] in the vocabulary of
//! `docs/contracts/capabilities.yaml`, and [`ActionOutcome`] rather than an exit code. A KUANG/11
//! provider is a [`Provider`], not a special case of one.
//!
//! The three primitives of spec §31.14 are [`Provider::snapshot`] (state now),
//! [`Provider::subscribe`] (changes over time) and, built on them, the runtime-managed `watch`.
//! Spec v0.5 §21 adds a fourth: [`Provider::history`] (state and events *before* now), together
//! with [`Provider::temporal`], which states what the provider can honestly answer about the past
//! before anybody asks it. Both default to the truthful answer for a provider that has never
//! thought about time — it claims nothing, and refuses.

#![forbid(unsafe_code)]

mod action;
mod capability;
mod events;
mod label;
mod object;
mod query;
mod registry;
mod temporal;

pub use action::{Action, ActionOutcome};
pub use capability::{Availability, Capability, Risk};
pub use events::{EventSink, EventStream};
pub use label::{declared_label, endpoint_label, endpoint_text, label_of};
pub use object::{EventKind, ObjectEvent, ObjectId, ObjectRef};
pub use query::{Query, Selector};
pub use registry::ProviderRegistry;
pub use temporal::{TemporalCapabilities, TimeWindow, unsupported_history};

use std::sync::Arc;

use ono_pipeline::ValueStream;
use ono_value::{ErrorValue, Schema};

/// A source of objects, and the operations that change them.
///
/// A provider answers about one or more targets. It never formats anything (spec §5), never
/// parses unstable human-readable text (spec §50), and reports what it does not know as `null`
/// rather than as zero (spec §35.3).
#[async_trait::async_trait]
pub trait Provider: Send + Sync + std::fmt::Debug {
    /// The provider's stable id, such as `linux.procfs`. It appears in every record's provenance,
    /// so a user can always ask where a value came from.
    fn id(&self) -> &str;

    /// The targets this provider answers about.
    fn targets(&self) -> &[&str];

    /// The token this provider writes into the `provider` field of the records it makes.
    ///
    /// A schema whose identity begins with `provider` — `ono.package/1`, `ono.service/1` — says
    /// which of several answering systems an object belongs to, and the value it says is not the
    /// provider's id: both Red Hat and SUSE keep the one `rpm` database, and one provider serves
    /// both. So the token is the provider's to state, and routing an action to the provider a
    /// record names is the registry's to do (ADR-0559).
    ///
    /// `None` — the default — for a provider whose targets no second provider claims. Where two
    /// providers claim one target and its schema identifies by `provider`, each of them must
    /// answer, and with a different token; `cargo xtask spec-check` holds that over the
    /// declarations in `docs/contracts/providers/`.
    fn identity_token(&self) -> Option<&str> {
        None
    }

    /// The schemas it produces.
    fn schemas(&self) -> Vec<Arc<Schema>>;

    /// What it must be allowed to do.
    fn capabilities(&self) -> Vec<Capability>;

    /// Whether it can answer on this machine, and why not when it cannot.
    ///
    /// The default is available; a provider that depends on something that may be missing —
    /// systemd, a netlink socket, a container runtime — overrides this rather than failing later
    /// with an empty result.
    fn availability(&self) -> Availability {
        Availability::Available
    }

    /// The objects matching `query`, as they are now.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the query cannot be answered at all. A *partial* failure —
    /// one object that could not be read — belongs on the stream's error channel instead, so the
    /// objects that could be read still arrive (spec §16.5).
    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue>;

    /// Changes to the objects matching `query`, beginning with a snapshot of the current state.
    ///
    /// # Errors
    ///
    /// Returns `provider.unsupported` when the provider cannot watch. Saying so is required:
    /// a provider that silently polled instead would make its cost invisible, and spec §18.2
    /// requires that polling be explicit in metadata.
    fn subscribe(&self, query: &Query) -> Result<EventStream, ErrorValue> {
        let _ = query;
        Err(ErrorValue::new(
            ono_core::ErrorCode::ProviderUnsupported,
            format!("{} cannot watch for changes", self.id()),
        )
        .with_help("`watch` needs a provider that can subscribe or poll; this one does neither"))
    }

    /// What this provider can answer about time (spec v0.5 §21.1).
    ///
    /// The default claims nothing, which is the only honest default: a provider that has not
    /// declared a temporal capability has not implemented one, and nothing downstream may read
    /// its silence as coverage (§7.4, §21.5).
    fn temporal(&self) -> TemporalCapabilities {
        TemporalCapabilities::none()
    }

    /// The objects or events matching `query` as they were within `window` (spec v0.5 §21.4).
    ///
    /// Only a provider whose source genuinely keeps the past answers this — journald does, a
    /// container runtime's event database may, `/proc` does not (§22.1, §22.3). Everything else
    /// refuses, and the refusal is what lets the shell look for the answer somewhere it was
    /// recorded rather than silently present a snapshot of now as a snapshot of then.
    ///
    /// # Errors
    ///
    /// `temporal.unsupported_source` by default: the provider cannot answer about the past at
    /// all. A provider that can, and could not this time, reports whatever went wrong.
    fn history(&self, query: &Query, window: &TimeWindow) -> Result<ValueStream, ErrorValue> {
        let _ = (query, window);
        Err(unsupported_history(self.id()))
    }

    /// The objects a selector names.
    ///
    /// # Errors
    ///
    /// Returns a structured error when the selector cannot be resolved.
    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue>;

    /// Performs `action`, and reports exactly what happened to that object.
    ///
    /// # Errors
    ///
    /// Returns a structured error only when the provider cannot attempt the action at all — an
    /// operation it does not implement, or a capability it does not hold. An action that was
    /// attempted and failed is an [`ActionOutcome`], not an error, because a bulk operation must
    /// be able to report per-target outcomes (spec §11.5, §16.5).
    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let _ = action;
        Err(ErrorValue::new(
            ono_core::ErrorCode::ProviderUnsupported,
            format!("{} answers queries only", self.id()),
        ))
    }
}
