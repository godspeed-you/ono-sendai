//! Exact fixed-point decimal arithmetic (spec §10.2).
//!
//! Spec §10.2 marks `Decimal` optional and does not fix a representation. A shell that reports
//! sizes, rates and money-like configuration values needs a number type whose text form survives
//! a round trip unchanged, which binary floating point cannot promise. The representation here is
//! an `i128` mantissa scaled by a power of ten, which is exact for every literal a user can type
//! and needs no dependency.

use std::cmp::Ordering;
use std::fmt;

use ono_core::ErrorCode;

use crate::ErrorValue;

/// The largest power of ten that still fits in an `i128`.
const MAX_SCALE: u32 = 38;

/// An exact decimal number, held as `mantissa * 10^-scale`.
///
/// ```
/// use ono_value::Decimal;
/// let value = Decimal::parse("1.250")?;
/// assert_eq!(value.to_string(), "1.250");
/// assert_eq!(value, Decimal::parse("1.25")?);
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Decimal {
    mantissa: i128,
    scale: u32,
}

impl Decimal {
    /// The value zero.
    pub const ZERO: Self = Self {
        mantissa: 0,
        scale: 0,
    };

    /// Creates `mantissa * 10^-scale`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the scale exceeds what an `i128` can represent.
    pub fn new(mantissa: i128, scale: u32) -> Result<Self, ErrorValue> {
        if scale > MAX_SCALE {
            return Err(ErrorValue::new(
                ErrorCode::TypeInvalidUnit,
                format!("a decimal scale of {scale} exceeds the maximum of {MAX_SCALE}"),
            ));
        }
        Ok(Self { mantissa, scale })
    }

    /// Creates a decimal holding a whole number.
    #[must_use]
    pub const fn from_int(value: i128) -> Self {
        Self {
            mantissa: value,
            scale: 0,
        }
    }

    /// The unscaled mantissa.
    #[must_use]
    pub const fn mantissa(self) -> i128 {
        self.mantissa
    }

    /// The number of digits after the decimal point.
    #[must_use]
    pub const fn scale(self) -> u32 {
        self.scale
    }

    /// The value as a `f64`, which is lossy for more than 15 significant digits.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        self.mantissa as f64 / 10f64.powi(self.scale as i32)
    }

    /// Parses a decimal literal such as `-12.750`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] if the text is not a decimal literal.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let trimmed = text.trim();
        let (negative, digits) = match trimmed.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
        };
        let (integer, fraction) = match digits.split_once('.') {
            Some((integer, fraction)) => (integer, fraction),
            None => (digits, ""),
        };
        if integer.is_empty() && fraction.is_empty() {
            return Err(Self::parse_error(text));
        }
        if !integer
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
        {
            return Err(Self::parse_error(text));
        }
        let scale = u32::try_from(fraction.len()).map_err(|_| Self::parse_error(text))?;
        if scale > MAX_SCALE {
            return Err(Self::parse_error(text));
        }
        let mut mantissa: i128 = 0;
        for byte in integer.bytes().chain(fraction.bytes()) {
            mantissa = mantissa
                .checked_mul(10)
                .and_then(|value| value.checked_add(i128::from(byte - b'0')))
                .ok_or_else(|| Self::parse_error(text))?;
        }
        Ok(Self {
            mantissa: if negative { -mantissa } else { mantissa },
            scale,
        })
    }

    /// Adds two decimals exactly.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the exact result does not fit an `i128`.
    pub fn checked_add(self, other: Self) -> Result<Self, ErrorValue> {
        let (left, right, scale) = align(self, other)?;
        left.checked_add(right)
            .map(|mantissa| Self { mantissa, scale })
            .ok_or_else(overflow)
    }

    /// Subtracts two decimals exactly.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the exact result does not fit an `i128`.
    pub fn checked_sub(self, other: Self) -> Result<Self, ErrorValue> {
        let (left, right, scale) = align(self, other)?;
        left.checked_sub(right)
            .map(|mantissa| Self { mantissa, scale })
            .ok_or_else(overflow)
    }

    /// Multiplies two decimals exactly.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the exact result does not fit an `i128`.
    pub fn checked_mul(self, other: Self) -> Result<Self, ErrorValue> {
        let scale = self.scale + other.scale;
        if scale > MAX_SCALE {
            return Err(overflow());
        }
        self.mantissa
            .checked_mul(other.mantissa)
            .map(|mantissa| Self { mantissa, scale })
            .ok_or_else(overflow)
    }

    /// Divides two decimals, truncating toward zero at ten fractional digits.
    ///
    /// Division is the one operation that cannot stay exact, so the result scale is fixed rather
    /// than pretending otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeMismatch`] when dividing by zero and
    /// [`ErrorCode::TypeInvalidUnit`] when the result does not fit an `i128`.
    pub fn checked_div(self, other: Self) -> Result<Self, ErrorValue> {
        if other.mantissa == 0 {
            return Err(ErrorValue::new(ErrorCode::TypeMismatch, "division by zero"));
        }
        let result_scale = self.scale.max(other.scale).max(10);
        let shift = result_scale + other.scale - self.scale;
        if shift > MAX_SCALE {
            return Err(overflow());
        }
        let numerator = self
            .mantissa
            .checked_mul(pow10(shift).ok_or_else(overflow)?)
            .ok_or_else(overflow)?;
        Ok(Self {
            mantissa: numerator / other.mantissa,
            scale: result_scale,
        })
    }

    /// Negates the value.
    #[must_use]
    pub const fn negated(self) -> Self {
        Self {
            mantissa: self.mantissa.wrapping_neg(),
            scale: self.scale,
        }
    }

    fn parse_error(text: &str) -> ErrorValue {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{text}` is not a decimal literal"),
        )
    }
}

fn overflow() -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeInvalidUnit,
        "the exact decimal result does not fit in 128 bits",
    )
}

fn pow10(exponent: u32) -> Option<i128> {
    10i128.checked_pow(exponent)
}

/// Brings two decimals onto a common scale so their mantissas can be compared or combined.
fn align(left: Decimal, right: Decimal) -> Result<(i128, i128, u32), ErrorValue> {
    let scale = left.scale.max(right.scale);
    let left_mantissa = left
        .mantissa
        .checked_mul(pow10(scale - left.scale).ok_or_else(overflow)?)
        .ok_or_else(overflow)?;
    let right_mantissa = right
        .mantissa
        .checked_mul(pow10(scale - right.scale).ok_or_else(overflow)?)
        .ok_or_else(overflow)?;
    Ok((left_mantissa, right_mantissa, scale))
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.partial_cmp(other) == Some(Ordering::Equal)
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match align(*self, *other) {
            Ok((left, right, _)) => Some(left.cmp(&right)),
            // Only reachable for values whose scales differ by more than 38 decimal places, where
            // the sign and magnitude still decide the order.
            Err(_) => self.to_f64().partial_cmp(&other.to_f64()),
        }
    }
}

impl From<i128> for Decimal {
    fn from(value: i128) -> Self {
        Self::from_int(value)
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = self.mantissa.unsigned_abs().to_string();
        if self.mantissa < 0 {
            f.write_str("-")?;
        }
        let scale = self.scale as usize;
        if scale == 0 {
            return f.write_str(&digits);
        }
        let padded = if digits.len() <= scale {
            format!("{}{digits}", "0".repeat(scale - digits.len() + 1))
        } else {
            digits
        };
        let split = padded.len() - scale;
        write!(f, "{}.{}", &padded[..split], &padded[split..])
    }
}
