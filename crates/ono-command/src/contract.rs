//! The public command contract, as `docs/contracts/commands/` declares it (spec §27, ADR-0012).
//!
//! Nothing here is invented: every field of [`CommandContract`] is a field of a registry entry,
//! and the enums are the closed vocabularies those files use. Loading is what turns the YAML into
//! these types; everything else in this crate reads them.

use std::fmt;
use std::str::FromStr;

use ono_core::ErrorCode;
use ono_value::{ByteSize, Duration, ErrorValue, IpNetwork, Value};
use serde::Deserialize;

/// How much of a compatibility promise a command is (ADR-0012 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stability {
    /// Named by a normative section of the specification. Id, semantics and output schema are a
    /// compatibility promise (spec §40.4).
    Stable,
    /// Named only by the §52 matrix, an example, or a section that declines to freeze it.
    Experimental,
    /// A `?` cell of the §52 matrix: the semantic usefulness must be validated before the command
    /// is implemented for symmetry (spec §52).
    Planned,
}

impl Stability {
    /// The word the registry files use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Stability::Stable => "stable",
            Stability::Experimental => "experimental",
            Stability::Planned => "planned",
        }
    }
}

impl fmt::Display for Stability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a registry entry came from (spec §31.64, ADR-0281).
///
/// Every registry entry records its origin, and the host sets it — a package cannot declare
/// itself core. `docs/contracts/kuang/contributions.v1.yaml` names three values; `remote-provider`
/// arrives with the remote registry projection of spec §31.40 and is not constructed yet, so it
/// is not spelled here as an arm nothing can build.
///
/// ```
/// use ono_command::Origin;
/// assert_eq!(Origin::Core.to_string(), "core");
/// assert_eq!(
///     Origin::plugin("dev.example.echo", "0.1.0").to_string(),
///     "plugin(dev.example.echo, 0.1.0)"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum Origin {
    /// Shipped with Ono. The only origin that may hold an `ono.*` id.
    #[default]
    Core,
    /// Contributed by a KUANG/11 package, which the entry names by id and version.
    Plugin {
        /// The contributing package's id.
        package: String,
        /// The version of the package that contributed the entry.
        version: String,
    },
}

impl Origin {
    /// The origin of an entry a package contributed.
    #[must_use]
    pub fn plugin(package: impl Into<String>, version: impl Into<String>) -> Self {
        Origin::Plugin {
            package: package.into(),
            version: version.into(),
        }
    }

    /// The contributing package's id, or `None` for a core entry.
    #[must_use]
    pub fn package(&self) -> Option<&str> {
        match self {
            Origin::Core => None,
            Origin::Plugin { package, .. } => Some(package),
        }
    }

    /// The contributing package's version, or `None` for a core entry.
    #[must_use]
    pub fn version(&self) -> Option<&str> {
        match self {
            Origin::Core => None,
            Origin::Plugin { version, .. } => Some(version),
        }
    }

    /// Whether the entry ships with Ono.
    #[must_use]
    pub const fn is_core(&self) -> bool {
        matches!(self, Origin::Core)
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Origin::Core => f.write_str("core"),
            Origin::Plugin { package, version } => write!(f, "plugin({package}, {version})"),
        }
    }
}

/// What the user needs before a command runs (ADR-0012 §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Privilege {
    /// Works as an ordinary user.
    None,
    /// Cannot work without elevated privilege.
    Elevated,
    /// Unprivileged for some targets and privileged for others — the common case on Linux, and
    /// the one a shell must not paper over.
    Conditional,
}

impl Privilege {
    /// The word the registry files use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Privilege::None => "none",
            Privilege::Elevated => "elevated",
            Privilege::Conditional => "conditional",
        }
    }
}

