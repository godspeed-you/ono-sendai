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

/// The inclusive range a numeric setting's value must lie in (v0.4.1 §55.2).
///
/// Both bounds are in the setting's base units — bytes for a `ByteSize`, milliseconds for a
/// duration — as Appendix A requires: "Limits MUST be expressed internally in integer base units
/// and rendered in human-readable units separately."
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    /// The smallest permitted value.
    pub min: i128,
    /// The largest permitted value.
    pub max: i128,
}

impl Range {
    /// Whether `magnitude` lies inside the range.
    #[must_use]
    pub const fn admits(self, magnitude: i128) -> bool {
        magnitude >= self.min && magnitude <= self.max
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
    /// The permitted range for a numeric setting, where one is declared (v0.4.1 §55.2).
    ///
    /// `None` means the setting is not range-checked, which is the state every key predating
    /// v0.4.1 §55 is in. A `limits.*` key always declares one: §55.2 makes the check mandatory
    /// there, and its binding sentence is that a security-sensitive limit must not silently
    /// become unlimited because a value failed to parse.
    pub range: Option<Range>,
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
        range: None,
    },
    SettingSpec {
        key: "prompt.vcs",
        ty: SettingType::Bool,
        description: "Whether the prompt carries the source-control segment of spec §4.2 — `git:<branch>`, read from the checkout's own files (ADR-0250).",
        default: DefaultValue::Bool(true),
        range: None,
    },
    SettingSpec {
        key: "theme.name",
        ty: SettingType::String,
        description: "Which theme paints the semantic tokens of spec §44: a theme this build ships (`ono`, `neon`), or the name of a `themes/<name>.toml` beside the configuration (spec §30, ADR-0332).",
        default: DefaultValue::Str("ono"),
        range: None,
    },
    SettingSpec {
        key: "render.table.max_rows",
        ty: SettingType::Int,
        description: "How many rows a rendered table shows before a visible `... N more` line (spec §13.3); 0 shows every row.",
        default: DefaultValue::Int(1000),
        range: None,
    },
    SettingSpec {
        key: "history.result_cache",
        ty: SettingType::ByteSize,
        description: "How much memory retained results may occupy (spec §20.2, §30). Superseded by `limits.history_bytes_total`, which v0.4.1 §55.1 names and the shell reads; kept declared so an existing configuration file still parses (v0.4.1 §4.5).",
        default: DefaultValue::Bytes(64 * 1024 * 1024),
        range: None,
    },
    SettingSpec {
        key: "safety.confirm.remote_destructive",
        ty: SettingType::Bool,
        description: "Whether a destructive command inside a link frame asks first (spec §17.4, §30). Recorded; confirmation is not interactive yet.",
        default: DefaultValue::Bool(true),
        range: None,
    },
    SettingSpec {
        key: "safety.confirm.bulk_threshold",
        ty: SettingType::Int,
        description: "How many targets a mutation may touch before it asks first (spec §17.4, §30). Recorded; confirmation is not interactive yet.",
        default: DefaultValue::Int(100),
        range: None,
    },
    // --- the spatial interface, spec v0.4 §47 ------------------------------------------------
    // §47 calls these eleven required and spells out each default. "Disabling `spatial.enabled`
    // MUST leave the typed shell and ordinary commands functional."
    SettingSpec {
        key: "spatial.enabled",
        ty: SettingType::Bool,
        description: "Whether the spatial interface is active (spec v0.4 §47). Disabling it leaves the typed shell and every ordinary command working.",
        default: DefaultValue::Bool(true),
        range: None,
    },
    SettingSpec {
        key: "spatial.startup_horizon",
        ty: SettingType::Bool,
        description: "Whether an interactive session opens with the compact spatial horizon (spec v0.4 §5, §53).",
        default: DefaultValue::Bool(true),
        range: None,
    },
    SettingSpec {
        key: "spatial.follow_cwd",
        ty: SettingType::String,
        description: "How `cd` and the current place stay related (spec v0.4 §30.3). `storage-only`: a directory change moves the storage place and nothing else.",
        default: DefaultValue::Str("storage-only"),
        range: None,
    },
    SettingSpec {
        key: "spatial.map.mode",
        ty: SettingType::String,
        description: "How `map` renders: `auto`, `text` or `fullscreen` (spec v0.4 §23, §47).",
        default: DefaultValue::Str("auto"),
        range: None,
    },
    SettingSpec {
        key: "spatial.map.live",
        ty: SettingType::Bool,
        description: "Whether a map subscribes to change by default (spec v0.4 §25.1). Motion always means real change, never decoration.",
        default: DefaultValue::Bool(false),
        range: None,
    },
    // §23.3's last line: "Key bindings MUST be configurable. Semantic actions are normative;
    // exact single-key choices MAY be remapped." §47 lists no key for it, so this is the one the
    // shell declares — a list of `<action>=<key…>` overrides on top of §23.3's own table.
    SettingSpec {
        key: "spatial.map.keys",
        ty: SettingType::String,
        description: "Key bindings for the full-screen map, as `<action>=<key>` entries separated by commas — `close=q, enter=Enter` (spec v0.4 §23.3). Empty leaves §23.3's table in force.",
        default: DefaultValue::Str(""),
        range: None,
    },
    SettingSpec {
        key: "spatial.map.node_budget",
        ty: SettingType::Int,
        description: "How many nodes a map may draw before it clusters, and above which it refuses with `spatial.map_too_large` (spec v0.4 §8.2, §34.2).",
        default: DefaultValue::Int(100),
        range: None,
    },
    SettingSpec {
        key: "spatial.look.change_window",
        ty: SettingType::String,
        description: "How far back `look`'s change section and the change landmarks reach (spec v0.4 §24.3, §26).",
        default: DefaultValue::Str("5m"),
        range: None,
    },
    SettingSpec {
        key: "spatial.tombstone.lifetime",
        ty: SettingType::String,
        description: "How long a place that went away stays reachable as a tombstone (spec v0.4 §10.3). \"Short-lived\" is the contract: long enough that `back` onto a process that has just exited arrives, short enough that no place returns from the dead mid-investigation.",
        default: DefaultValue::Str("1m"),
        range: None,
    },
    SettingSpec {
        key: "spatial.live.interval",
        ty: SettingType::String,
        description: "How often a live view re-reads a source that does not announce its own changes (spec v0.4 §25.1, §25.3). Polling is explicit, never invisible.",
        default: DefaultValue::Str("500ms"),
        range: None,
    },
    SettingSpec {
        key: "spatial.landmarks.enabled",
        ty: SettingType::Bool,
        description: "Whether the landmark engine runs at all (spec v0.4 §26).",
        default: DefaultValue::Bool(true),
        range: None,
    },
    SettingSpec {
        key: "spatial.reduced_motion",
        ty: SettingType::Bool,
        description: "Suppresses animation in live views (spec v0.4 §25.2, §39.4). What is shown is unchanged; only the movement between frames is.",
        default: DefaultValue::Bool(false),
        range: None,
    },
    SettingSpec {
        key: "spatial.remote_search",
        ty: SettingType::String,
        description: "Whether discovery reaches across links (spec v0.4 §9.3, §35.4). `explicit`: never until asked, and `jump` opens no connection because a name resembles a known place.",
        default: DefaultValue::Str("explicit"),
        range: None,
    },
    SettingSpec {
        key: "spatial.trail.persist",
        ty: SettingType::Bool,
        description: "Whether the navigation trail survives a restart (spec v0.4 §46.1). Off by default for privacy and stale identity; pins persist regardless.",
        default: DefaultValue::Bool(false),
        range: None,
    },
    // §26.3: "Thresholds MUST be inspectable and configurable." `docs/contracts/spatial/landmarks.yaml`
    // names the setting behind each threshold; these are those settings, and `spec-check` holds
    // the two defaults together (ADR-0128).
    SettingSpec {
        key: "spatial.landmarks.high_cpu",
        ty: SettingType::Int,
        description: "The CPU percentage at or above which an object is a `high_cpu` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(80),
        range: None,
    },
    SettingSpec {
        key: "spatial.landmarks.high_memory",
        ty: SettingType::Int,
        description: "The share of the host or cgroup memory budget, in percent, that makes a `high_memory` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(25),
        range: None,
    },
    SettingSpec {
        key: "spatial.landmarks.restart_loop",
        ty: SettingType::Int,
        description: "Restarts within the change window that make a `restarting` landmark rather than a `recently_changed` one (spec v0.4 §26.3).",
        default: DefaultValue::Int(3),
        range: None,
    },
    SettingSpec {
        key: "spatial.landmarks.connection_spike",
        ty: SettingType::Int,
        description: "New connections within the change window that make a `connection_spike` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(100),
        range: None,
    },
    SettingSpec {
        key: "spatial.landmarks.storage_pressure",
        ty: SettingType::Int,
        description: "The used share of a filesystem, in percent, that makes a `storage_pressure` landmark (spec v0.4 §26.3).",
        default: DefaultValue::Int(90),
        range: None,
    },
    // --- the hardening limits of v0.4.1 §55.1 and Appendix A (ADR-0456) ----------------------
    // Every one of these is declared with a range, because §55.2 makes the check mandatory here
    // and its binding sentence is that a security-sensitive agent limit must never silently
    // become unlimited because a value failed to parse. `docs/contracts/hardening/limits.yaml` holds
    // the same thirteen rows, and `resource_limits.rs` compares the two in both directions.
    SettingSpec {
        key: "limits.materialize_items",
        ty: SettingType::Int,
        description: "How many values one global operation may hold at once (v0.4.1 §22.2, Appendix A). Zero permits none; it is not unlimited.",
        default: DefaultValue::Int(100_000),
        range: Some(Range {
            min: 0,
            max: 1000000000,
        }),
    },
    SettingSpec {
        key: "limits.materialize_bytes",
        ty: SettingType::ByteSize,
        description: "How many bytes one global operation may hold at once, counted with the estimator of v0.4.1 §21.2 (§22.2, Appendix A). Both ceilings apply; the first reached wins.",
        default: DefaultValue::Bytes(134_217_728),
        range: Some(Range {
            min: 0,
            max: 1099511627776,
        }),
    },
    SettingSpec {
        key: "limits.command_capture_bytes",
        ty: SettingType::ByteSize,
        description: "How many bytes every capture inside one shell command may retain together (v0.4.1 §23.4, Appendix A) — a ceiling across nested captures, not an allowance each may spend in full.",
        default: DefaultValue::Bytes(268_435_456),
        range: Some(Range {
            min: 0,
            max: 1099511627776,
        }),
    },
    SettingSpec {
        key: "limits.history_results",
        ty: SettingType::Int,
        description: "How many recent pipeline results `@-1` … `@-N` can reach (v0.4.1 §24.1, Appendix A). Supersedes the count of spec §20.2.",
        default: DefaultValue::Int(16),
        range: Some(Range { min: 0, max: 4096 }),
    },
    SettingSpec {
        key: "limits.history_items_per_result",
        ty: SettingType::Int,
        description: "How many values of one result are retained for reuse (v0.4.1 §24.1, Appendix A). Retention only: the emitted output is never truncated to fit.",
        default: DefaultValue::Int(10_000),
        range: Some(Range {
            min: 0,
            max: 100000000,
        }),
    },
    SettingSpec {
        key: "limits.history_bytes_per_result",
        ty: SettingType::ByteSize,
        description: "How many bytes of one result are retained (v0.4.1 §24.1, Appendix A). A single value larger than this is not retained, and still flows through the pipeline.",
        default: DefaultValue::Bytes(16_777_216),
        range: Some(Range {
            min: 0,
            max: 68719476736,
        }),
    },
    SettingSpec {
        key: "limits.history_bytes_total",
        ty: SettingType::ByteSize,
        description: "How many bytes the whole result history may hold before oldest-first eviction (v0.4.1 §24.1, §24.2, Appendix A). Supersedes `history.result_cache`, which nothing reads.",
        default: DefaultValue::Bytes(67_108_864),
        range: Some(Range {
            min: 0,
            max: 274877906944,
        }),
    },
    SettingSpec {
        key: "limits.completion_soft_ms",
        ty: SettingType::Int,
        description: "When completion stops waiting for a provider and answers with what it has, in milliseconds (v0.4.1 §36.2, Appendix A).",
        default: DefaultValue::Int(50),
        range: Some(Range { min: 1, max: 60000 }),
    },
    SettingSpec {
        key: "limits.completion_hard_ms",
        ty: SettingType::Int,
        description: "When completion stops asking further providers, whatever it has found, in milliseconds (v0.4.1 §36.2, Appendix A).",
        default: DefaultValue::Int(150),
        range: Some(Range { min: 1, max: 60000 }),
    },
    SettingSpec {
        key: "limits.orientation_objects",
        ty: SettingType::Int,
        description: "How many objects one orientation reads from a provider target before it counts the rest instead (v0.4.1 §34.4, Appendix A).",
        default: DefaultValue::Int(128),
        range: Some(Range {
            min: 1,
            max: 1000000,
        }),
    },
    SettingSpec {
        key: "limits.orientation_ceiling",
        ty: SettingType::Int,
        description: "How many objects one orientation reads from a provider target whose count it cannot keep true, before it stops and says so (v0.4.1 §34.4, Appendix A).",
        default: DefaultValue::Int(16384),
        range: Some(Range {
            min: 1,
            max: 100000000,
        }),
    },
    SettingSpec {
        key: "limits.remote_connections",
        ty: SettingType::Int,
        description: "Concurrent authenticated connections one listening agent accepts (v0.4.1 §12.1, Appendix A). Declared and validated; enforcement is phase H3's.",
        default: DefaultValue::Int(32),
        range: Some(Range { min: 1, max: 65536 }),
    },
    SettingSpec {
        key: "limits.remote_pending_handshakes",
        ty: SettingType::Int,
        description: "Connections that may be mid-negotiation at once (v0.4.1 §12.2, Appendix A). Declared and validated; enforcement is phase H3's.",
        default: DefaultValue::Int(16),
        range: Some(Range { min: 1, max: 65536 }),
    },
    SettingSpec {
        key: "limits.remote_connections_per_client",
        ty: SettingType::Int,
        description: "Concurrent connections one authenticated fingerprint may hold (v0.4.1 §12.3, Appendix A). Declared and validated; enforcement is phase H3's.",
        default: DefaultValue::Int(4),
        range: Some(Range { min: 1, max: 65536 }),
    },
    SettingSpec {
        key: "limits.remote_handshake_timeout_ms",
        ty: SettingType::Int,
        description: "How long TLS and protocol negotiation may take, in milliseconds (v0.4.1 §12.2, Appendix A). Declared and validated; enforcement is phase H3's.",
        default: DefaultValue::Int(10_000),
        range: Some(Range {
            min: 100,
            max: 600000,
        }),
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
        // v0.4.1 §55.2: numeric limits are range-checked here rather than at each read site, so
        // one check covers the file, the environment and `set config` at the prompt. Nothing is
        // stored when it fails, which is what keeps the earlier layer in force.
        if let Some(range) = setting.range
            && !magnitude_of(&value).is_some_and(|number| range.admits(number))
        {
            return Err(out_of_range(setting, range, &value));
        }
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

/// The number a numeric setting's value carries, in its base unit.
fn magnitude_of(value: &Value) -> Option<i128> {
    match value {
        Value::Int(number) => Some(*number),
        Value::ByteSize(size) => i128::try_from(size.bytes()).ok(),
        _ => None,
    }
}

/// The refusal a limit outside its declared range carries (v0.4.1 §55.2).
///
/// It names the key, what was given and the range it is outside, because a diagnostic that says
/// only "invalid" leaves the user to guess which end they were on. The earlier layer's value
/// stays in force, which is the existing config-layer rule (ADR-0010) and is what keeps §55.2's
/// binding sentence true: a limit that failed to parse never becomes unlimited, it stays what it
/// was.
fn out_of_range(setting: &SettingSpec, range: Range, value: &Value) -> ErrorValue {
    let Range { min, max } = range;
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!(
            "`{}` accepts {min} to {max} {}, and {value} is outside that",
            setting.key,
            match setting.ty {
                SettingType::ByteSize => "bytes",
                _ => "inclusive",
            }
        ),
    )
    .with_help(format!(
        "`{}` keeps its current value; v0.4.1 §55.2 range-checks every limit, and a limit that \
         fails the check never becomes unlimited",
        setting.key
    ))
    .with_metadata("setting", Value::string(setting.key))
    .with_metadata("min", Value::Int(min))
    .with_metadata("max", Value::Int(max))
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
