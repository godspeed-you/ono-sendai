//! Semantic scalars carrying a unit: byte sizes, durations and percentages (spec §10.6, §13.4).
//!
//! A unit-carrying value knows its dimension, so `512MiB > 1GiB` is a comparison and
//! `10s + 512MiB` is an error. Parsing normalises every unit of a dimension onto one base
//! quantity — bytes and nanoseconds — which is what makes automatic conversion free.
//!
//! Two renderings exist on purpose. [`Display`](std::fmt::Display) is the human form of spec
//! §13.4 (`1.20 GiB`, `4d 03h`) and is deliberately lossy; `exact` and `render_in` produce text
//! that parses back to the identical value.

use std::cmp::Ordering;
use std::fmt;

use ono_core::ErrorCode;

use crate::ErrorValue;

/// A unit of information, as spec §10.6 writes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ByteUnit {
    /// Bytes.
    B,
    /// Kibibytes, 2^10 bytes.
    KiB,
    /// Mebibytes, 2^20 bytes.
    MiB,
    /// Gibibytes, 2^30 bytes.
    GiB,
    /// Tebibytes, 2^40 bytes.
    TiB,
    /// Pebibytes, 2^50 bytes.
    PiB,
    /// Kilobytes, 10^3 bytes.
    KB,
    /// Megabytes, 10^6 bytes.
    MB,
    /// Gigabytes, 10^9 bytes.
    GB,
    /// Terabytes, 10^12 bytes.
    TB,
    /// Petabytes, 10^15 bytes.
    PB,
}

impl ByteUnit {
    /// Every byte unit, in the order spec §10.6 lists them.
    pub const ALL: &'static [ByteUnit] = &[
        ByteUnit::B,
        ByteUnit::KiB,
        ByteUnit::MiB,
        ByteUnit::GiB,
        ByteUnit::TiB,
        ByteUnit::PiB,
        ByteUnit::KB,
        ByteUnit::MB,
        ByteUnit::GB,
        ByteUnit::TB,
        ByteUnit::PB,
    ];

    /// The binary units, largest first, used to choose a human rendering.
    const BINARY_DESCENDING: &'static [ByteUnit] = &[
        ByteUnit::PiB,
        ByteUnit::TiB,
        ByteUnit::GiB,
        ByteUnit::MiB,
        ByteUnit::KiB,
        ByteUnit::B,
    ];

    /// The suffix a literal of this unit carries.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            ByteUnit::B => "B",
            ByteUnit::KiB => "KiB",
            ByteUnit::MiB => "MiB",
            ByteUnit::GiB => "GiB",
            ByteUnit::TiB => "TiB",
            ByteUnit::PiB => "PiB",
            ByteUnit::KB => "KB",
            ByteUnit::MB => "MB",
            ByteUnit::GB => "GB",
            ByteUnit::TB => "TB",
            ByteUnit::PB => "PB",
        }
    }

    /// How many bytes one of this unit is.
    #[must_use]
    pub const fn factor(self) -> u128 {
        match self {
            ByteUnit::B => 1,
            ByteUnit::KiB => 1 << 10,
            ByteUnit::MiB => 1 << 20,
            ByteUnit::GiB => 1 << 30,
            ByteUnit::TiB => 1 << 40,
            ByteUnit::PiB => 1 << 50,
            ByteUnit::KB => 1_000,
            ByteUnit::MB => 1_000_000,
            ByteUnit::GB => 1_000_000_000,
            ByteUnit::TB => 1_000_000_000_000,
            ByteUnit::PB => 1_000_000_000_000_000,
        }
    }

    /// Resolves a unit from its exact suffix. Matching is case-sensitive so `MB` and `MiB` can
    /// never be confused for one another.
    #[must_use]
    pub fn from_suffix(suffix: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|unit| unit.suffix() == suffix)
    }
}

impl fmt::Display for ByteUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// A quantity of information in bytes (spec §10.2).
///
/// ```
/// use ono_value::ByteSize;
/// assert_eq!(ByteSize::parse("3.5GiB")?.bytes(), 3_758_096_384);
/// assert_eq!(ByteSize::from_bytes(1_288_490_188).to_string(), "1.20 GiB");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ByteSize(u128);

impl ByteSize {
    /// No bytes at all.
    pub const ZERO: Self = Self(0);

    /// Creates a size from a byte count.
    #[must_use]
    pub const fn from_bytes(bytes: u128) -> Self {
        Self(bytes)
    }

    /// The size in bytes.
    #[must_use]
    pub const fn bytes(self) -> u128 {
        self.0
    }