impl fmt::Display for Privilege {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a provider capability needs privilege (`docs/contracts/capabilities.yaml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Elevation {
    /// Available to an ordinary user.
    None,
    /// Unprivileged for some targets, privileged for others.
    Conditional,
    /// Cannot work without privilege.
    Required,
}

impl Elevation {
    /// The word the registry files use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Elevation::None => "none",
            Elevation::Conditional => "conditional",
            Elevation::Required => "required",
        }
    }
}

impl fmt::Display for Elevation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a command's arguments are lexed and parsed (ADR-0009).
///
/// The registry restates what the parser's built-in table already decides, so that help and
/// completion describe the language the parser actually implements. A disagreement between the
/// two is a defect, and the crate's tests fail on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArgumentMode {
    /// Bare tokens are words, `<` and `>` redirect.
    Words,
    /// Bare identifiers are field paths, `<` and `>` compare.
    Expression,
}

impl ArgumentMode {
    /// The word the registry files use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ArgumentMode::Words => "words",
            ArgumentMode::Expression => "expression",
        }
    }

    /// The parser's own spelling of the same mode.
    #[must_use]
    pub const fn as_parser_mode(self) -> ono_parser::ArgMode {
        match self {
            ArgumentMode::Words => ono_parser::ArgMode::Words,
            ArgumentMode::Expression => ono_parser::ArgMode::Expression,
        }
    }
}

impl fmt::Display for ArgumentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// When a command's `--confirm` option must be written for it to act (spec §11.6, §17.4).
///
/// Every mutating command with a `confirm` option refuses a bulk selection above the threshold
/// unless it is written. A command whose single action is already destructive — closing a
/// socket under a running process — declares `confirmation: always` and refuses without it
/// every time, so a script never reaches an action it did not spell out (ADR-0088).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confirmation {
    /// `--confirm` is needed for a selection above the bulk threshold.
    Bulk,
    /// `--confirm` is needed for every run.
    Always,
}

/// The spec §37 phase that delivers a command, where one does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The phase letter of spec §37, `A` through `J`.
    Delivered(char),
    /// No phase delivers it yet. An honest label, not a euphemism: the command is part of the
    /// product surface and nothing schedules it.
    Planned,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Phase::Delivered(letter) => write!(f, "{letter}"),
            Phase::Planned => f.write_str("planned"),
        }
    }
}

/// The declared type of a selector or an option, in the value vocabulary of spec §10.2 with the
/// three additions of ADR-0012 §7.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredType {
    /// `true` or `false`.
    Bool,
    /// A signed integer.
    Int,
    /// A binary floating-point number.
    Float,
    /// Text.
    String,
    /// A filesystem path.
    Path,
    /// A span of time with a unit suffix, such as `5s`.
    Duration,
    /// A quantity of information with a unit suffix, such as `1GiB`.
    ByteSize,
    /// An instant in time, in RFC 3339 form.
    Timestamp,
    /// A TCP or UDP port number.
    Port,
    /// An IP address.
    Ip,
    /// An IP address with a prefix length.
    IpNetwork,
    /// A record literal, which only an evaluated expression can supply.
    Record,
    /// Any value of the model, where the parameter genuinely spans the union (ADR-0012 §7).
    Value,
    /// An ordered list of the inner type.
    List(Box<DeclaredType>),
    /// An identity handle for an object of the named schema, such as `ref<ono.user/1>`.
    Ref(String),
}

impl DeclaredType {
    /// The spelling the registry files use, which is also how errors name the type.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            DeclaredType::Bool => "bool".to_owned(),
            DeclaredType::Int => "int".to_owned(),
            DeclaredType::Float => "float".to_owned(),
            DeclaredType::String => "string".to_owned(),
            DeclaredType::Path => "path".to_owned(),
            DeclaredType::Duration => "duration".to_owned(),
            DeclaredType::ByteSize => "bytesize".to_owned(),
            DeclaredType::Timestamp => "timestamp".to_owned(),
            DeclaredType::Port => "port".to_owned(),
            DeclaredType::Ip => "ip".to_owned(),
            DeclaredType::IpNetwork => "ipnetwork".to_owned(),
            DeclaredType::Record => "record".to_owned(),
            DeclaredType::Value => "value".to_owned(),
            DeclaredType::List(inner) => format!("list<{}>", inner.name()),
            DeclaredType::Ref(schema) => format!("ref<{schema}>"),
        }
    }

    /// Whether the type is satisfied by the presence of the option alone, so that `--tree` means
    /// `--tree=true` and consumes no following word.
    #[must_use]
    pub const fn is_flag(&self) -> bool {
        matches!(self, DeclaredType::Bool)
    }

    /// The values the type admits, where it admits a closed set metadata can enumerate.
    ///
    /// Only `bool` has one today: the registry declares no enumerated parameter, and the closed
    /// sets that exist in prose — the format names of `to`, the direction of `sort` — are not
    /// declared as data, so completion offers them through the provider hook instead.
    #[must_use]
    pub fn closed_set(&self) -> Option<&'static [&'static str]> {
        match self {
            DeclaredType::Bool => Some(&["false", "true"]),
            _ => None,
        }
    }

    /// Reinterprets a word's exact source text as a value of this type.
    ///
    /// This is the reinterpretation ADR-0009 deliberately keeps out of the parser: a words-mode
    /// argument arrives as the text that was typed, and the command's declared type decides what
    /// it means.
    ///
    /// # Errors
    ///
    /// [`ErrorCode::TypeInvalidUnit`] when a unit is unknown or belongs to another dimension, and
    /// [`ErrorCode::TypeMismatch`] for every other way the text fails to be a value of this type.
    pub fn coerce(&self, text: &str) -> Result<Value, ErrorValue> {
        match self {
            DeclaredType::Bool => match text {
                "true" => Ok(Value::Bool(true)),
                "false" => Ok(Value::Bool(false)),
                _ => Err(self.mismatch(text)),
            },
            DeclaredType::Int => parse_integer(text)
                .map(Value::Int)
                .ok_or_else(|| self.mismatch(text)),
            DeclaredType::Float => text
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| self.mismatch(text)),
            DeclaredType::String | DeclaredType::Value => Ok(Value::string(text)),
            DeclaredType::Path => Ok(Value::Path(std::path::Path::new(text).into())),
            DeclaredType::Duration => Duration::parse(text).map(Value::Duration),
            DeclaredType::ByteSize => ByteSize::parse(text).map(Value::ByteSize),
            DeclaredType::Timestamp => text
                .parse::<jiff::Timestamp>()
                .map(Value::Timestamp)
                .map_err(|_| self.mismatch(text)),
            DeclaredType::Port => text
                .parse::<u16>()
                .map(Value::Port)
                .map_err(|_| self.mismatch(text)),
            DeclaredType::Ip => text
                .parse::<std::net::IpAddr>()
                .map(Value::Ip)
                .map_err(|_| self.mismatch(text)),
            DeclaredType::IpNetwork => IpNetwork::parse(text).map(Value::IpNetwork),
            DeclaredType::Record => Err(self.mismatch(text)),
            DeclaredType::List(inner) => {
                let items = text
                    .split(',')
                    .map(|item| inner.coerce(item.trim()))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::list(items))
            }
            // A reference is resolved against a provider, which binding cannot reach. The
            // identity text is carried through so the command can resolve it (ADR-0012 §7).
            DeclaredType::Ref(_) => Ok(Value::string(text)),
        }
    }

    fn mismatch(&self, text: &str) -> ErrorValue {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`{text}` is not a valid `{}`", self.name()),
        )
    }
}

impl fmt::Display for DeclaredType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name())
    }
}

impl FromStr for DeclaredType {
    type Err = ErrorValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(inner) = text.strip_prefix("list<").and_then(|r| r.strip_suffix('>')) {
            return Ok(DeclaredType::List(Box::new(inner.parse()?)));
        }
        if let Some(schema) = text.strip_prefix("ref<").and_then(|r| r.strip_suffix('>')) {
            return Ok(DeclaredType::Ref(schema.to_owned()));
        }
        match text {
            "bool" => Ok(DeclaredType::Bool),
            "int" => Ok(DeclaredType::Int),
            "float" => Ok(DeclaredType::Float),
            "string" => Ok(DeclaredType::String),
            "path" => Ok(DeclaredType::Path),
            "duration" => Ok(DeclaredType::Duration),
            "bytesize" => Ok(DeclaredType::ByteSize),
            "timestamp" => Ok(DeclaredType::Timestamp),
            "port" => Ok(DeclaredType::Port),
            "ip" => Ok(DeclaredType::Ip),
            "ipnetwork" => Ok(DeclaredType::IpNetwork),
            "record" => Ok(DeclaredType::Record),
            "value" => Ok(DeclaredType::Value),
            _ => Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{text}` is not a declared type of the registry vocabulary"),
            )),
        }
    }
}

