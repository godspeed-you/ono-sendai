//! The shell's configuration: a declared, typed catalogue of settings and the layered values
//! ADR-0010 fixes, each carrying the provenance spec §30 asks for.
//!
//! Configuration is a record of declared settings, not a bag of strings. Every key has a type
//! and a built-in default; every layer that sets it — the system file, the user file, an `ONO_*`
//! variable, a `set config` in the running shell — pushes a typed value on top of the earlier
//! ones, and `get config` reports the effective value with the layer, file and line that set it.
//! A key nothing declares is `type.unknown_field`; a value of the wrong type is `type.mismatch`
//! and leaves the earlier layer's value in force (ADR-0094).

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use ono_core::ErrorCode;
use ono_value::{ByteSize, ErrorValue, Provenance, RecordValue, SchemaId, Value};

/// Which of ADR-0010's five layers a value came from, in override order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// The built-in default.
    Default,
    /// `/etc/ono/config.ono`.
    System,
    /// The user's `config.ono`, or the one file `ONO_CONFIG` / `--config` names.
    User,
    /// An `ONO_*` environment variable.
    Environment,
    /// `set config` in the running shell, or an option of the invocation.
    Invocation,
}

impl Layer {
    /// The name `config-setting.v1` uses.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Layer::Default => "default",
            Layer::System => "system",
            Layer::User => "user",
            Layer::Environment => "environment",
            Layer::Invocation => "invocation",
        }
    }
}

/// The type a setting is declared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingType {
    /// A whole number.
    Int,
    /// `true` or `false`.
    Bool,
    /// Text.
    String,
    /// A byte size such as `64MiB`.
    ByteSize,
}

impl SettingType {
    /// The name `config-setting.v1` reports in `type`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            SettingType::Int => "int",
            SettingType::Bool => "bool",
            SettingType::String => "string",
            SettingType::ByteSize => "bytesize",
        }
    }

    /// Whether a value already has this type.
    fn accepts(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (SettingType::Int, Value::Int(_))
                | (SettingType::Bool, Value::Bool(_))
                | (SettingType::String, Value::String(_))
                | (SettingType::ByteSize, Value::ByteSize(_) | Value::Int(_))
        )
    }

    /// Reads a bare word as this type, the way a typed parameter reads one (ADR-0070).
    fn read_word(self, word: &str) -> Option<Value> {
        let word = word.trim();
        match self {
            SettingType::Int => word.parse().ok().map(Value::Int),
            SettingType::Bool => match word {
                "true" => Some(Value::Bool(true)),
                "false" => Some(Value::Bool(false)),
                _ => None,
            },
            SettingType::String => Some(Value::string(word)),
            SettingType::ByteSize => ByteSize::parse(word)
                .ok()
                .map(Value::ByteSize)
                .or_else(|| word.parse().ok().map(Value::Int)),
        }
    }
}

/// The built-in default of a setting, spelled so the catalogue can be a constant.
#[derive(Debug, Clone, Copy)]
enum DefaultValue {
    Int(i128),
    Bool(bool),
    Str(&'static str),
    Bytes(u128),
}

impl DefaultValue {
    fn value(self) -> Value {
        match self {
            DefaultValue::Int(number) => Value::Int(number),
            DefaultValue::Bool(flag) => Value::Bool(flag),
            DefaultValue::Str(text) => Value::string(text),
            DefaultValue::Bytes(bytes) => Value::ByteSize(ByteSize::from_bytes(bytes)),
        }
    }
}

/// One declared setting.
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    /// The dotted key, e.g. `render.table.max_rows`.
    pub key: &'static str,
    /// The declared type; an assignment of another type is refused.
    pub ty: SettingType,
    /// One line for `help` and completion.
    pub description: &'static str,
    default: DefaultValue,
}

impl SettingSpec {
    /// The built-in default.
    #[must_use]
    pub fn default_value(&self) -> Value {
        self.default.value()
    }

    /// The `ONO_*` variable that sets this key (ADR-0010): mechanical, so nothing needs a table.
    #[must_use]
    pub fn environment_variable(&self) -> String {
        format!("ONO_{}", self.key.to_ascii_uppercase().replace('.', "_"))
    }
}

