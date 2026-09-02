//! The shared resource budget of spec v0.4.1 §21.
//!
//! §21.1 asks for **one** abstraction behind every operation that retains or materializes values,
//! and names the way such an abstraction is normally defeated: *"Production interactive paths MUST
//! NOT accidentally obtain an unlimited budget through a default constructor."* [`Budget`]
//! therefore cannot express an unlimited ceiling at all — see ADR-0453 — and implements no
//! `Default`.
//!
//! §21.3 fixes what happens at the ceiling. A budget **refuses**; it never keeps collecting while
//! warning. A cache that evicts instead — result history, §24.2 — reads the same consumption
//! figures and applies its own documented policy, so the two behaviours are chosen explicitly by
//! the caller and never mixed by accident.

use std::fmt;
use std::sync::Arc;

use ono_core::ErrorCode;

use crate::{ErrorValue, Value, estimated_size};

/// Values a default global materializer may collect (v0.4.1 §22.2, Appendix A).
pub const MATERIALIZE_MAX_ITEMS: u64 = 100_000;

/// Bytes a default global materializer may collect (v0.4.1 §22.2, Appendix A: 128 MiB).
pub const MATERIALIZE_MAX_BYTES: u64 = 134_217_728;

/// Bytes every capture inside one shell command may retain together (§23.4, Appendix A: 256 MiB).
///
/// A ceiling across nested capture contexts, not an allowance each capture may spend in full.
pub const COMMAND_CAPTURE_MAX_BYTES: u64 = 268_435_456;

/// Values every capture inside one shell command may retain together (§23.2, §23.4).
///
/// §23.2 gives captures the global materialization defaults unless a narrower bound is defined.
/// §23.4 defines a narrower *byte* bound and leaves the item bound alone, so the command ceiling
/// carries the global item figure.
pub const COMMAND_CAPTURE_MAX_ITEMS: u64 = MATERIALIZE_MAX_ITEMS;

/// The two ceilings every materializer in one pipeline enforces (§22.2, §55.1).
///
/// Carried by [`PipelineConfig`](https://docs.rs/ono-pipeline) so the configured figures reach every
/// blocking stage without each transform growing its own setting. Appendix A calls them "hard per
/// materializer", so each stage mints its own [`Budget`] from these and spends it alone.
///
/// This type does implement [`Default`], and that is not the hazard §21.1 names: its default is
/// Appendix A's finite pair, and it cannot express an unlimited one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterializationLimits {
    max_items: u64,
    max_bytes: u64,
}

impl Default for MaterializationLimits {
    fn default() -> Self {
        Self {
            max_items: MATERIALIZE_MAX_ITEMS,
            max_bytes: MATERIALIZE_MAX_BYTES,
        }
    }
}

impl MaterializationLimits {
    /// Limits of `max_items` values and `max_bytes` bytes, both stated.
    #[must_use]
    pub const fn new(max_items: u64, max_bytes: u64) -> Self {
        Self {
            max_items,
            max_bytes,
        }
    }

    /// The number of values one materializer may collect.
    #[must_use]
    pub const fn max_items(self) -> u64 {
        self.max_items
    }

    /// The number of bytes one materializer may collect.
    #[must_use]
    pub const fn max_bytes(self) -> u64 {
        self.max_bytes
    }

    /// A fresh budget for `stage`, spending these limits.
    #[must_use]
    pub fn budget_for(self, stage: &str) -> Budget {
        Budget::of(stage, self.max_items, self.max_bytes)
    }
}

/// Which of a budget's two ceilings a refusal is about (§21.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ceiling {
    /// The number of values retained.
    Items,
    /// The number of bytes retained, estimated as §21.2 estimates them.
    Bytes,
}

impl Ceiling {
    /// The error code this ceiling raises (§21.4, §53.1).
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Ceiling::Items => ErrorCode::ResourceItemLimit,
            Ceiling::Bytes => ErrorCode::ResourceByteLimit,
        }
    }

    const fn noun(self) -> &'static str {
        match self {
            Ceiling::Items => "values",
            Ceiling::Bytes => "bytes",
        }
    }
}

/// A budget ceiling that was reached: which one, what it was, and what crossed it.
///
/// Deliberately not an [`ErrorValue`] yet. §21.3 gives a caller two lawful responses — stop with a
/// structured error, or evict per a documented cache policy — and a type that had already decided
/// which one applied would be mixing them implicitly. [`Exceeded::into_error`] takes the first
/// branch; `ono-history` takes the second and never calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exceeded {
    stage: Arc<str>,
    ceiling: Ceiling,
    configured: u64,
    observed: u64,
    setting: &'static str,
}