/// Parses an integer literal in the spellings ADR-0009 gives them: decimal, `0x`, `0b`, with `_`
/// separators and an optional sign.
fn parse_integer(text: &str) -> Option<i128> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let cleaned: String = digits.chars().filter(|c| *c != '_').collect();
    let magnitude = if let Some(hex) = cleaned.strip_prefix("0x") {
        i128::from_str_radix(hex, 16).ok()?
    } else if let Some(binary) = cleaned.strip_prefix("0b") {
        i128::from_str_radix(binary, 2).ok()?
    } else if cleaned.is_empty() || !cleaned.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    } else {
        cleaned.parse::<i128>().ok()?
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// The input or output type of a command, as the registry writes it.
///
/// The registry spells these as a small type language rather than as a schema id alone —
/// `stream<ono.process/1>`, `null | ono.process/1`, `string | bytes` — because a command's input
/// is genuinely a union and its output is genuinely a stream or not. This type keeps the exact
/// text, which is what help and `explain` show, and picks the schema references out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoType {
    text: String,
}

impl IoType {
    /// The type spelled out, for a plan that threads one stage's output into the next.
    pub(crate) fn from_text(text: &str) -> Self {
        Self {
            text: text.to_owned(),
        }
    }

    /// The type exactly as the registry writes it.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Whether the type is a stream of values rather than a single one.
    #[must_use]
    pub fn is_stream(&self) -> bool {
        self.alternatives().any(|part| part.starts_with("stream<"))
    }

    /// Whether the type admits `null`, which is how a command says it can start a pipeline.
    #[must_use]
    pub fn accepts_null(&self) -> bool {
        self.alternatives().any(|part| part == "null")
    }

    /// Every schema id the type mentions, in the order it mentions them.
    #[must_use]
    pub fn schema_references(&self) -> Vec<&str> {
        self.alternatives()
            .map(Self::element_of)
            .filter(|part| part.contains('/'))
            .collect()
    }

    /// The single schema this type carries, where it carries exactly one.
    ///
    /// This is what `explain` threads from one stage to the next: a stage whose output is
    /// `stream<ono.process/1>` hands `ono.process/1` to the stage after it.
    #[must_use]
    pub fn element_schema(&self) -> Option<&str> {
        let references = self.schema_references();
        match references.as_slice() {
            [only] => Some(only),
            _ => None,
        }
    }

    /// Whether the type names no concrete element at all — `any`, `stream<any>`, `value` — so a
    /// plan carries the upstream element type through it rather than losing it.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.alternatives()
            .filter(|part| *part != "null")
            .all(|part| matches!(Self::element_of(part), "any" | "value"))
    }

    /// Whether the type admits raw bytes.
    #[must_use]
    pub fn admits_bytes(&self) -> bool {
        self.alternatives().any(|part| part.starts_with("bytes"))
    }

    /// Whether the type admits text.
    #[must_use]
    pub fn admits_text(&self) -> bool {
        self.alternatives().any(|part| part.starts_with("string"))
    }

    /// The alternatives of a union type, each trimmed.
    fn alternatives(&self) -> impl Iterator<Item = &str> {
        self.text.split('|').map(str::trim)
    }

    /// The element of `stream<T>`, or the part itself when it is not a stream.
    fn element_of(part: &str) -> &str {
        part.strip_prefix("stream<")
            .and_then(|inner| inner.strip_suffix('>'))
            .unwrap_or(part)
    }

    /// The same type with its element replaced, which is how a transform that declares
    /// `stream<any>` reports the concrete type flowing through it.
    pub(crate) fn with_element(&self, element: &str) -> Self {
        let text = if self.is_stream() {
            format!("stream<{element}>")
        } else {
            element.to_owned()
        };
        Self { text }
    }
}