/// Every setting this build declares (spec §30, ADR-0094).
///
/// A key is declared here or it does not exist: `set config` refuses anything else. Which ones a
/// component actually reads is stated per entry, so the catalogue never promises an effect
/// nothing has.
pub const CATALOGUE: &[SettingSpec] = &[
    SettingSpec {
        key: "prompt.path",
        ty: SettingType::String,
        description: "How the prompt shows the working directory (spec §30). Recorded; no prompt style reads it yet.",
        default: DefaultValue::Str("smart"),
    },
    SettingSpec {
        key: "prompt.vcs",
        ty: SettingType::Bool,
        description: "Whether the prompt carries the source-control segment of spec §4.2 — `git:<branch>`, read from the checkout's own files (ADR-0250).",
        default: DefaultValue::Bool(true),
    },
    SettingSpec {
        key: "render.table.max_rows",
        ty: SettingType::Int,
        description: "How many rows a rendered table shows before a visible `... N more` line (spec §13.3); 0 shows every row.",
        default: DefaultValue::Int(1000),
    },
    SettingSpec {
        key: "history.result_cache",
        ty: SettingType::ByteSize,
        description: "How much memory retained results may occupy (spec §20.2, §30). Recorded; retention is counted in results today.",
        default: DefaultValue::Bytes(64 * 1024 * 1024),
    },
    SettingSpec {
        key: "safety.confirm.remote_destructive",
        ty: SettingType::Bool,
        description: "Whether a destructive command inside a link frame asks first (spec §17.4, §30). Recorded; confirmation is not interactive yet.",
        default: DefaultValue::Bool(true),
    },
    SettingSpec {
        key: "safety.confirm.bulk_threshold",
        ty: SettingType::Int,
        description: "How many targets a mutation may touch before it asks first (spec §17.4, §30). Recorded; confirmation is not interactive yet.",
        default: DefaultValue::Int(100),
    },
    // --- the spatial interface, spec v0.4 §47 ------------------------------------------------
    // §47 calls these eleven required and spells out each default. "Disabling `spatial.enabled`
    // MUST leave the typed shell and ordinary commands functional."
    SettingSpec {
        key: "spatial.enabled",
        ty: SettingType::Bool,
        description: "Whether the spatial interface is active (spec v0.4 §47). Disabling it leaves the typed shell and every ordinary command working.",
        default: DefaultValue::Bool(true),
    },
    SettingSpec {
        key: "spatial.startup_horizon",
        ty: SettingType::Bool,
        description: "Whether an interactive session opens with the compact spatial horizon (spec v0.4 §5, §53).",
        default: DefaultValue::Bool(true),
    },
    SettingSpec {
        key: "spatial.follow_cwd",
        ty: SettingType::String,
        description: "How `cd` and the current place stay related (spec v0.4 §30.3). `storage-only`: a directory change moves the storage place and nothing else.",
        default: DefaultValue::Str("storage-only"),
    },
    SettingSpec {
        key: "spatial.map.mode",
        ty: SettingType::String,
        description: "How `map` renders: `auto`, `text` or `fullscreen` (spec v0.4 §23, §47).",
        default: DefaultValue::Str("auto"),
    },
    SettingSpec {
        key: "spatial.map.live",
        ty: SettingType::Bool,
        description: "Whether a map subscribes to change by default (spec v0.4 §25.1). Motion always means real change, never decoration.",
        default: DefaultValue::Bool(false),
    },
    // §23.3's last line: "Key bindings MUST be configurable. Semantic actions are normative;
    // exact single-key choices MAY be remapped." §47 lists no key for it, so this is the one the
    // shell declares — a list of `<action>=<key…>` overrides on top of §23.3's own table.
    SettingSpec {
        key: "spatial.map.keys",
        ty: SettingType::String,
        description: "Key bindings for the full-screen map, as `<action>=<key>` entries separated by commas — `close=q, enter=Enter` (spec v0.4 §23.3). Empty leaves §23.3's table in force.",
        default: DefaultValue::Str(""),
    },
    SettingSpec {
        key: "spatial.map.node_budget",
        ty: SettingType::Int,
        description: "How many nodes a map may draw before it clusters, and above which it refuses with `spatial.map_too_large` (spec v0.4 §8.2, §34.2).",
        default: DefaultValue::Int(100),
    },
    SettingSpec {
        key: "spatial.look.change_window",
        ty: SettingType::String,
        description: "How far back `look`'s change section and the change landmarks reach (spec v0.4 §24.3, §26).",
        default: DefaultValue::Str("5m"),
    },
    SettingSpec {
        key: "spatial.tombstone.lifetime",
        ty: SettingType::String,
        description: "How long a place that went away stays reachable as a tombstone (spec v0.4 §10.3). \"Short-lived\" is the contract: long enough that `back` onto a process that has just exited arrives, short enough that no place returns from the dead mid-investigation.",
        default: DefaultValue::Str("1m"),
    },
    SettingSpec {
        key: "spatial.live.interval",
        ty: SettingType::String,
        description: "How often a live view re-reads a source that does not announce its own changes (spec v0.4 §25.1, §25.3). Polling is explicit, never invisible.",
        default: DefaultValue::Str("500ms"),
    },
    SettingSpec {
        key: "spatial.landmarks.enabled",
        ty: SettingType::Bool,
        description: "Whether the landmark engine runs at all (spec v0.4 §26).",
        default: DefaultValue::Bool(true),
    },
    SettingSpec {
        key: "spatial.reduced_motion",
        ty: SettingType::Bool,
        description: "Suppresses animation in live views (spec v0.4 §25.2, §39.4). What is shown is unchanged; only the movement between frames is.",
        default: DefaultValue::Bool(false),
    },
    SettingSpec {
        key: "spatial.remote_search",
        ty: SettingType::String,
        description: "Whether discovery reaches across links (spec v0.4 §9.3, §35.4). `explicit`: never until asked, and `jump` opens no connection because a name resembles a known place.",
        default: DefaultValue::Str("explicit"),
    },
    SettingSpec {
        key: "spatial.trail.persist",
        ty: SettingType::Bool,
        description: "Whether the navigation trail survives a restart (spec v0.4 §46.1). Off by default for privacy and stale identity; pins persist regardless.",
        default: DefaultValue::Bool(false),
    },
    // §26.3: "Thresholds MUST be inspectable and configurable." `docs/spec/spatial/landmarks.yaml`
    // names the setting behind each threshold; these are those settings, and `spec-check` holds
    // the two defaults together (ADR-0128).
    SettingSpec {
        key: "spatial.landmarks.high_cpu",
        ty: SettingType::Int,
        description: "The CPU percentage at or above which an object is a `high_cpu` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(80),
    },
    SettingSpec {
        key: "spatial.landmarks.high_memory",
        ty: SettingType::Int,
        description: "The share of the host or cgroup memory budget, in percent, that makes a `high_memory` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(25),
    },
    SettingSpec {
        key: "spatial.landmarks.restart_loop",
        ty: SettingType::Int,
        description: "Restarts within the change window that make a `restarting` landmark rather than a `recently_changed` one (spec v0.4 §26.3).",
        default: DefaultValue::Int(3),
    },
    SettingSpec {
        key: "spatial.landmarks.connection_spike",
        ty: SettingType::Int,
        description: "New connections within the change window that make a `connection_spike` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(100),
    },
    SettingSpec {
        key: "spatial.landmarks.storage_pressure",
        ty: SettingType::Int,
        description: "The used share of a filesystem, in percent, that makes a `storage_pressure` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(90),
    },
];