impl Exceeded {
    /// Which ceiling was reached.
    #[must_use]
    pub const fn ceiling(&self) -> Ceiling {
        self.ceiling
    }

    /// The ceiling as it was configured.
    #[must_use]
    pub const fn configured(&self) -> u64 {
        self.configured
    }

    /// The consumption that would have crossed the ceiling, had the value been admitted.
    #[must_use]
    pub const fn observed(&self) -> u64 {
        self.observed
    }

    /// The operation that was enforcing the ceiling, as a user would name it.
    #[must_use]
    pub fn stage(&self) -> &str {
        &self.stage
    }

    /// The configuration key a user raises to permit more (§55.1).
    #[must_use]
    pub const fn setting(&self) -> &'static str {
        self.setting
    }

    /// The refusal as a structured error (§21.4, §53.1).
    ///
    /// The metadata carries the ceiling, the configured limit and the observed consumption, and
    /// **not the retained values**: §53.3 keeps secrets out of error details, and §21.4 keeps the
    /// payload out, because a resource error that prints what it was holding is a second resource
    /// problem.
    #[must_use]
    pub fn into_error(self) -> ErrorValue {
        let Exceeded {
            stage,
            ceiling,
            configured,
            observed,
            setting,
        } = self;
        let rendered = match ceiling {
            Ceiling::Items => format!("{configured} values"),
            Ceiling::Bytes => format!("{configured} bytes ({})", human_bytes(configured)),
        };
        ErrorValue::new(
            ceiling.code(),
            format!(
                "{stage} reached its {rendered} budget after {observed} {}",
                ceiling.noun()
            ),
        )
        .with_help(format!(
            "narrow the input, or raise `{setting}` deliberately"
        ))
        .with_metadata("stage", Value::String(stage))
        .with_metadata("ceiling", Value::string(ceiling.noun()))
        .with_metadata("limit", Value::Int(i128::from(configured)))
        .with_metadata("consumed", Value::Int(i128::from(observed)))
        .with_metadata("setting", Value::string(setting))
    }
}

impl fmt::Display for Exceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} reached its {} {} budget after {}",
            self.stage,
            self.configured,
            self.ceiling.noun(),
            self.observed
        )
    }
}

/// What one operation may retain: a ceiling in values, a ceiling in bytes, and what it has spent.
///
/// Spec v0.4.1 §21.1's model, with one deliberate difference: the ceilings are `u64` rather than
/// `Option<u64>`. §21.1 permits `None` "only for internal/test contexts where unboundedness is
/// explicit in the type or constructor name", and the surest way to keep an unlimited budget out
/// of a production path is for the type to be unable to hold one (ADR-0453). A ceiling of zero
/// means "no values permitted", which is §22.2's rule and not a way back to unlimited.
///
/// ```
/// use ono_value::{Budget, Value};
///
/// let mut budget = Budget::of("collect", 2, 4096);
/// budget.charge(&Value::Int(1))?;
/// budget.charge(&Value::Int(2))?;
/// let refused = budget.charge(&Value::Int(3)).unwrap_err();
/// assert_eq!(refused.configured(), 2);
/// # Ok::<(), ono_value::Exceeded>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    stage: Arc<str>,
    max_items: u64,
    max_bytes: u64,
    consumed_items: u64,
    consumed_bytes: u64,
    item_setting: &'static str,
    byte_setting: &'static str,
}

impl Budget {
    /// A budget for `stage` with both ceilings stated.
    ///
    /// There is no constructor that states only one, and none that states neither.
    #[must_use]
    pub fn of(stage: &str, max_items: u64, max_bytes: u64) -> Self {
        Self {
            stage: Arc::from(stage),
            max_items,
            max_bytes,
            consumed_items: 0,
            consumed_bytes: 0,
            item_setting: "limits.materialize_items",
            byte_setting: "limits.materialize_bytes",
        }
    }

    /// Names the configuration keys a refusal from this budget should point the user at (§55.1).
    ///
    /// A refusal that told a user to raise the materialization limit when the ceiling they hit
    /// was the capture ceiling would send them to change a number that cannot help them.
    #[must_use]
    pub const fn for_settings(mut self, items: &'static str, bytes: &'static str) -> Self {
        self.item_setting = items;
        self.byte_setting = bytes;
        self
    }