impl fmt::Display for IoType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// One selector or option of a command.
///
/// Selectors and options carry the same declaration in the registry — a name, a type and a doc —
/// and differ only in how they are written: a selector is positional, an option is `--named`.
#[derive(Debug, Clone, PartialEq)]
pub struct ParameterSpec {
    name: String,
    declared_type: DeclaredType,
    doc: String,
    repeatable: bool,
    optional_value: bool,
    default_text: Option<String>,
    default_value: Option<Value>,
}

impl ParameterSpec {
    /// The parameter's name, without the `--` an option is written with.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type the registry declares for it.
    #[must_use]
    pub fn declared_type(&self) -> &DeclaredType {
        &self.declared_type
    }

    /// What it is for, as the registry documents it.
    #[must_use]
    pub fn doc(&self) -> &str {
        &self.doc
    }

    /// Whether it may be written more than once. A `list<T>` parameter always accumulates.
    #[must_use]
    pub fn is_repeatable(&self) -> bool {
        self.repeatable || matches!(self.declared_type, DeclaredType::List(_))
    }

    /// Whether the option may be written without its value.
    ///
    /// Spec v0.4 §6.1 writes `look --changes [duration]` and §6.2 writes `near --changed
    /// [duration]`: the option carries a value where the caller gives one, and means the
    /// configured default where the caller does not. Every other option keeps the rule that a
    /// declared type is a promise of a value, so a missing one is a usage error (ADR-0144).
    #[must_use]
    pub fn has_optional_value(&self) -> bool {
        self.optional_value
    }

    /// The default exactly as the registry writes it, for help and completion.
    #[must_use]
    pub fn default_text(&self) -> Option<&str> {
        self.default_text.as_deref()
    }

    /// The default as a value of the declared type, applied when the parameter is absent.
    #[must_use]
    pub fn default_value(&self) -> Option<&Value> {
        self.default_value.as_ref()
    }

    /// The values completion can offer from metadata alone.
    ///
    /// A `bool` has a closed set; every other parameter offers at most its declared default, and
    /// anything richer — the users on this machine, the services of this host — needs a provider
    /// and reaches completion through its hook instead.
    #[must_use]
    pub fn closed_set(&self) -> Vec<String> {
        if let Some(values) = self.declared_type.closed_set() {
            return values.iter().map(|value| (*value).to_owned()).collect();
        }
        self.default_text
            .as_deref()
            .map(|default| vec![default.to_owned()])
            .unwrap_or_default()
    }
}

/// One command of the registry: everything `docs/contracts/commands/` declares about it.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandContract {
    id: String,
    family: String,
    verb: String,
    target: Option<String>,
    summary: String,
    note: Option<String>,
    stability: Stability,
    validation_required: bool,
    confirmation: Confirmation,
    argument_mode: ArgumentMode,
    input: IoType,
    output: IoType,
    provider_capability: Option<String>,
    selectors: Vec<ParameterSpec>,
    options: Vec<ParameterSpec>,
    privilege: Privilege,
    streaming: bool,
    execution: Option<ExecutionClass>,
    phase: Phase,
    examples: Vec<String>,
    origin: Origin,
    required_capabilities: Vec<String>,
}

/// Where a pipeline operation sits in the streaming classification matrix of v0.4.1 Appendix E.
///
/// The matrix is `docs/contracts/hardening/streaming_classification.yaml`, and Appendix E's closing
/// sentence is what makes it a contract rather than a note: *"If a command cannot be placed in
/// this matrix, its execution semantics are underspecified and MUST be resolved before release."*
/// `cargo xtask spec-check` refuses a stream-consuming command that names no class.
///
/// The two properties are what the rest of the hardening layer acts on: whether the stage refuses
/// a declared-unbounded upstream (§22.3), and whether it may hold its input within the budget of
/// §22.2. `explain` derives what it shows from these rather than restating them (§22.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ExecutionClass {
    /// One value in, zero or more out, with no state that grows.
    ItemTransform,
    /// A test applied to each value in turn.
    Predicate,
    /// Answers from a bounded prefix and then stops reading.
    Prefix,
    /// Folds into state of a fixed or explicitly bounded size.
    IncrementalAggregate,
    /// Emits the same values in an order it can only know once it has them all.
    GlobalReorder,
    /// Emits one value per group, which it can only close once the input has ended.
    GlobalGrouping,
    /// Holds a collection because its answer is defined over the whole of it.
    ExplicitCollect,
    /// Maintains a bounded model of current state and repaints it.
    LiveView,
}

impl ExecutionClass {
    /// Every class of Appendix E, in its own order.
    pub const ALL: &'static [ExecutionClass] = &[
        ExecutionClass::ItemTransform,
        ExecutionClass::Predicate,
        ExecutionClass::Prefix,
        ExecutionClass::IncrementalAggregate,
        ExecutionClass::GlobalReorder,
        ExecutionClass::GlobalGrouping,
        ExecutionClass::ExplicitCollect,
        ExecutionClass::LiveView,
    ];

    /// The id the registry spells it with.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            ExecutionClass::ItemTransform => "item_transform",
            ExecutionClass::Predicate => "predicate",
            ExecutionClass::Prefix => "prefix",
            ExecutionClass::IncrementalAggregate => "incremental_aggregate",
            ExecutionClass::GlobalReorder => "global_reorder",
            ExecutionClass::GlobalGrouping => "global_grouping",
            ExecutionClass::ExplicitCollect => "explicit_collect",
            ExecutionClass::LiveView => "live_view",
        }
    }

    /// Resolves a class from its id.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|class| class.id() == id)
    }

    /// Whether the stage refuses a declared-unbounded upstream immediately (§22.3).
    #[must_use]
    pub const fn requires_finite_input(self) -> bool {
        matches!(
            self,
            ExecutionClass::GlobalReorder
                | ExecutionClass::GlobalGrouping
                | ExecutionClass::ExplicitCollect
        )
    }

    /// Whether the stage may hold its input, within the budget of §22.2.
    #[must_use]
    pub const fn may_materialize(self) -> bool {
        self.requires_finite_input()
    }

    /// How `explain` names the execution mode (§22.4).
    #[must_use]
    pub const fn execution_mode(self) -> &'static str {
        if self.may_materialize() {
            "global materialization"
        } else {
            "streaming"
        }
    }
}

