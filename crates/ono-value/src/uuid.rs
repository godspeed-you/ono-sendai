//! The `Uuid` semantic scalar of spec §10.2.
//!
//! A UUID is sixteen opaque bytes with one canonical text form. That is small enough to own here
//! rather than to take a dependency for, and owning it keeps the text form under the control of
//! the round-trip tests.

use std::fmt;

use ono_core::ErrorCode;

use crate::ErrorValue;

/// A universally unique identifier, held as its sixteen bytes.
///
/// ```
/// use ono_value::Uuid;
/// let id = Uuid::parse("0191f0e2-7c4a-7b3d-8e91-2a5c6f7d8e9f")?;
/// assert_eq!(id.to_string(), "0191f0e2-7c4a-7b3d-8e91-2a5c6f7d8e9f");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// The all-zero UUID.
    pub const NIL: Self = Self([0; 16]);

    /// Creates a UUID from its sixteen bytes, in network order.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// The sixteen bytes, in network order.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Parses the canonical hyphenated form, in either letter case.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::ParseSyntax`] if the text is not a canonical UUID.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let error = || ErrorValue::new(ErrorCode::ParseSyntax, format!("`{text}` is not a UUID"));
        let groups: Vec<&str> = text.split('-').collect();
        let widths: Vec<usize> = groups.iter().map(|group| group.len()).collect();
        if widths != [8, 4, 4, 4, 12] {
            return Err(error());
        }
        let digits: String = groups.concat();
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(error());
        }
        let mut bytes = [0_u8; 16];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let pair = digits.get(index * 2..index * 2 + 2).ok_or_else(error)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| error())?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, byte) in self.0.iter().enumerate() {
            if matches!(index, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
