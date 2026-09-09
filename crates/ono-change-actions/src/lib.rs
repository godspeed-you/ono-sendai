//! The first-party change providers of Ono-Sendai v0.6 (spec §6, §8, §30, §32, §33, §34, §51).
//!
//! `ono_change_core::ChangeProvider` is a declaration, and this crate is the thing that answers
//! it. Without one, `plan restart service nginx` has nothing to turn an intent into plan actions,
//! and the rest of v0.6 has nothing to plan over.
//!
//! # What makes an operation plannable
//!
//! §6.1 lists nine things a provider contract must declare before Ono will plan an operation.
//! Six of them are already stated somewhere the shell reads: `docs/contracts/commands/*.yaml`
//! declares the target schema, the required provider capability, the privilege and the arguments;
//! `docs/contracts/verbs.yaml` says which verbs mutate; [`ono_provider_api::Provider::act`] is the
//! execution method and [`ono_provider_api::ActionOutcome`] the report.
//!
//! The other three — the expected direct effects, the idempotency class and the recovery
//! semantics — are facts about the operation rather than about its spelling, and nothing that
//! exists can derive them. So they are data: `plannable_operations:` in
//! `docs/contracts/change/actions.yaml`, read by [`registry`] and embedded at build time.
//!
//! **An operation with no row is not plannable.** That is §6.2's answer for `plan sh -c '…'` and
//! the same answer for a mutating command nobody has written a contract for yet. [`opaque`] holds
//! both halves of §6.2: the refusal, and §6.2's second sentence — an external-command adapter may
//! expose a plannable action where its contract is explicit.
//!
//! # What a resolved plan carries
//!
//! [`provider::ProviderChangeProvider`] is the bridge. `supports` and `resolve` reach the world
//! through an [`preconditions::Observer`], which answers questions and cannot change anything, so
//! §51's prohibition on mutating during either is a property of the shape rather than a rule
//! somebody has to remember. `execute` is the only method that can call
//! [`ono_provider_api::Provider::act`], and it goes through [`bridge::Bridge`].
//!
//! Every action it builds carries §7.2's preconditions ([`preconditions`]), §8's effects at
//! §8.1's four confidence classes and no probability, §41.1's idempotency class, §6.1's recovery
//! semantics, and the verification contracts §23.1 requires — "even if the minimum verification
//! is only provider-level state acknowledgement", which is `exists` read back through
//! [`ono_provider_api::Provider::snapshot`].

#![forbid(unsafe_code)]

mod intent;

pub mod bridge;
pub mod opaque;
pub mod preconditions;
pub mod provider;
pub mod registry;

pub use bridge::{Bridge, RuntimeBridge};
pub use opaque::{AdapterAction, opaque_action, plan_external, refuse};
pub use preconditions::Observer;
pub use provider::ProviderChangeProvider;
pub use registry::{
    EffectSpec, Operation, OperationRegistry, RebootDimension, RebootRequirement, Tolerance,
    VerificationSpec,
};