/// The declaration of `key`, if there is one.
#[must_use]
pub fn spec(key: &str) -> Option<&'static SettingSpec> {
    CATALOGUE.iter().find(|setting| setting.key == key)
}

/// One value a layer gave a setting, with where it came from.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// The typed value.
    pub value: Value,
    /// The layer that set it.
    pub layer: Layer,
    /// The file, for the system and user layers.
    pub source: Option<PathBuf>,
    /// The line within `source`.
    pub line: Option<u32>,
}

/// Where the configuration being read comes from, while a file is being evaluated.
#[derive(Debug, Clone)]
pub struct Reading {
    /// The layer the file belongs to.
    pub layer: Layer,
    /// The file.
    pub path: PathBuf,
}

/// The layered settings of one session.
#[derive(Debug)]
pub struct Settings {
    /// Per key, every layer's value in override order; the last is the effective one.
    layers: BTreeMap<&'static str, Vec<Resolved>>,
    /// What went wrong while loading, as values (`get config --problems`).
    problems: Vec<Value>,
    /// The file being evaluated right now, so `set config` in it knows its layer and line.
    reading: Option<Reading>,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    /// Every setting at its built-in default.
    #[must_use]
    pub fn new() -> Self {
        let layers = CATALOGUE
            .iter()
            .map(|setting| {
                (
                    setting.key,
                    vec![Resolved {
                        value: setting.default_value(),
                        layer: Layer::Default,
                        source: None,
                        line: None,
                    }],
                )
            })
            .collect();
        Self {
            layers,
            problems: Vec::new(),
            reading: None,
        }
    }