impl std::fmt::Display for ExecutionClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

impl CommandContract {
    /// The stable command id, `ono.<target>.<verb>` (ADR-0012 §2).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Where the entry came from: the core, or the package that contributed it (spec §31.64).
    #[must_use]
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// The same contract, attributed to `origin`. The host uses it at registration; a package
    /// never sets its own origin.
    #[must_use]
    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    /// The registry file the command was declared in, such as `process`.
    #[must_use]
    pub fn family(&self) -> &str {
        &self.family
    }

    /// The verb the user types.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// The target word the user types after the verb, where the command takes one.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// The command as it is written, such as `get process` or `where`.
    #[must_use]
    pub fn spelling(&self) -> String {
        match &self.target {
            Some(target) => format!("{} {target}", self.verb),
            None => self.verb.clone(),
        }
    }

    /// One line saying what the command does.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The longer note the registry attaches, where it attaches one.
    #[must_use]
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// How much of a compatibility promise the command is.
    #[must_use]
    pub fn stability(&self) -> Stability {
        self.stability
    }

    /// When the command's `--confirm` option is required rather than merely honoured.
    #[must_use]
    pub fn confirmation(&self) -> Confirmation {
        self.confirmation
    }

    /// Whether the entry records a `?` cell of the §52 matrix whose usefulness must be validated
    /// before it is implemented.
    #[must_use]
    pub fn validation_required(&self) -> bool {
        self.validation_required
    }

    /// How the command's arguments are lexed and parsed.
    #[must_use]
    pub fn argument_mode(&self) -> ArgumentMode {
        self.argument_mode
    }

    /// What the command accepts through the pipeline.
    #[must_use]
    pub fn input(&self) -> &IoType {
        &self.input
    }

    /// What the command emits.
    #[must_use]
    pub fn output(&self) -> &IoType {
        &self.output
    }

    /// What a provider must be able to do for the command to work.
    #[must_use]
    pub fn provider_capability(&self) -> Option<&str> {
        self.provider_capability.as_deref()
    }

    /// The positional selectors, in the order they are written.
    #[must_use]
    pub fn selectors(&self) -> &[ParameterSpec] {
        &self.selectors
    }

    /// The `--named` options.
    #[must_use]
    pub fn options(&self) -> &[ParameterSpec] {
        &self.options
    }

    /// A selector by name.
    #[must_use]
    pub fn selector(&self, name: &str) -> Option<&ParameterSpec> {
        self.selectors.iter().find(|selector| selector.name == name)
    }

    /// An option by name.
    #[must_use]
    pub fn option(&self, name: &str) -> Option<&ParameterSpec> {
        self.options.iter().find(|option| option.name == name)
    }

    /// What the user needs before the command runs.
    #[must_use]
    pub fn privilege(&self) -> Privilege {
        self.privilege
    }

    /// Whether the command produces its output incrementally.
    #[must_use]
    pub fn is_streaming(&self) -> bool {
        self.streaming
    }

    /// Where the command sits in the streaming classification matrix of v0.4.1 Appendix E.
    ///
    /// `None` for a command that consumes no stream, which is most of them: Appendix E classifies
    /// *pipeline operations*, and a producer that reads a provider is not one.
    #[must_use]
    pub const fn execution(&self) -> Option<ExecutionClass> {
        self.execution
    }

    /// Whether the command refuses a declared-unbounded upstream immediately (§22.3).
    #[must_use]
    pub fn requires_finite_input(&self) -> bool {
        self.execution
            .is_some_and(ExecutionClass::requires_finite_input)
    }

    /// Whether the command may hold its input within the materialization budget (§22.1, §22.2).
    #[must_use]
    pub fn materializes(&self) -> bool {
        self.execution.is_some_and(ExecutionClass::may_materialize)
    }

    /// The spec §37 phase that delivers it.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// The KUANG/11 capabilities a contributed command needs before it runs (spec §31.22).
    ///
    /// Empty for a core command, whose authority is the provider capability of
    /// [`CommandContract::provider_capability`] instead.
    #[must_use]
    pub fn required_capabilities(&self) -> &[String] {
        &self.required_capabilities
    }

    /// The examples the registry documents, every one of which must parse and run (spec §50).
    #[must_use]
    pub fn examples(&self) -> &[String] {
        &self.examples
    }
}

// --- loading -------------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub(crate) struct RawFamily {
    pub(crate) family: String,
    pub(crate) commands: Vec<RawCommand>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawCommand {
    id: String,
    verb: String,
    target: Option<String>,
    summary: String,
    #[serde(default)]
    note: Option<String>,
    stability: String,
    #[serde(default)]
    validation_required: bool,
    #[serde(default)]
    confirmation: Option<String>,
    argument_mode: String,
    input: String,
    output: String,
    provider_capability: Option<String>,
    #[serde(default)]
    selectors: Vec<RawParameter>,
    #[serde(default)]
    options: Vec<RawParameter>,
    privilege: String,
    streaming: bool,
    #[serde(default)]
    execution: Option<String>,
    phase: String,
    #[serde(default)]
    examples: Vec<RawExample>,
}

