//! Appendix D.3's free-space floor, as §53's `recovery.min_filesystem_free` states it.
//!
//! §53 writes the default as `"10%"`, a share of the pool, and the setting also takes a quantity
//! such as `2GiB`. The two are different claims about the same pool: a share scales with the pool
//! and a quantity does not. [`FreeSpaceFloor`] keeps them apart until the pool's size has been
//! read, so a share is never turned into bytes against a pool nobody measured.

use std::fmt;

use ono_core::ErrorCode;
use ono_value::{ByteSize, ErrorValue, Percent};

/// The floor below which automatic protection fails closed (Appendix D.3, §53).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum FreeSpaceFloor {
    /// No floor is configured, and nothing is refused for space.
    #[default]
    None,
    /// At least this many bytes stay free in the pool.
    Bytes(ByteSize),
    /// At least this share of the pool's size stays free.
    Share(Percent),
    /// A share and a quantity together, both in force, as §53's floor arrives when the setting
    /// states one and policy adds the other: the stricter of the two at the pool's size.
    Both {
        /// The share of the pool that must stay free.
        share: Percent,
        /// The quantity that must stay free.
        bytes: ByteSize,
    },
}

impl FreeSpaceFloor {
    /// Reads a setting value: a share such as `10%` or a quantity such as `2GiB`.
    ///
    /// `0%` and `0B` are no floor at all.
    ///
    /// # Errors
    ///
    /// `type.invalid_unit` when the text is neither a percentage nor a byte quantity, or when a
    /// share lies outside 0% to 100% — no pool keeps more than all of itself free.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let trimmed = text.trim();
        if trimmed.ends_with('%') {
            let share = Percent::parse(trimmed)?;
            let value = share.value();
            if !value.is_finite() || !(0.0..=100.0).contains(&value) {
                return Err(ErrorValue::new(
                    ErrorCode::TypeInvalidUnit,
                    format!(
                        "`{trimmed}` is not a free-space floor: a share of a pool lies between 0% \
                         and 100%"
                    ),
                ));
            }
            return Ok(if value <= 0.0 {
                Self::None
            } else {
                Self::Share(share)
            });
        }
        let bytes = ByteSize::parse(trimmed)?;
        Ok(if bytes == ByteSize::ZERO {
            Self::None
        } else {
            Self::Bytes(bytes)
        })
    }

    /// The bytes that must stay free in a pool of `size` bytes.
    ///
    /// `None` when the floor is a share and the pool's size is unknown: the floor then cannot be
    /// met by a pool nobody measured, which §56.3 makes a refusal rather than a pass.
    #[must_use]
    pub fn minimum_bytes(self, size: Option<u128>) -> Option<u128> {
        match self {
            Self::None => Some(0),
            Self::Bytes(bytes) => Some(bytes.bytes()),
            // A share is rounded up: a floor that rounds in the pool's favour is not the floor.
            Self::Share(share) => {
                size.map(|size| (size as f64 * share.as_fraction()).ceil() as u128)
            }
            // Both parts are in force, so the pool keeps whichever demands more. A share nobody could
            // measure leaves the floor unknown, and an unknown floor is not met (§56.3).
            Self::Both { share, bytes } => Self::Share(share)
                .minimum_bytes(size)
                .map(|shared| shared.max(bytes.bytes())),
        }
    }

    /// The floor as a refusal states it, with the size a share was taken of where it is known.
    #[must_use]
    pub fn describe(self, size: Option<u128>) -> String {
        match (self, size) {
            (Self::Share(_), Some(size)) => {
                format!("{self} of the pool's {}", ByteSize::from_bytes(size))
            }
            (Self::Share(_), None) => format!("{self} of a pool whose size was not read"),
            (Self::Both { .. }, Some(size)) => {
                format!("{self} of the pool's {}", ByteSize::from_bytes(size))
            }
            (Self::Both { .. }, None) => format!("{self} of a pool whose size was not read"),
            _ => self.to_string(),
        }
    }
}

impl fmt::Display for FreeSpaceFloor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => formatter.write_str("none"),
            Self::Bytes(bytes) => write!(formatter, "{bytes}"),
            Self::Share(share) => write!(formatter, "{}%", share.value()),
            Self::Both { share, bytes } => {
                write!(formatter, "{}% and at least {bytes}", share.value())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn both() -> FreeSpaceFloor {
        FreeSpaceFloor::Both {
            share: Percent::parse("10%").unwrap(),
            bytes: ByteSize::parse("2GiB").unwrap(),
        }
    }

    #[test]
    fn should_keep_the_quantity_free_when_it_is_the_larger_part_of_both() {
        let pool = 10 * 1024 * 1024 * 1024;
        assert_eq!(
            both().minimum_bytes(Some(pool)),
            Some(2 * 1024 * 1024 * 1024)
        );
    }

    #[test]
    fn should_keep_the_share_free_when_it_is_the_larger_part_of_both() {
        let pool = 100 * 1024 * 1024 * 1024;
        assert_eq!(
            both().minimum_bytes(Some(pool)),
            Some(10 * 1024 * 1024 * 1024)
        );
    }

    #[test]
    fn should_leave_both_unmet_against_a_pool_nobody_measured() {
        assert_eq!(
            both().minimum_bytes(None),
            None,
            "§56.3: a share of an unknown size is not a floor anyone met"
        );
    }
}
