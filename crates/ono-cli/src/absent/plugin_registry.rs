//! The command registry of a build without KUANG/11 (#127, ADR-0910).
//!
//! Mounted at `crate::plugin_registry` when the `kuang` feature is off. No package can declare a
//! command, so the registry is the embedded contracts, narrowed to the tiers this build carries.

use ono_command::CommandRegistry;
use ono_value::ErrorValue;

/// The registry this build advertises and runs.
///
/// # Errors
///
/// The structured error of an embedded contract that cannot be read, which is a build defect.
pub fn registry() -> Result<&'static CommandRegistry, ErrorValue> {
    crate::absent::registry()
}