/// An example, as YAML reads it.
///
/// Most are plain scalars. One is not: `get process | select pid name {mem_mb: memory / 1MiB}` is
/// unquoted in `docs/contracts/commands/data.yaml`, and YAML reads a plain scalar containing `: ` as a
/// mapping rather than as text. Rejoining the mapping with `": "` restores the line the file
/// actually contains, so the example survives verbatim without the registry being edited from
/// here. Quoting the scalar in the contract file would remove the need for this arm.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawExample {
    Text(String),
    Folded(serde_yaml_ng::Mapping),
}

impl RawExample {
    fn text(&self) -> String {
        match self {
            RawExample::Text(text) => text.clone(),
            RawExample::Folded(mapping) => mapping
                .iter()
                .map(|(key, value)| format!("{}: {}", scalar_text(key), scalar_text(value)))
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// A YAML scalar as the text it was written with.
fn scalar_text(value: &serde_yaml_ng::Value) -> String {
    match value {
        serde_yaml_ng::Value::String(text) => text.clone(),
        serde_yaml_ng::Value::Bool(flag) => flag.to_string(),
        serde_yaml_ng::Value::Number(number) => number.to_string(),
        serde_yaml_ng::Value::Null => "null".to_owned(),
        other => serde_yaml_ng::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_owned(),
    }
}

#[derive(Debug, Deserialize)]
struct RawParameter {
    name: String,
    #[serde(rename = "type")]
    declared_type: String,
    doc: String,
    #[serde(default)]
    repeatable: bool,
    #[serde(default)]
    optional_value: bool,
    #[serde(default)]
    default: Option<RawScalar>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawScalar {
    Bool(bool),
    Int(i64),
    Text(String),
}

impl RawScalar {
    fn text(&self) -> String {
        match self {
            RawScalar::Bool(value) => value.to_string(),
            RawScalar::Int(value) => value.to_string(),
            RawScalar::Text(value) => value.clone(),
        }
    }
}

/// A contract file that does not typecheck against this vocabulary is a build defect, so loading
/// says exactly which entry and which field went wrong.
fn contract_error(id: &str, detail: impl AsRef<str>) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!(
            "`{id}` in docs/contracts/commands/ is not a valid contract: {}",
            detail.as_ref()
        ),
    )
}

impl RawCommand {
    pub(crate) fn into_contract(self, family: &str) -> Result<CommandContract, ErrorValue> {
        let id = self.id.clone();
        let stability = match self.stability.as_str() {
            "stable" => Stability::Stable,
            "experimental" => Stability::Experimental,
            "planned" => Stability::Planned,
            other => return Err(contract_error(&id, format!("unknown stability `{other}`"))),
        };
        let argument_mode = match self.argument_mode.as_str() {
            "words" => ArgumentMode::Words,
            "expression" => ArgumentMode::Expression,
            other => {
                return Err(contract_error(
                    &id,
                    format!("unknown argument mode `{other}`"),
                ));
            }
        };
        let privilege = match self.privilege.as_str() {
            "none" => Privilege::None,
            "elevated" => Privilege::Elevated,
            "conditional" => Privilege::Conditional,
            other => return Err(contract_error(&id, format!("unknown privilege `{other}`"))),
        };
        let confirmation = match self.confirmation.as_deref() {
            None | Some("bulk") => Confirmation::Bulk,
            Some("always") => Confirmation::Always,
            Some(other) => {
                return Err(contract_error(
                    &id,
                    format!("unknown confirmation `{other}`; it is `bulk` or `always`"),
                ));
            }
        };
        let phase = match self.phase.as_str() {
            "planned" => Phase::Planned,
            letter if letter.len() == 1 && letter.starts_with(|c: char| c.is_ascii_uppercase()) => {
                Phase::Delivered(letter.as_bytes()[0] as char)
            }
            other => return Err(contract_error(&id, format!("unknown phase `{other}`"))),
        };
        let selectors = parameters(&id, self.selectors)?;
        let options = parameters(&id, self.options)?;
        let execution = match self.execution.as_deref() {
            None => None,
            Some(name) => Some(ExecutionClass::from_id(name).ok_or_else(|| {
                contract_error(
                    &id,
                    format!(
                        "unknown execution class `{name}`; v0.4.1 Appendix E has eight, and \
                         docs/contracts/hardening/streaming_classification.yaml lists them"
                    ),
                )
            })?),
        };

        Ok(CommandContract {
            id: self.id,
            family: family.to_owned(),
            verb: self.verb,
            target: self.target,
            summary: self.summary,
            note: self.note,
            stability,
            validation_required: self.validation_required,
            confirmation,
            argument_mode,
            input: IoType { text: self.input },
            output: IoType { text: self.output },
            provider_capability: self.provider_capability,
            selectors,
            options,
            privilege,
            streaming: self.streaming,
            execution,
            phase,
            examples: self.examples.iter().map(RawExample::text).collect(),
            // A contract file under `docs/contracts/commands/` is the core's own declaration. A
            // package's contribution is re-attributed by the host at registration, never by the
            // document it was read from (spec §31.64).
            origin: Origin::Core,
            required_capabilities: Vec::new(),
        })
    }
}

/// What a KUANG/11 package declares about one command it contributes (spec §31.22,
/// `docs/contracts/kuang/contributions.v1.yaml`).
///
/// The declaration crosses two boundaries with the same fields: the handshake, where the host
/// receives it from a running instance, and the package's own `contributions.commands`
/// documents, which the host reads without starting anything (spec §31.68). Both arrive here.
///
/// `origin` is not part of the declaration a package writes; the host sets it at registration
/// (ADR-0281).
#[derive(Debug, Clone, PartialEq)]
pub struct ContributedCommand {
    /// `<package.id>.command.<kebab-name>`.
    pub id: String,
    /// The verb the user types.
    pub verb: String,
    /// The target word after the verb.
    pub target: String,
    /// One line, for `help` and completion.
    pub summary: String,
    /// The declared input type, `None` for a command taking nothing through the pipeline.
    pub input: Option<String>,
    /// The declared output type.
    pub output: String,
    /// The KUANG/11 capabilities the command needs (spec §31.22).
    pub capabilities: Vec<String>,
    /// The argument mode from ADR-0009's table.
    pub argument_mode: String,
    /// Documented examples.
    pub examples: Vec<String>,
    /// The package that contributed it, as the host attributes it.
    pub origin: Origin,
}

impl ContributedCommand {
    /// The registry entry the declaration becomes.
    ///
    /// # Errors
    ///
    /// `package.invalid` when a declared word is not one the registry's vocabulary carries — an
    /// unknown argument mode, or an id outside the package's own namespace. The refusal names
    /// the command, because a package that declares nonsense must be told which line is wrong
    /// (`docs/contracts/kuang/contributions.v1.yaml` → `registration_checks`).
    pub fn into_contract(self) -> Result<CommandContract, ErrorValue> {
        let Origin::Plugin { package, .. } = &self.origin else {
            return Err(contributed_error(
                &self.id,
                "a contributed command is attributed to the package that contributed it; the \
                 core never contributes one",
            ));
        };
        // Two kinds of entry reach this constructor and the id is where they are told apart. A
        // command a package declares is `<package.id>.command.<kebab>`; the `get` the host
        // synthesises for a target the package answers for is `<package.id>.target.<kebab>`
        // (spec §31.23). Both are namespaced under the package, which is the rule §31.5 actually
        // states; the infix says how an invocation is routed, and keeping it in the id means
        // `get command`, `help` and `explain` show the distinction rather than hiding it.
        let command_namespace = format!("{package}.command.");
        let target_namespace = format!("{package}.target.");
        if !self.id.starts_with(&command_namespace) && !self.id.starts_with(&target_namespace) {
            return Err(contributed_error(
                &self.id,
                format!(
                    "the id is not `{command_namespace}<kebab-name>` or \
                     `{target_namespace}<kebab-name>` (spec §31.5, §31.22, §31.23)"
                ),
            ));
        }
        let argument_mode = match self.argument_mode.as_str() {
            "words" => ArgumentMode::Words,
            "expression" => ArgumentMode::Expression,
            other => {
                return Err(contributed_error(
                    &self.id,
                    format!("unknown argument mode `{other}`"),
                ));
            }
        };
        if self.verb.is_empty() || self.target.is_empty() || self.summary.is_empty() {
            return Err(contributed_error(
                &self.id,
                "a contribution declares a verb, a target and a summary",
            ));
        }
        let output = IoType { text: self.output };
        Ok(CommandContract {
            id: self.id,
            // The registry's `family` is the file a command was declared in; for a contribution
            // that is the package itself.
            family: package.clone(),
            verb: self.verb,
            target: Some(self.target),
            summary: self.summary,
            note: None,
            // A contributed command is not named by a normative section of the specification, so
            // it is not a compatibility promise of Ono's (spec §36.3). Its own package decides
            // what it promises, and says so in its version.
            stability: Stability::Experimental,
            validation_required: false,
            confirmation: Confirmation::Bulk,
            argument_mode,
            input: IoType {
                text: self.input.unwrap_or_else(|| "null".to_owned()),
            },
            // Whether the command emits incrementally is not a separate claim: it is what the
            // declared output type says.
            streaming: output.text.starts_with("stream<") || output.text.starts_with("stream "),
            // A contribution declares no execution class: v0.4.1 Appendix E classifies the core's
            // pipeline operations, and a plugin command runs inside the KUANG/11 supervisor with
            // its own quotas (spec §31.15) rather than under the evaluator's materialization
            // budget. `explain` therefore shows it as it shows any other stage, without a
            // materialization line it cannot substantiate.
            execution: None,
            output,
            // `provider_capability` names an entry of `docs/contracts/capabilities.yaml`, which is the
            // core's provider vocabulary. A package's authority is its KUANG/11 capabilities,
            // which are a different register and are carried separately.
            provider_capability: None,
            required_capabilities: self.capabilities,
            // A contribution declares no selectors or options: the wire contribution of
            // `docs/contracts/kuang/protocol.v1.yaml` has no field for them, and the arguments a
            // contributed command receives are the words the user typed (spec §31.22). The
            // registry does not invent parameters nobody declared.
            selectors: Vec::new(),
            options: Vec::new(),
            // The shell cannot know whether the code inside a package needs privilege; the
            // capabilities it asked for say what it may do, and `conditional` is the honest
            // answer to a question the host cannot decide (spec §17).
            privilege: Privilege::Conditional,
            // Phase I is what delivers a contributed command: the extension runtime (spec §37).
            phase: Phase::Delivered('I'),
            examples: self.examples,
            origin: self.origin,
        })
    }
}

fn contributed_error(id: &str, detail: impl fmt::Display) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::KuangPackageInvalid,
        format!("the contributed command `{id}` is invalid: {detail}"),
    )
}