    /// Parses a literal such as `128B`, `64KiB` or `3.5GiB`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the number, the suffix or the sign is not one a
    /// byte size can carry.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let (number, suffix) = split_literal(text).ok_or_else(|| unit_error(text, "byte size"))?;
        let unit = ByteUnit::from_suffix(suffix).ok_or_else(|| unit_error(text, "byte size"))?;
        if number.starts_with('-') {
            return Err(unit_error(text, "byte size"));
        }
        scale_to_base(number, unit.factor())
            .map(|bytes| Self(bytes.unsigned_abs()))
            .ok_or_else(|| unit_error(text, "byte size"))
    }

    /// The exact text form, always in bytes, which always parses back to this value.
    #[must_use]
    pub fn exact(self) -> String {
        format!("{}B", self.0)
    }

    /// Renders the size in a chosen unit, exactly when it is a whole multiple of that unit.
    #[must_use]
    pub fn render_in(self, unit: ByteUnit) -> String {
        let factor = unit.factor();
        if self.0.is_multiple_of(factor) {
            format!("{}{}", self.0 / factor, unit.suffix())
        } else {
            format!("{}{}", self.0 as f64 / factor as f64, unit.suffix())
        }
    }

    /// The largest binary unit that does not render the size as a fraction below one.
    #[must_use]
    pub fn human_unit(self) -> ByteUnit {
        ByteUnit::BINARY_DESCENDING
            .iter()
            .copied()
            .find(|unit| self.0 >= unit.factor())
            .unwrap_or(ByteUnit::B)
    }

    /// Adds two sizes.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the sum exceeds 128 bits.
    pub fn checked_add(self, other: Self) -> Result<Self, ErrorValue> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or_else(|| range_error("the sum of two byte sizes exceeds 128 bits"))
    }

    /// Subtracts two sizes.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the result would be negative, because a byte
    /// size has no negative values and silently clamping to zero would fabricate data.
    pub fn checked_sub(self, other: Self) -> Result<Self, ErrorValue> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or_else(|| range_error("a byte size cannot be negative"))
    }

    /// Scales the size by a factor.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the factor is negative or the result exceeds
    /// 128 bits.
    pub fn checked_scale(self, factor: f64) -> Result<Self, ErrorValue> {
        if !factor.is_finite() || factor < 0.0 {
            return Err(range_error("a byte size cannot be scaled by that factor"));
        }
        let scaled = self.0 as f64 * factor;
        if scaled > u128::MAX as f64 {
            return Err(range_error("the scaled byte size exceeds 128 bits"));
        }
        Ok(Self(scaled as u128))
    }
}

impl fmt::Display for ByteSize {
    /// The human form of spec §13.4: `128 B`, `1.20 GiB`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = self.human_unit();
        if unit == ByteUnit::B {
            write!(f, "{} B", self.0)
        } else {
            write!(
                f,
                "{:.2} {}",
                self.0 as f64 / unit.factor() as f64,
                unit.suffix()
            )
        }
    }
}

/// A unit of time, as spec §10.6 writes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DurationUnit {
    /// Nanoseconds.
    Nanoseconds,
    /// Microseconds.
    Microseconds,
    /// Milliseconds.
    Milliseconds,
    /// Seconds.
    Seconds,
    /// Minutes.
    Minutes,
    /// Hours.
    Hours,
    /// Days.
    Days,
    /// Weeks.
    Weeks,
}

impl DurationUnit {
    /// Every duration unit, smallest first.
    pub const ALL: &'static [DurationUnit] = &[
        DurationUnit::Nanoseconds,
        DurationUnit::Microseconds,
        DurationUnit::Milliseconds,
        DurationUnit::Seconds,
        DurationUnit::Minutes,
        DurationUnit::Hours,
        DurationUnit::Days,
        DurationUnit::Weeks,
    ];

    /// The suffix a literal of this unit carries.
    #[must_use]
    pub const fn suffix(self) -> &'static str {
        match self {
            DurationUnit::Nanoseconds => "ns",
            DurationUnit::Microseconds => "us",
            DurationUnit::Milliseconds => "ms",
            DurationUnit::Seconds => "s",
            DurationUnit::Minutes => "m",
            DurationUnit::Hours => "h",
            DurationUnit::Days => "d",
            DurationUnit::Weeks => "w",
        }
    }

    /// How many nanoseconds one of this unit is.
    #[must_use]
    pub const fn nanoseconds(self) -> i128 {
        match self {
            DurationUnit::Nanoseconds => 1,
            DurationUnit::Microseconds => 1_000,
            DurationUnit::Milliseconds => 1_000_000,
            DurationUnit::Seconds => 1_000_000_000,
            DurationUnit::Minutes => 60 * 1_000_000_000,
            DurationUnit::Hours => 60 * 60 * 1_000_000_000,
            DurationUnit::Days => 24 * 60 * 60 * 1_000_000_000,
            DurationUnit::Weeks => 7 * 24 * 60 * 60 * 1_000_000_000,
        }
    }

    /// Resolves a unit from its exact suffix.
    #[must_use]
    pub fn from_suffix(suffix: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|unit| unit.suffix() == suffix)
    }
}

