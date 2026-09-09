//! The refusals this provider raises, all of them built from `ono-change-core`'s constructors.
//!
//! §45 keeps every code and its metadata in one place, and `ono-change-core::error` is that place
//! for the whole change and recovery family. Nothing here invents a code: each function names the
//! core constructor it uses and supplies the Btrfs-specific sentence, so a caller matching on
//! `recovery.plan_incomplete` matches the same value whichever provider raised it.
//!
//! The one rule worth stating is §56.3's, because it decides which constructor a situation gets:
//! a fact that could not be **established** is [`fact_not_established`] and blocks destructive
//! recovery; a command that could not be **run** is a tool failure. Collapsing the two would let
//! a missing `btrfs` binary look like a proven absence of nested subvolumes.

use ono_change_core::error;
use ono_value::ErrorValue;

use crate::safety::SafetyFact;

/// The provider id §12.1 registers this provider under.
pub const PROVIDER_ID: &str = "ono.recovery.btrfs";

/// A `btrfs` invocation printed something this crate could not read (§12.3).
#[must_use]
pub fn unreadable_output(command: &str, detail: &str) -> ErrorValue {
    error::tool_failed(
        command,
        &format!(
            "the command ran and printed output this provider could not read, so nothing was \
             established from it. {detail}"
        ),
    )
}

/// A `btrfs` invocation failed (§12.3, Appendix G.4).
#[must_use]
pub fn command_failed(command: &str, detail: &str) -> ErrorValue {
    error::tool_failed(command, detail)
}

/// One of §56.2's ten facts could not be established, so recovery is blocked (§56.3).
#[must_use]
pub fn fact_not_established(fact: SafetyFact, detail: &str) -> ErrorValue {
    error::recovery_plan_incomplete(fact.description(), detail)
}

/// A protection action could not create its snapshot (§2.3, Appendix F.1).
#[must_use]
pub fn snapshot_failed(scope: &str, detail: &str) -> ErrorValue {
    error::asset_create_failed(PROVIDER_ID, scope, detail)
}

/// The configured snapshot location is nested inside a subvolume being snapshotted (Appendix D.8).
///
/// Appendix D.8 asks for a predictable recovery namespace on the same filesystem and forbids
/// placing it under a source subvolume "in a way that causes recursive operational confusion".
/// The confusion is concrete: a snapshot of `@` that contains `@/.snapshots` grows a nested
/// boundary of its own with every snapshot taken, and §14.3 then makes each of those an
/// uncovered hole inside the very asset that was supposed to cover the subvolume.
#[must_use]
pub fn recursive_snapshot_location(location: &str, subvolume: &str) -> ErrorValue {
    error::asset_create_failed(
        PROVIDER_ID,
        subvolume,
        &format!(
            "the configured snapshot location {location} is inside the subvolume {subvolume} that \
             would be snapshotted. Appendix D.8: the recovery namespace MUST NOT sit underneath a \
             source subvolume, because each snapshot then becomes a nested boundary inside the \
             next one and §14.3 makes every one of those a hole in the coverage. Nothing was \
             changed"
        ),
    )
}

/// A path is not on a Btrfs filesystem this provider can see (Appendix B.1).
#[must_use]
pub fn no_btrfs_mount(path: &str) -> ErrorValue {
    error::target_unresolved(
        path,
        "no Btrfs mount in this mount table contains the path, and Appendix B.1 resolves a \
         persistence domain from a mount rather than from a name",
    )
}