fn parameters(id: &str, raw: Vec<RawParameter>) -> Result<Vec<ParameterSpec>, ErrorValue> {
    raw.into_iter()
        .map(|parameter| {
            let declared_type: DeclaredType =
                parameter
                    .declared_type
                    .parse()
                    .map_err(|error: ErrorValue| {
                        contract_error(id, format!("`{}`: {}", parameter.name, error.message()))
                    })?;
            let default_text = parameter.default.as_ref().map(RawScalar::text);
            let default_value = default_text
                .as_deref()
                .map(|text| {
                    declared_type.coerce(text).map_err(|error| {
                        contract_error(
                            id,
                            format!(
                                "`{}` declares default `{text}`, which is not a `{}`: {}",
                                parameter.name,
                                declared_type.name(),
                                error.message()
                            ),
                        )
                    })
                })
                .transpose()?;
            Ok(ParameterSpec {
                name: parameter.name,
                declared_type,
                doc: parameter.doc,
                repeatable: parameter.repeatable,
                optional_value: parameter.optional_value,
                default_text,
                default_value,
            })
        })
        .collect()
}

/// One verb of `docs/contracts/verbs.yaml` (spec §7.1).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VerbSpec {
    id: String,
    verb: String,
    semantics: String,
    #[serde(default)]
    typical_targets: Vec<String>,
    pipeline_role: String,
    mutating: bool,
}