    /// Marks the start of reading a configuration file.
    pub fn begin_file(&mut self, layer: Layer, path: PathBuf) {
        self.reading = Some(Reading { layer, path });
    }

    /// Marks the end of reading a configuration file.
    pub fn end_file(&mut self) {
        self.reading = None;
    }

    /// The file being read right now, if any.
    #[must_use]
    pub fn reading(&self) -> Option<&Reading> {
        self.reading.as_ref()
    }

    /// Records a load-time problem, so it stays available after it was printed (ADR-0010).
    pub fn note_problem(&mut self, error: &ErrorValue) {
        self.problems.push(error.clone().into_value());
    }

    /// The load-time problems, as values.
    #[must_use]
    pub fn problems(&self) -> &[Value] {
        &self.problems
    }

    /// The effective value of `key`.
    #[must_use]
    pub fn effective(&self, key: &str) -> Option<&Resolved> {
        self.layers.get(key).and_then(|stack| stack.last())
    }

    /// Every setting's key with its effective value, in catalogue order.
    pub fn effective_values(&self) -> impl Iterator<Item = (&'static str, &Value)> {
        self.layers
            .iter()
            .filter_map(|(key, stack)| stack.last().map(|resolved| (*key, &resolved.value)))
    }

    /// The effective value of an integer setting.
    #[must_use]
    pub fn int(&self, key: &str) -> Option<i128> {
        match self.effective(key).map(|resolved| &resolved.value) {
            Some(Value::Int(number)) => Some(*number),
            _ => None,
        }
    }

    /// The effective value of a boolean setting.
    #[must_use]
    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.effective(key).map(|resolved| &resolved.value) {
            Some(Value::Bool(state)) => Some(*state),
            _ => None,
        }
    }

    /// The effective value of a string setting.
    #[must_use]
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.effective(key).map(|resolved| &resolved.value) {
            Some(Value::String(text)) => Some(text.as_ref()),
            _ => None,
        }
    }

    /// Sets `key` at `layer`, from a bare word or a value.
    ///
    /// Returns whether the effective value changed.
    ///
    /// # Errors
    ///
    /// `type.unknown_field` for a key the catalogue does not declare; `type.mismatch` for a value
    /// the declared type does not admit. Neither changes anything.
    pub fn assign(
        &mut self,
        key: &str,
        given: Given,
        layer: Layer,
        source: Option<PathBuf>,
        line: Option<u32>,
    ) -> Result<bool, ErrorValue> {
        let setting = spec(key).ok_or_else(|| {
            let error = ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("there is no setting `{key}`"),
            );
            match closest(key) {
                Some(near) => error.with_help(format!(
                    "did you mean `{near}`? `get config` lists every setting"
                )),
                None => error.with_help("`get config` lists every setting"),
            }
        })?;
        let value = match given {
            Given::Word(word) => setting
                .ty
                .read_word(&word)
                .ok_or_else(|| mismatch(setting, &format!("`{word}`")))?,
            Given::Value(value) => {
                if !setting.ty.accepts(&value) {
                    return Err(mismatch(setting, &format!("a {}", value.type_name())));
                }
                match (setting.ty, value) {
                    (SettingType::ByteSize, Value::Int(bytes)) => {
                        Value::ByteSize(ByteSize::from_bytes(u128::try_from(bytes).unwrap_or(0)))
                    }
                    (_, value) => value,
                }
            }
        };
        let stack = self.layers.entry(setting.key).or_default();
        let changed = stack.last().is_none_or(|current| current.value != value);
        // A layer sets a key once; a later assignment at the same layer replaces the earlier one
        // rather than stacking a history nobody asked for.
        stack.retain(|resolved| resolved.layer != layer);
        stack.push(Resolved {
            value,
            layer,
            source,
            line,
        });
        stack.sort_by_key(|resolved| resolved.layer);
        Ok(changed)
    }

    /// Reads every `ONO_*` variable that names a declared setting (ADR-0010, layer 4).
    ///
    /// A variable that does not parse as the setting's type is a problem, reported and recorded,
    /// and the earlier layer's value stays in force.
    pub fn apply_environment(
        &mut self,
        variables: &BTreeMap<OsString, OsString>,
        report: &mut dyn FnMut(&ErrorValue),
    ) {
        for setting in CATALOGUE {
            let name = setting.environment_variable();
            let Some(raw) = variables.get(OsStr::new(&name)) else {
                continue;
            };
            let word = raw.to_string_lossy().into_owned();
            if let Err(error) = self.assign(
                setting.key,
                Given::Word(word),
                Layer::Environment,
                None,
                None,
            ) {
                let error = error.with_help(format!("set by the environment variable `{name}`"));
                report(&error);
                self.note_problem(&error);
            }
        }
    }

    /// The `ono.config-setting/1` records `get config` streams.
    ///
    /// `selector` is an exact key or a dotted prefix such as `safety.`; none means every setting.
    /// With `overridden`, every layer's value is a row, effective last.
    ///
    /// # Errors
    ///
    /// `type.unknown_field` when an exact key matches nothing.
    pub fn records(
        &self,
        selector: Option<&str>,
        overridden: bool,
    ) -> Result<Vec<Value>, ErrorValue> {
        let matches = |key: &str| match selector {
            None => true,
            Some(prefix) if prefix.ends_with('.') => key.starts_with(prefix),
            Some(exact) => key == exact,
        };
        let mut rows = Vec::new();
        for setting in CATALOGUE {
            if !matches(setting.key) {
                continue;
            }
            let Some(stack) = self.layers.get(setting.key) else {
                continue;
            };
            let shown: Vec<&Resolved> = if overridden {
                stack.iter().collect()
            } else {
                stack.last().into_iter().collect()
            };
            for resolved in shown {
                rows.push(record(setting, resolved)?);
            }
        }
        if rows.is_empty()
            && let Some(key) = selector
            && !key.ends_with('.')
        {
            let error = ErrorValue::new(
                ErrorCode::TypeUnknownField,
                format!("there is no setting `{key}`"),
            );
            return Err(match closest(key) {
                Some(near) => error.with_help(format!("did you mean `{near}`?")),
                None => error.with_help("`get config` lists every setting"),
            });
        }
        Ok(rows)
    }
}

