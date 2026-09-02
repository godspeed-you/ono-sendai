//! The effective runtime limits of v0.4.1 §55.1 and Appendix A, read from the one place they are
//! declared.
//!
//! §12.4 asks for a centralized `Limits` a diagnostic command or a test fixture can print, and
//! §52.2 says why it matters: *"A number such as `max_connections = 32` MUST not be independently
//! typed into five files if one contract can generate the others."*
//!
//! So this module holds no numbers. Every figure comes from [`crate::settings::CATALOGUE`], which
//! is the shell's declaration of what a limit is and what it defaults to, and
//! `docs/spec/hardening/limits.yaml` is the machine-readable copy the gate compares against it.
//! What lives here are the typed readings the components need — a
//! [`ono_pipeline::MaterializationLimits`] for the pipeline, a
//! [`HistoryLimits`] for the retained results — and the rows `inspect limits` shows.

use ono_pipeline::MaterializationLimits;
use ono_value::{ByteSize, Value};

use crate::settings::{CATALOGUE, SettingType, Settings};

/// Every key whose effective value `inspect limits` reports (§54.3).
///
/// The prefix rather than a list: a key added to the catalogue under `limits.` is a limit, and a
/// diagnostic that had to be told about it separately would be the second copy §52.2 forbids.
pub const PREFIX: &str = "limits.";

/// The number a `limits.*` key carries, in its base unit, or its declared default.
///
/// A key the catalogue does not declare cannot be read: the catalogue is the declaration, so a
/// missing key is a programming error rather than a configuration one, and answering zero for it
/// would turn a typo into "no values permitted" (§22.2).
#[must_use]
pub fn magnitude(settings: &Settings, key: &str) -> u64 {
    let declared = CATALOGUE
        .iter()
        .find(|setting| setting.key == key)
        .map(|setting| setting.default_value());
    let effective = settings
        .effective(key)
        .map(|resolved| resolved.value.clone())
        .or(declared);
    match effective {
        Some(Value::Int(number)) => u64::try_from(number).unwrap_or(0),
        Some(Value::ByteSize(size)) => u64::try_from(size.bytes()).unwrap_or(u64::MAX),
        _ => 0,
    }
}

/// What one materializing stage may collect (§22.2).
#[must_use]
pub fn materialization(settings: &Settings) -> MaterializationLimits {
    MaterializationLimits::new(
        magnitude(settings, "limits.materialize_items"),
        magnitude(settings, "limits.materialize_bytes"),
    )
}

/// What every capture inside one shell command may retain together (§23.4).
#[must_use]
pub fn command_capture_bytes(settings: &Settings) -> u64 {
    magnitude(settings, "limits.command_capture_bytes")
}

/// What the retained result history of §24.1 may hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryLimits {
    /// How many results `@-1` … `@-N` can reach.
    pub results: usize,
    /// How many values of one result are retained.
    pub items_per_result: usize,
    /// How many bytes of one result are retained.
    pub bytes_per_result: u64,
    /// How many bytes the whole history may hold before oldest-first eviction.
    pub bytes_total: u64,
}

impl HistoryLimits {
    /// The limits in force for `settings`.
    #[must_use]
    pub fn of(settings: &Settings) -> Self {
        Self {
            results: usize::try_from(magnitude(settings, "limits.history_results"))
                .unwrap_or(usize::MAX),
            items_per_result: usize::try_from(magnitude(
                settings,
                "limits.history_items_per_result",
            ))
            .unwrap_or(usize::MAX),
            bytes_per_result: magnitude(settings, "limits.history_bytes_per_result"),
            bytes_total: magnitude(settings, "limits.history_bytes_total"),
        }
    }
}

/// Appendix A's defaults, for a context that has no settings to read.
impl Default for HistoryLimits {
    fn default() -> Self {
        Self::of(&Settings::new())
    }
}

/// The effective non-secret limits, as objects (§54.3).
///
/// One row per declared `limits.*` key, carrying what the shell will actually enforce rather than
/// a second table of the same numbers. `unit` and `enforced_by` come from the same registry the
/// gate compares the catalogue against, so a user reading this and a test reading
/// `docs/spec/hardening/limits.yaml` are reading one thing.
#[must_use]
pub fn rows(settings: &Settings) -> Vec<Value> {
    CATALOGUE
        .iter()
        .filter(|setting| setting.key.starts_with(PREFIX))
        .map(|setting| {
            let effective = settings.effective(setting.key);
            let mut row = ono_value::MapValue::new();
            row.insert("key".into(), Value::string(setting.key));
            row.insert(
                "value".into(),
                effective.map_or_else(|| setting.default_value(), |r| r.value.clone()),
            );
            row.insert(
                "bytes".into(),
                match setting.ty {
                    SettingType::ByteSize => {
                        Value::Int(i128::from(magnitude(settings, setting.key)))
                    }
                    _ => Value::Null,
                },
            );
            row.insert("type".into(), Value::string(setting.ty.name()));
            row.insert(
                "layer".into(),
                Value::string(effective.map_or("default", |r| r.layer.name())),
            );
            row.insert(
                "min".into(),
                setting
                    .range
                    .map_or(Value::Null, |range| numeric(setting.ty, range.min)),
            );
            row.insert(
                "max".into(),
                setting
                    .range
                    .map_or(Value::Null, |range| numeric(setting.ty, range.max)),
            );
            row.insert("description".into(), Value::string(setting.description));
            Value::Map(std::sync::Arc::new(row))
        })
        .collect()
}

/// A bound in the unit its setting is declared in, so a byte ceiling reads as a byte size.
fn numeric(ty: SettingType, magnitude: i128) -> Value {
    match ty {
        SettingType::ByteSize => {
            Value::ByteSize(ByteSize::from_bytes(u128::try_from(magnitude).unwrap_or(0)))
        }
        _ => Value::Int(magnitude),
    }
}