impl fmt::Display for DurationUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// A signed span of time at nanosecond resolution (spec §10.2).
///
/// Durations are signed because subtracting two timestamps must be able to run backwards.
///
/// ```
/// use ono_value::Duration;
/// assert_eq!(Duration::parse("1h30m")?, Duration::parse("90m")?);
/// assert_eq!(Duration::parse("4d 3h")?.to_string(), "4d 03h");
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Duration(i128);

impl Duration {
    /// No time at all.
    pub const ZERO: Self = Self(0);

    /// Creates a duration from a signed nanosecond count.
    #[must_use]
    pub const fn from_nanoseconds(nanoseconds: i128) -> Self {
        Self(nanoseconds)
    }

    /// The duration in signed nanoseconds.
    #[must_use]
    pub const fn nanoseconds(self) -> i128 {
        self.0
    }

    /// The duration in fractional seconds, for arithmetic that has left the exact domain.
    #[must_use]
    pub fn as_seconds_f64(self) -> f64 {
        self.0 as f64 / 1e9
    }

    /// Whether the duration runs backwards.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// Parses a literal such as `250ms`, `10s`, `1h30m` or `-4d 3h`.
    ///
    /// A compound literal is the sum of its terms, so the human rendering of spec §13.4 parses
    /// back.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if a term or a suffix is not one a duration can
    /// carry.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let trimmed = text.trim();
        let (negative, mut rest) = match trimmed.strip_prefix('-') {
            Some(rest) => (true, rest.trim_start()),
            None => (false, trimmed),
        };
        if rest.is_empty() {
            return Err(unit_error(text, "duration"));
        }
        let mut total: i128 = 0;
        let mut terms = 0_u32;
        while !rest.is_empty() {
            let number_len = rest
                .find(|c: char| !(c.is_ascii_digit() || c == '.'))
                .unwrap_or(rest.len());
            let (number, tail) = rest.split_at(number_len);
            let suffix_len = tail
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(tail.len());
            let (suffix, tail) = tail.split_at(suffix_len);
            let unit =
                DurationUnit::from_suffix(suffix).ok_or_else(|| unit_error(text, "duration"))?;
            let term = scale_to_base(number, unit.nanoseconds().unsigned_abs())
                .ok_or_else(|| unit_error(text, "duration"))?;
            total = total
                .checked_add(term)
                .ok_or_else(|| unit_error(text, "duration"))?;
            terms += 1;
            rest = tail.trim_start();
        }
        if terms == 0 {
            return Err(unit_error(text, "duration"));
        }
        Ok(Self(if negative { -total } else { total }))
    }

    /// The exact text form, always in nanoseconds, which always parses back to this value.
    #[must_use]
    pub fn exact(self) -> String {
        format!("{}ns", self.0)
    }

    /// Renders the duration in a chosen unit, exactly when it is a whole multiple of that unit.
    #[must_use]
    pub fn render_in(self, unit: DurationUnit) -> String {
        let factor = unit.nanoseconds();
        if self.0 % factor == 0 {
            format!("{}{}", self.0 / factor, unit.suffix())
        } else {
            format!("{}{}", self.0 as f64 / factor as f64, unit.suffix())
        }
    }

    /// Adds two durations.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the sum exceeds 128 bits.
    pub fn checked_add(self, other: Self) -> Result<Self, ErrorValue> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or_else(|| range_error("the sum of two durations exceeds 128 bits"))
    }

    /// Subtracts two durations.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the difference exceeds 128 bits.
    pub fn checked_sub(self, other: Self) -> Result<Self, ErrorValue> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or_else(|| range_error("the difference of two durations exceeds 128 bits"))
    }

    /// Scales the duration by a factor.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the factor is not finite or the result exceeds
    /// 128 bits.
    pub fn checked_scale(self, factor: f64) -> Result<Self, ErrorValue> {
        if !factor.is_finite() {
            return Err(range_error("a duration cannot be scaled by that factor"));
        }
        let scaled = self.0 as f64 * factor;
        if scaled.abs() > i128::MAX as f64 {
            return Err(range_error("the scaled duration exceeds 128 bits"));
        }
        Ok(Self(scaled as i128))
    }
}