impl VerbSpec {
    /// The stable verb id, `ono.verb.<verb>`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The word the user types.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// One line of semantics, from the spec §7.1 table.
    #[must_use]
    pub fn semantics(&self) -> &str {
        &self.semantics
    }

    /// The targets spec §7.1 names as typical. Not a closed list and not a constraint: the
    /// commands that exist are in `docs/contracts/commands/`.
    #[must_use]
    pub fn typical_targets(&self) -> &[String] {
        &self.typical_targets
    }

    /// Where the verb sits in a pipeline — producer, transform, terminal.
    #[must_use]
    pub fn pipeline_role(&self) -> &str {
        &self.pipeline_role
    }

    /// Whether the verb changes state outside the shell's own view of the world.
    #[must_use]
    pub fn is_mutating(&self) -> bool {
        self.mutating
    }
}

/// One target of `docs/contracts/targets.yaml` (spec §8).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TargetSpec {
    id: String,
    name: String,
    category: String,
    summary: String,
    schema: Option<String>,
    phase: String,
}

impl TargetSpec {
    /// The stable target id, `ono.target.<name>`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The word the user types after a verb.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `system`, `development` or `infrastructure`, as spec §8 groups them.
    #[must_use]
    pub fn category(&self) -> &str {
        &self.category
    }

    /// What the target denotes.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The canonical schema a producer of this target emits, where one is defined yet.
    #[must_use]
    pub fn schema(&self) -> Option<&str> {
        self.schema.as_deref()
    }

    /// The spec §37 phase that delivers the target, or `planned`.
    #[must_use]
    pub fn phase(&self) -> &str {
        &self.phase
    }
}

/// One provider capability of `docs/contracts/capabilities.yaml` (spec §27.1).
///
/// These are what a command needs from a provider. They are *not* the KUANG/11 capabilities of
/// spec §31.16, which are a security boundary; conflating the two is how someone eventually
/// grants a package `process.list` believing it is `process.read` (ADR-0012 §11).
#[derive(Debug, Clone, PartialEq)]
pub struct CapabilitySpec {
    id: String,
    summary: String,
    risk: ono_provider_api::Risk,
    elevation: Elevation,
}

impl CapabilitySpec {
    /// The capability id a command's `provider_capability` names.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the capability lets a provider do.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// How much it could change or reveal.
    #[must_use]
    pub fn risk(&self) -> ono_provider_api::Risk {
        self.risk
    }

    /// Whether it needs privilege.
    #[must_use]
    pub fn elevation(&self) -> Elevation {
        self.elevation
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawCapabilityFile {
    pub(crate) provider_capabilities: Vec<RawCapability>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawCapability {
    id: String,
    summary: String,
    risk: String,
    elevation: String,
}

impl RawCapability {
    pub(crate) fn into_spec(self) -> Result<CapabilitySpec, ErrorValue> {
        let risk = match self.risk.as_str() {
            "read" => ono_provider_api::Risk::Read,
            "observe" => ono_provider_api::Risk::Observe,
            "mutate" => ono_provider_api::Risk::Mutate,
            "destructive" => ono_provider_api::Risk::Destructive,
            other => {
                return Err(contract_error(
                    &self.id,
                    format!("unknown risk `{other}` in docs/contracts/capabilities.yaml"),
                ));
            }
        };
        let elevation = match self.elevation.as_str() {
            "none" => Elevation::None,
            "conditional" => Elevation::Conditional,
            "required" => Elevation::Required,
            other => {
                return Err(contract_error(
                    &self.id,
                    format!("unknown elevation `{other}` in docs/contracts/capabilities.yaml"),
                ));
            }
        };
        Ok(CapabilitySpec {
            id: self.id,
            summary: self.summary,
            risk,
            elevation,
        })
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawVerbFile {
    pub(crate) verbs: Vec<VerbSpec>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct RawTargetFile {
    pub(crate) targets: Vec<TargetSpec>,
}
