//! The names a coverage claim is filed under (v0.5 §8.1).
//!
//! §8.1 makes coverage a claim about a "object/relationship capability" rather than about a
//! whole reconstruction, and the specification's own examples are `process.existence`,
//! `service.state` and `relation:process.owns_socket`. Spelling those by hand in three crates is
//! how a recorder's coverage stops matching a reconstruction's lookup, so they are built here
//! and nowhere else.

use std::sync::Arc;

use ono_spatial_core::SpatialType;

/// Whether objects of this type existed — `process.existence`.
#[must_use]
pub fn existence(object_type: SpatialType) -> Arc<str> {
    Arc::from(format!("{}.existence", lowercase(object_type)))
}

/// What one field of this type held — `service.state`.
#[must_use]
pub fn field(object_type: SpatialType, name: &str) -> Arc<str> {
    Arc::from(format!("{}.{name}", lowercase(object_type)))
}

/// Whether one relation held — `relation:process.owns_socket`.
#[must_use]
pub fn relation(name: &str) -> Arc<str> {
    Arc::from(format!("relation:{name}"))
}

/// The type's name as a capability spells it.
///
/// [`SpatialType::as_str`] is the display spelling — `Process`, `BlockDevice` — and a capability
/// is lowercase, so the two are one mechanical step apart rather than two lists to keep in sync.
fn lowercase(object_type: SpatialType) -> String {
    object_type.as_str().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_spell_the_capabilities_the_specification_shows_when_a_name_is_built() {
        assert_eq!(&*existence(SpatialType::Process), "process.existence");
        assert_eq!(&*field(SpatialType::Service, "state"), "service.state");
        assert_eq!(
            &*relation("process.owns_socket"),
            "relation:process.owns_socket"
        );
    }
}