impl fmt::Display for Duration {
    /// The human form of spec §13.4: `4d 03h`, `843ms`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 < 0 {
            f.write_str("-")?;
        }
        let nanos = self.0.unsigned_abs();
        let day = DurationUnit::Days.nanoseconds().unsigned_abs();
        let hour = DurationUnit::Hours.nanoseconds().unsigned_abs();
        let minute = DurationUnit::Minutes.nanoseconds().unsigned_abs();
        let second = DurationUnit::Seconds.nanoseconds().unsigned_abs();
        let milli = DurationUnit::Milliseconds.nanoseconds().unsigned_abs();
        let micro = DurationUnit::Microseconds.nanoseconds().unsigned_abs();

        if nanos == 0 {
            f.write_str("0s")
        } else if nanos >= day {
            write!(f, "{}d {:02}h", nanos / day, (nanos % day) / hour)
        } else if nanos >= hour {
            write!(f, "{}h {:02}m", nanos / hour, (nanos % hour) / minute)
        } else if nanos >= minute {
            write!(f, "{}m {:02}s", nanos / minute, (nanos % minute) / second)
        } else if nanos >= second {
            write!(f, "{:.2}s", nanos as f64 / second as f64)
        } else if nanos >= milli {
            write!(f, "{}ms", nanos / milli)
        } else if nanos >= micro {
            write!(f, "{}us", nanos / micro)
        } else {
            write!(f, "{nanos}ns")
        }
    }
}

/// A percentage, where `Percent::new(24.8)` renders as `24.8%` (spec §13.2, §27.3).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default)]
pub struct Percent(f64);

impl Percent {
    /// Zero percent.
    pub const ZERO: Self = Self(0.0);

    /// Creates a percentage from a number of percent, not from a fraction.
    #[must_use]
    pub const fn new(percent: f64) -> Self {
        Self(percent)
    }

    /// The number of percent.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }

    /// The value as a fraction of one, so `Percent::new(50.0).as_fraction()` is `0.5`.
    #[must_use]
    pub fn as_fraction(self) -> f64 {
        self.0 / 100.0
    }

    /// Parses a literal such as `24.8%`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::TypeInvalidUnit`] if the text is not a percentage literal.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let trimmed = text.trim();
        let number = trimmed
            .strip_suffix('%')
            .ok_or_else(|| unit_error(text, "percentage"))?;
        number
            .trim()
            .parse::<f64>()
            .map(Self)
            .map_err(|_| unit_error(text, "percentage"))
    }

    /// Adds two percentages.
    #[must_use]
    pub fn plus(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }

    /// Subtracts two percentages.
    #[must_use]
    pub fn minus(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }

    /// Scales the percentage by a factor.
    #[must_use]
    pub fn scale(self, factor: f64) -> Self {
        Self(self.0 * factor)
    }

    /// Orders two percentages, or reports that one of them is not a number.
    #[must_use]
    pub fn compare(self, other: Self) -> Option<Ordering> {
        self.0.partial_cmp(&other.0)
    }
}

impl fmt::Display for Percent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.0)
    }
}

/// Splits `3.5GiB` into `("3.5", "GiB")`, or reports that there is no unit suffix at all.
fn split_literal(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.trim();
    let split = trimmed.find(|c: char| c.is_ascii_alphabetic())?;
    let number = trimmed[..split].trim();
    if number.is_empty() {
        return None;
    }
    Some((number, trimmed[split..].trim()))
}

/// Multiplies a decimal literal by a base factor, exactly when the literal is a whole number.
fn scale_to_base(number: &str, factor: u128) -> Option<i128> {
    let factor = i128::try_from(factor).ok()?;
    if let Ok(whole) = number.parse::<i128>() {
        return whole.checked_mul(factor);
    }
    let scaled = crate::Decimal::parse(number)
        .ok()?
        .checked_mul(crate::Decimal::from_int(factor))
        .ok()?;
    Some(scaled.mantissa() / 10i128.checked_pow(scaled.scale())?)
}

fn unit_error(text: &str, dimension: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeInvalidUnit,
        format!("`{text}` is not a {dimension} literal"),
    )
}

fn range_error(message: &str) -> ErrorValue {
    ErrorValue::new(ErrorCode::TypeInvalidUnit, message)
}