    /// The global materialization defaults of §22.2 and Appendix A, for `stage`.
    #[must_use]
    pub fn materialization(stage: &str) -> Self {
        Self::of(stage, MATERIALIZE_MAX_ITEMS, MATERIALIZE_MAX_BYTES)
    }

    /// The aggregate ceiling every capture inside one shell command shares (§23.4, Appendix A).
    #[must_use]
    pub fn command_captures() -> Self {
        Self::of(
            "this command's captures",
            COMMAND_CAPTURE_MAX_ITEMS,
            COMMAND_CAPTURE_MAX_BYTES,
        )
        .for_settings("limits.materialize_items", "limits.command_capture_bytes")
    }

    /// The number of values this budget permits.
    #[must_use]
    pub const fn max_items(&self) -> u64 {
        self.max_items
    }

    /// The number of bytes this budget permits, as §21.2 estimates bytes.
    #[must_use]
    pub const fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// The values admitted so far.
    #[must_use]
    pub const fn consumed_items(&self) -> u64 {
        self.consumed_items
    }

    /// The bytes admitted so far.
    #[must_use]
    pub const fn consumed_bytes(&self) -> u64 {
        self.consumed_bytes
    }

    /// The operation this budget belongs to, as a user would name it.
    #[must_use]
    pub fn stage(&self) -> &str {
        &self.stage
    }

    /// Admits `value`, or refuses because a ceiling would be crossed.
    ///
    /// # Errors
    ///
    /// Returns [`Exceeded`] naming the ceiling that stopped it. Nothing is admitted when it
    /// refuses: a budget that recorded the value it just refused would be collecting while
    /// warning, which §21.3 forbids.
    pub fn charge(&mut self, value: &Value) -> Result<(), Exceeded> {
        self.charge_estimate(estimated_size(value))
    }

    /// Admits a payload of `bytes` counted as one value.
    ///
    /// For a caller that already knows the size of what it retains — a byte buffer captured from
    /// a child process — and would otherwise estimate a `Value` it built only to measure it.
    ///
    /// # Errors
    ///
    /// As [`charge`](Self::charge).
    pub fn charge_estimate(&mut self, bytes: u64) -> Result<(), Exceeded> {
        let items = self.consumed_items.saturating_add(1);
        if items > self.max_items {
            return Err(self.exceeded(Ceiling::Items, self.max_items, items));
        }
        let consumed = self.consumed_bytes.saturating_add(bytes);
        if consumed > self.max_bytes {
            return Err(self.exceeded(Ceiling::Bytes, self.max_bytes, consumed));
        }
        self.consumed_items = items;
        self.consumed_bytes = consumed;
        Ok(())
    }

    /// A budget for a nested operation, drawn from what remains of this one (§23.4).
    ///
    /// The child cannot be larger than its parent's remainder, so nested captures share one
    /// allowance instead of each starting again at the global default. The parent is charged for
    /// what the child spent when the child is handed back to [`absorb`](Self::absorb).
    #[must_use]
    pub fn child(&self, stage: &str) -> Budget {
        Self::of(
            stage,
            self.max_items.saturating_sub(self.consumed_items),
            self.max_bytes.saturating_sub(self.consumed_bytes),
        )
        .for_settings(self.item_setting, self.byte_setting)
    }

    /// Charges this budget for everything `child` spent (§23.4).
    ///
    /// Saturating rather than refusing: the child already refused whatever exceeded the allowance
    /// it was given, so there is nothing left to refuse here.
    pub fn absorb(&mut self, child: Budget) {
        self.consumed_items = self.consumed_items.saturating_add(child.consumed_items);
        self.consumed_bytes = self.consumed_bytes.saturating_add(child.consumed_bytes);
    }

    fn exceeded(&self, ceiling: Ceiling, configured: u64, observed: u64) -> Exceeded {
        Exceeded {
            stage: Arc::clone(&self.stage),
            ceiling,
            configured,
            observed,
            setting: match ceiling {
                Ceiling::Items => self.item_setting,
                Ceiling::Bytes => self.byte_setting,
            },
        }
    }
}

/// Renders a byte count the way §54.1 and Appendix A want it read: base units, human aside.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GiB", 1 << 30),
        ("MiB", 1 << 20),
        ("KiB", 1 << 10),
        ("B", 1),
    ];
    for (name, scale) in UNITS {
        if bytes >= scale {
            let whole = bytes / scale;
            let tenths = (bytes % scale) * 10 / scale;
            return if tenths == 0 {
                format!("{whole} {name}")
            } else {
                format!("{whole}.{tenths} {name}")
            };
        }
    }
    "0 B".to_owned()
}