/// What an assignment was given: a bare word, read as the declared type, or a value, which must
/// already have it (ADR-0070's rule for typed parameters).
#[derive(Debug, Clone)]
pub enum Given {
    /// A bare word.
    Word(String),
    /// An evaluated value.
    Value(Value),
}

fn mismatch(setting: &SettingSpec, what: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!(
            "`{}` is {}, and {what} is not one",
            setting.key,
            with_article(setting.ty)
        ),
    )
    .with_help(format!(
        "`{}` is declared as {}; the setting keeps its current value",
        setting.key,
        setting.ty.name()
    ))
}

fn with_article(ty: SettingType) -> &'static str {
    match ty {
        SettingType::Int => "an int",
        SettingType::Bool => "a bool",
        SettingType::String => "a string",
        SettingType::ByteSize => "a bytesize",
    }
}

/// The declared key nearest to `key`, for a suggestion.
fn closest(key: &str) -> Option<&'static str> {
    CATALOGUE
        .iter()
        .map(|setting| (crate::resolve::edit_distance(key, setting.key), setting.key))
        .filter(|(distance, _)| *distance <= key.len().div_ceil(3).max(1))
        .min()
        .map(|(_, near)| near)
}

fn record(setting: &SettingSpec, resolved: &Resolved) -> Result<Value, ErrorValue> {
    let schema = ono_value::builtin_schemas()
        .get(&SchemaId::new("ono.config-setting", 1))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderSchemaViolation,
                "the `ono.config-setting/1` schema is not built in",
            )
        })?;
    let provenance =
        Provenance::local("ono.config", schema.id().clone()).from_source(resolved.layer.name());
    let built = RecordValue::builder(schema, provenance)
        .set("key", Value::string(setting.key))?
        .set("value", resolved.value.clone())?
        .set("type", Value::string(setting.ty.name()))?
        .set("layer", Value::string(resolved.layer.name()))?
        .set(
            "source",
            resolved
                .source
                .as_ref()
                .map_or(Value::Null, |path| Value::Path(path.as_path().into())),
        )?
        .set(
            "line",
            resolved
                .line
                .map_or(Value::Null, |line| Value::Int(i128::from(line))),
        )?
        .set("default_value", setting.default_value())?
        .set("description", Value::string(setting.description))?
        .build();
    Ok(built.into_value())
}
