//! The adapter pack contract of `docs/spec/adapters/schema.yaml` (spec v0.3 §1.44, ADR-0055).
//!
//! A pack is data: what an executable family looks like, which invocations are adapted, how
//! the output decodes and onto which canonical schema. Parsing fails closed on unknown fields;
//! `validate` checks everything parsing cannot — schema ids, capability grants, fixtures.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::OnceLock;

use ono_value::{SchemaId, SchemaRegistry};
use serde::Deserialize;

use crate::version::VersionRange;

/// The first-party packs bundled with the shell, as data (spec v0.3 §1.66).
const FIRST_PARTY: &[&str] = &[
    include_str!("../../../docs/spec/adapters/first-party/util-linux.yaml"),
    include_str!("../../../docs/spec/adapters/first-party/iproute2.yaml"),
    include_str!("../../../docs/spec/adapters/first-party/systemd.yaml"),
    include_str!("../../../docs/spec/adapters/first-party/procps.yaml"),
    include_str!("../../../docs/spec/adapters/first-party/coreutils.yaml"),
    include_str!("../../../docs/spec/adapters/first-party/findutils.yaml"),
];

/// The decoders implemented in Rust that a `builtin` decoder may name.
const BUILTIN_DECODERS: &[&str] = &[];

/// Something a pack promises that the contract does not allow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The pack, or `<pack>/<adapter>`, the problem is in.
    pub location: String,
    /// What is wrong, in one sentence.
    pub detail: String,
}

/// Distribution tier of a pack (spec v0.3 §1.27).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// Shipped with the shell.
    FirstParty,
    /// Reviewed and recommended by the project.
    Recommended,
    /// Published by a third party.
    Community,
    /// Not yet trusted to influence structured output by default.
    Experimental,
}

/// Strategy tier of an adapter (spec v0.3 §1.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum StrategyTier {
    /// A native machine-readable protocol.
    A,
    /// A stable explicit field protocol.
    B,
    /// A version-constrained human-output parser.
    C,
}

/// The demands an adapter answers (ADR-0052).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DemandKind {
    /// A native consumer over objects.
    Structured,
    /// The terminal.
    Interactive,
}

/// What an unsupported invocation does under an interactive demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fallback {
    /// Run the program raw.
    Raw,
    /// Fail with the `adapter.*` error.
    Error,
}

/// How decoded values are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DecoderKind {
    /// A JSON document.
    Json,
    /// One JSON document per line, decoded as the lines arrive.
    Jsonl,
    /// An explicit field protocol.
    Lines,
    /// `key=value` lines, one record per blank-line-separated block.
    Properties,
    /// A decoder implemented in Rust.
    Builtin,
}

impl DecoderKind {
    /// Whether records can be decoded while the child still runs: one document or record per
    /// line arrives whole, a document or a block does not.
    #[must_use]
    pub fn streams(self) -> bool {
        matches!(self, Self::Jsonl | Self::Lines)
    }
}

/// How much a builtin decoder can be trusted across versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stability {
    /// The format is documented as stable.
    Stable,
    /// The parser reads human output and is pinned to a version range.
    VersionConstrained,
}

/// How a bare number in the tool's output is to be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Unit {
    /// Bytes.
    Bytes,
    /// Kibibytes.
    Kib,
    /// Mebibytes.
    Mib,
    /// Seconds.
    Seconds,
    /// Milliseconds.
    Milliseconds,
    /// Microseconds.
    Microseconds,
    /// Per cent.
    Percent,
}

/// How faithfully a canonical field reproduces the tool's output (spec v0.3 §1.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Exactness {
    /// Taken as the tool reported it.
    Exact,
    /// Converted or translated, losslessly.
    Normalized,
    /// Derived from something the tool did not state directly.
    Inferred,
}

/// A derivation the decoder performs that the tool did not state (spec v0.3 §1.8 `inferred`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Inference {
    /// `inet` or `inet6` from an IP address or network.
    IpFamily,
    /// The program's name from a command line: the basename of its first word, brackets
    /// stripped for a kernel thread.
    ProgramName,
    /// An instant: the moment of decoding minus this many elapsed seconds.
    StartedFromElapsed,
}

/// Whether further positional words may follow the matched ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Positionals {
    /// They pass through.
    Allow,
    /// They make the invocation unsupported.
    Forbid,
}

/// What the child sees on stdin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StdinMode {
    /// Nothing.
    Null,
    /// The pipeline's stdin.
    Inherit,
}

/// A pack of adapters: one executable grant, several adapters.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterPack {
    format: String,
    package: Package,
    roles: Vec<String>,
    capabilities: Capabilities,
    adapters: Vec<Adapter>,
}

/// Identity and tier of a pack.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    id: String,
    name: String,
    version: String,
    publisher: String,
    tier: Tier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Capabilities {
    #[serde(rename = "process.exec")]
    process_exec: ProcessExec,
}

/// The `process.exec` grant of spec v0.3 §1.22.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessExec {
    executables: Vec<String>,
    argv_policy: String,
}

/// One adapter: an executable family, one schema, one decoder, several invocations.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adapter {
    id: String,
    summary: String,
    executable: Executable,
    tier: StrategyTier,
    output_demand: Vec<DemandKind>,
    fallback: Fallback,
    schema: String,
    decoder: Decoder,
    fields: BTreeMap<String, FieldMap>,
    #[serde(default)]
    literals: BTreeMap<String, serde_yaml_ng::Value>,
    invocations: Vec<Invocation>,
    limits: Vec<String>,
    fixtures: String,
    #[serde(skip)]
    pack_id: String,
    #[serde(skip)]
    pack_version: String,
}

/// The executable family an adapter answers to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Executable {
    names: Vec<String>,
    versions: String,
    #[serde(default)]
    version_probe: Option<VersionProbe>,
}

/// A declared, bounded version probe (spec v0.3 §1.46).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionProbe {
    argv: Vec<String>,
    pattern: String,
    cache: String,
}

/// How the tool's stdout becomes records.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decoder {
    kind: DecoderKind,
    #[serde(default)]
    records: Option<String>,
    #[serde(default)]
    nested: Option<String>,
    #[serde(default)]
    children: Option<String>,
    #[serde(default)]
    field_separator: Option<String>,
    #[serde(default)]
    record_separator: Option<String>,
    #[serde(default)]
    columns: Option<Vec<String>>,
    #[serde(default)]
    header_lines: Option<usize>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    stability: Option<Stability>,
}

/// How one decoded field becomes one canonical field.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldMap {
    from: String,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    first: Option<bool>,
    #[serde(default)]
    basename: Option<bool>,
    #[serde(default)]
    infer: Option<Inference>,
    #[serde(default)]
    unit: Option<Unit>,
    #[serde(default)]
    split: Option<String>,
    #[serde(default)]
    contains: Option<String>,
    #[serde(default)]
    map: Option<BTreeMap<String, serde_yaml_ng::Value>>,
    #[serde(default)]
    exactness: Option<Exactness>,
}

/// One user-facing invocation and the plan it compiles to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    id: String,
    summary: String,
    #[serde(rename = "match")]
    matcher: Match,
    plan: Plan,
}

/// What selects an invocation (spec v0.3 §1.15).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Match {
    words: Vec<Vec<String>>,
    flags: Flags,
    positionals: Positionals,
}

/// The flags an invocation lets through.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flags {
    allow: Vec<String>,
    allow_with_value: Vec<String>,
    #[serde(default)]
    require: Vec<String>,
}

/// The machine-oriented invocation (spec v0.3 §1.7).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    argv: Vec<String>,
    append_user_flags: bool,
    env: BTreeMap<String, String>,
    stdin: StdinMode,
    #[serde(default)]
    unbounded: bool,
    #[serde(default)]
    trailing_argv: Vec<String>,
}

/// A fixture's sidecar: where the bytes came from and what they must decode to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    invocation: Vec<String>,
    tool_version: String,
    distro: String,
    expect: Expectation,
}

/// What a fixture's bytes must decode to.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    #[serde(default)]
    records: Option<Vec<BTreeMap<String, serde_yaml_ng::Value>>>,
    #[serde(default)]
    error: Option<String>,
}

impl AdapterPack {
    /// Parses a pack, failing closed on anything the contract does not name.
    ///
    /// # Errors
    ///
    /// The YAML error, including an unknown field, as text.
    pub fn parse(yaml: &str) -> Result<Self, String> {
        let mut pack: Self = serde_yaml_ng::from_str(yaml).map_err(|error| error.to_string())?;
        for adapter in &mut pack.adapters {
            adapter.pack_id.clone_from(&pack.package.id);
            adapter.pack_version.clone_from(&pack.package.version);
        }
        Ok(pack)
    }

    /// The pack id, `org.ono.compat.util-linux`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.package.id
    }

    /// The pack's display name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.package.name
    }

    /// The pack's own version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.package.version
    }

    /// The distribution tier.
    #[must_use]
    pub fn tier(&self) -> Tier {
        self.package.tier
    }

    /// The executables the pack is granted.
    #[must_use]
    pub fn executables(&self) -> &[String] {
        &self.capabilities.process_exec.executables
    }

    /// The adapters, in contract order.
    #[must_use]
    pub fn adapters(&self) -> &[Adapter] {
        &self.adapters
    }
}

impl Adapter {
    /// The adapter's own id, `lsblk`.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The full id, `org.ono.compat.util-linux.lsblk`.
    #[must_use]
    pub fn full_id(&self) -> String {
        format!("{}.{}", self.pack_id, self.id)
    }

    /// The pack the adapter belongs to.
    #[must_use]
    pub fn pack_id(&self) -> &str {
        &self.pack_id
    }

    /// The pack's version, which provenance quotes as the adapter version.
    #[must_use]
    pub fn pack_version(&self) -> &str {
        &self.pack_version
    }

    /// One line about the adapter.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The executable family.
    #[must_use]
    pub fn executable(&self) -> &Executable {
        &self.executable
    }

    /// The strategy tier.
    #[must_use]
    pub fn tier(&self) -> StrategyTier {
        self.tier
    }

    /// The demands the adapter answers.
    #[must_use]
    pub fn output_demand(&self) -> &[DemandKind] {
        &self.output_demand
    }

    /// What an unsupported invocation does under an interactive demand.
    #[must_use]
    pub fn fallback(&self) -> Fallback {
        self.fallback
    }

    /// The canonical schema id.
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// The decoder.
    #[must_use]
    pub fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    /// Canonical field → mapping.
    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, FieldMap> {
        &self.fields
    }

    /// Canonical fields the adapter's invocation implies, as constants.
    #[must_use]
    pub fn literals(&self) -> &BTreeMap<String, serde_yaml_ng::Value> {
        &self.literals
    }

    /// The invocations, in contract order.
    #[must_use]
    pub fn invocations(&self) -> &[Invocation] {
        &self.invocations
    }

    /// What the adapter does not do.
    #[must_use]
    pub fn limits(&self) -> &[String] {
        &self.limits
    }

    /// The fixture directory, relative to `docs/spec/adapters/fixtures/`.
    #[must_use]
    pub fn fixtures(&self) -> &str {
        &self.fixtures
    }
}

impl Executable {
    /// The program names.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The version range, as written.
    #[must_use]
    pub fn versions(&self) -> &str {
        &self.versions
    }

    /// The version probe, if the range needs one.
    #[must_use]
    pub fn version_probe(&self) -> Option<&VersionProbe> {
        self.version_probe.as_ref()
    }
}

impl VersionProbe {
    /// The arguments after the executable.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// The regex whose first capture is the version.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

impl Decoder {
    /// The kind.
    #[must_use]
    pub fn kind(&self) -> DecoderKind {
        self.kind
    }

    /// json: the key holding the record list.
    #[must_use]
    pub fn records(&self) -> Option<&str> {
        self.records.as_deref()
    }

    /// json: the key whose children are records too.
    #[must_use]
    pub fn nested(&self) -> Option<&str> {
        self.nested.as_deref()
    }

    /// json: the key whose entries are the records, inside each element.
    #[must_use]
    pub fn children(&self) -> Option<&str> {
        self.children.as_deref()
    }

    /// lines: the field separator.
    #[must_use]
    pub fn field_separator(&self) -> Option<&str> {
        self.field_separator.as_deref()
    }

    /// lines: the record separator.
    #[must_use]
    pub fn record_separator(&self) -> Option<&str> {
        self.record_separator.as_deref()
    }

    /// lines: the decoded field names in argv order.
    #[must_use]
    pub fn columns(&self) -> Option<&[String]> {
        self.columns.as_deref()
    }

    /// lines: leading records that are a header, not data.
    #[must_use]
    pub fn header_lines(&self) -> usize {
        self.header_lines.unwrap_or(0)
    }

    /// builtin: the decoder id.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// builtin: the stability.
    #[must_use]
    pub fn stability(&self) -> Option<Stability> {
        self.stability
    }
}

impl FieldMap {
    /// The decoded field name.
    #[must_use]
    pub fn from(&self) -> &str {
        &self.from
    }

    /// A `{field}` template over the record or over each object in the list.
    #[must_use]
    pub fn template(&self) -> Option<&str> {
        self.template.as_deref()
    }

    /// Whether only the first element of the list is taken.
    #[must_use]
    pub fn takes_first(&self) -> bool {
        self.first.unwrap_or(false)
    }

    /// Whether only the last path component of the string at `from` is taken.
    #[must_use]
    pub fn takes_basename(&self) -> bool {
        self.basename.unwrap_or(false)
    }

    /// The derivation performed, if any.
    #[must_use]
    pub fn infer(&self) -> Option<Inference> {
        self.infer
    }

    /// The unit a bare number carries.
    #[must_use]
    pub fn unit(&self) -> Option<Unit> {
        self.unit
    }

    /// The separator a string is split on.
    #[must_use]
    pub fn split(&self) -> Option<&str> {
        self.split.as_deref()
    }

    /// The literal whose presence in the split list makes the field true.
    #[must_use]
    pub fn contains(&self) -> Option<&str> {
        self.contains.as_deref()
    }

    /// Literal translations applied before coercion.
    #[must_use]
    pub fn map(&self) -> Option<&BTreeMap<String, serde_yaml_ng::Value>> {
        self.map.as_ref()
    }

    /// The exactness, defaulted as the contract says.
    #[must_use]
    pub fn exactness(&self) -> Exactness {
        self.exactness.unwrap_or(if self.infer.is_some() {
            Exactness::Inferred
        } else if self.unit.is_some()
            || self.split.is_some()
            || self.contains.is_some()
            || self.map.is_some()
            || self.template.is_some()
            || self.first.is_some()
            || self.basename.is_some()
        {
            Exactness::Normalized
        } else {
            Exactness::Exact
        })
    }
}

impl Invocation {
    /// The invocation's own id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The user-facing spelling.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// What selects it.
    #[must_use]
    pub fn matcher(&self) -> &Match {
        &self.matcher
    }

    /// The plan it compiles to.
    #[must_use]
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
}

impl Match {
    /// Alternatives of positional words.
    #[must_use]
    pub fn words(&self) -> &[Vec<String>] {
        &self.words
    }

    /// Flags that pass through.
    #[must_use]
    pub fn allowed_flags(&self) -> &[String] {
        &self.flags.allow
    }

    /// Flags that take the next word and pass through.
    #[must_use]
    pub fn allowed_flags_with_value(&self) -> &[String] {
        &self.flags.allow_with_value
    }

    /// Flags that must all be present.
    #[must_use]
    pub fn required_flags(&self) -> &[String] {
        &self.flags.require
    }

    /// Whether further words may follow.
    #[must_use]
    pub fn positionals(&self) -> Positionals {
        self.positionals
    }
}

impl Plan {
    /// The exact argv, program first.
    #[must_use]
    pub fn argv(&self) -> &[String] {
        &self.argv
    }

    /// Whether user flags are appended.
    #[must_use]
    pub fn appends_user_flags(&self) -> bool {
        self.append_user_flags
    }

    /// Environment stabilisation.
    #[must_use]
    pub fn env(&self) -> &BTreeMap<String, String> {
        &self.env
    }

    /// What the child sees on stdin.
    #[must_use]
    pub fn stdin(&self) -> StdinMode {
        self.stdin
    }

    /// Whether the invocation may never end on its own (`journalctl -f`).
    #[must_use]
    pub fn is_unbounded(&self) -> bool {
        self.unbounded
    }

    /// Words appended after the user's own — `find`'s `-printf` action, which must come last.
    #[must_use]
    pub fn trailing_argv(&self) -> &[String] {
        &self.trailing_argv
    }
}

impl Fixture {
    /// Parses a fixture sidecar.
    ///
    /// # Errors
    ///
    /// The YAML error as text.
    pub fn parse(yaml: &str) -> Result<Self, String> {
        serde_yaml_ng::from_str(yaml).map_err(|error| error.to_string())
    }

    /// The user's argv that produced the bytes.
    #[must_use]
    pub fn invocation(&self) -> &[String] {
        &self.invocation
    }

    /// The tool version the bytes came from.
    #[must_use]
    pub fn tool_version(&self) -> &str {
        &self.tool_version
    }

    /// Where the bytes were captured.
    #[must_use]
    pub fn distro(&self) -> &str {
        &self.distro
    }

    /// The records the decoder must produce, when it must produce records.
    #[must_use]
    pub fn expected_records(&self) -> Option<&[BTreeMap<String, serde_yaml_ng::Value>]> {
        self.expect.records.as_deref()
    }

    /// The `adapter.*` selector the decoder must produce, when it must fail.
    #[must_use]
    pub fn expected_error(&self) -> Option<&str> {
        self.expect.error.as_deref()
    }
}

/// The first-party packs bundled with the shell.
///
/// A pack that does not parse is left out here and reported by `cargo xtask spec-check` and
/// the crate's own tests; one broken contract must not take the others down.
#[must_use]
pub fn first_party() -> &'static [AdapterPack] {
    static PACKS: OnceLock<Vec<AdapterPack>> = OnceLock::new();
    PACKS.get_or_init(|| {
        FIRST_PARTY
            .iter()
            .filter_map(|yaml| AdapterPack::parse(yaml).ok())
            .collect()
    })
}

/// Checks a parsed pack against everything the contract requires beyond its shape.
///
/// `schemas` is where `schema:` ids must be registered; `fixtures_root` is
/// `docs/spec/adapters/fixtures/`.
#[must_use]
pub fn validate(
    pack: &AdapterPack,
    schemas: &SchemaRegistry,
    fixtures_root: &Path,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let at = |detail: String| Problem {
        location: pack.id().to_owned(),
        detail,
    };

    if pack.format != "ono-adapter-pack/1" {
        problems.push(at(format!(
            "`format` is `{}`; this contract is `ono-adapter-pack/1`",
            pack.format
        )));
    }
    let id = pack.id();
    let segments: Vec<&str> = id.split('.').collect();
    let well_formed = segments.len() >= 2
        && segments.iter().all(|segment| {
            let mut chars = segment.chars();
            chars.next().is_some_and(|c| c.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if !well_formed {
        problems.push(at(format!(
            "`package.id` `{id}` is not a lowercase reverse-DNS name (spec §31.5)"
        )));
    }
    match pack.tier() {
        Tier::FirstParty if !id.starts_with("org.ono.compat.") => problems.push(at(format!(
            "a first-party pack is `org.ono.compat.<name>`, not `{id}` (ADR-0055)"
        ))),
        Tier::FirstParty => {}
        _ if id.starts_with("ono.") || id.starts_with("org.ono.") => problems.push(at(format!(
            "`{id}` claims the Ono namespace, which only first-party packs may (spec §31.5)"
        ))),
        _ => {}
    }
    if !id.starts_with(&format!("{}.", pack.package.publisher)) {
        problems.push(at(format!(
            "`package.id` `{id}` does not begin with `package.publisher` `{}`",
            pack.package.publisher
        )));
    }
    if pack.package.name.trim().is_empty() {
        problems.push(at("`package.name` is empty".to_owned()));
    }
    if crate::version::Version::parse(&pack.package.version).is_none() {
        problems.push(at(format!(
            "`package.version` `{}` is not a dotted numeric version",
            pack.package.version
        )));
    }
    if !pack.roles.iter().any(|role| role == "adapter") {
        problems.push(at(
            "`roles` must contain `adapter` (spec v0.3 §1.44)".to_owned()
        ));
    }
    if pack.capabilities.process_exec.argv_policy != "declared-invocations-only" {
        problems.push(at(format!(
            "`process.exec.argv_policy` is `{}`; the only policy is `declared-invocations-only` \
             (spec v0.3 §1.22)",
            pack.capabilities.process_exec.argv_policy
        )));
    }
    if pack.adapters.is_empty() {
        problems.push(at("the pack declares no adapters".to_owned()));
    }

    let granted: BTreeSet<&str> = pack
        .executables()
        .iter()
        .map(|executable| basename(executable))
        .collect();
    let mut seen_adapters = BTreeSet::new();
    for adapter in pack.adapters() {
        if !seen_adapters.insert(adapter.id()) {
            problems.push(at(format!(
                "adapter id `{}` is declared twice",
                adapter.id()
            )));
        }
        validate_adapter(adapter, &granted, schemas, fixtures_root, &mut problems);
    }
    problems
}

fn validate_adapter(
    adapter: &Adapter,
    granted: &BTreeSet<&str>,
    schemas: &SchemaRegistry,
    fixtures_root: &Path,
    problems: &mut Vec<Problem>,
) {
    let location = format!("{}/{}", adapter.pack_id(), adapter.id());
    let mut report = |detail: String| {
        problems.push(Problem {
            location: location.clone(),
            detail,
        });
    };

    if !is_kebab(adapter.id()) {
        report(format!("adapter id `{}` is not kebab-case", adapter.id()));
    }
    if adapter.summary().trim().is_empty() {
        report("`summary` is empty".to_owned());
    }

    // The executable family and its grant (spec v0.3 §1.22).
    if adapter.executable().names().is_empty() {
        report("`executable.names` is empty".to_owned());
    }
    for name in adapter.executable().names() {
        if !granted.contains(basename(name)) {
            report(format!(
                "executable `{name}` is not in the pack's `process.exec` grant (spec v0.3 §1.22)"
            ));
        }
    }
    match VersionRange::parse(adapter.executable().versions()) {
        None => report(format!(
            "`executable.versions` `{}` is not `any`, `>=X.Y` or `>=X.Y <Z`",
            adapter.executable().versions()
        )),
        Some(range) if !range.is_any() && adapter.executable().version_probe().is_none() => {
            report(
                "`executable.versions` constrains the version but no `version_probe` is declared \
                 (spec v0.3 §1.46)"
                    .to_owned(),
            );
        }
        Some(_) => {}
    }
    if let Some(probe) = adapter.executable().version_probe() {
        if probe.argv.is_empty() {
            report("`version_probe.argv` is empty".to_owned());
        }
        match regex::Regex::new(&probe.pattern) {
            Err(error) => report(format!("`version_probe.pattern` does not compile: {error}")),
            Ok(regex) if regex.captures_len() < 2 => report(
                "`version_probe.pattern` has no capture group, so it cannot yield a version \
                 (spec v0.3 §1.46)"
                    .to_owned(),
            ),
            Ok(_) => {}
        }
        if probe.cache != "executable-identity" {
            report(format!(
                "`version_probe.cache` is `{}`; the only cache key is `executable-identity`",
                probe.cache
            ));
        }
    }

    if adapter.output_demand().is_empty() {
        report(
            "`output_demand` is empty; an adapter that answers no demand adapts nothing".to_owned(),
        );
    }

    // The canonical schema and the field map (spec v0.3 §1.11).
    let schema = adapter
        .schema()
        .parse::<SchemaId>()
        .ok()
        .and_then(|id| schemas.get(&id));
    match &schema {
        None => report(format!(
            "`schema` `{}` is not a registered canonical schema (spec v0.3 §1.11)",
            adapter.schema()
        )),
        Some(schema) => {
            for target in adapter.literals().keys() {
                if schema.field(target).is_none() {
                    report(format!(
                        "`literals.{target}` names a field `{}` does not have",
                        adapter.schema()
                    ));
                }
            }
            for (target, map) in adapter.fields() {
                if schema.field(target).is_none() {
                    report(format!(
                        "`fields.{target}` names a field `{}` does not have",
                        adapter.schema()
                    ));
                }
                if map.from().trim().is_empty() && map.template().is_none() {
                    report(format!("`fields.{target}.from` is empty"));
                }
                if let Some(template) = map.template()
                    && !template.contains('{')
                {
                    report(format!(
                        "`fields.{target}.template` names no `{{field}}` placeholder, so it derives nothing"
                    ));
                }
                if map.contains().is_some() && map.split().is_none() {
                    report(format!(
                        "`fields.{target}.contains` needs `split` to say what it searches"
                    ));
                }
            }
        }
    }

    // The decoder (spec v0.3 §1.9).
    let decoder = adapter.decoder();
    match decoder.kind() {
        DecoderKind::Json => {
            if decoder.field_separator().is_some()
                || decoder.record_separator().is_some()
                || decoder.columns().is_some()
                || decoder.id().is_some()
                || decoder.stability().is_some()
            {
                report("a `json` decoder takes only `records`, `nested` and `children`".to_owned());
            }
            if decoder.nested().is_some() && decoder.children().is_some() {
                report("a `json` decoder is either `nested` or `children`, not both".to_owned());
            }
        }
        DecoderKind::Jsonl => {
            if decoder.records().is_some()
                || decoder.nested().is_some()
                || decoder.children().is_some()
                || decoder.field_separator().is_some()
                || decoder.record_separator().is_some()
                || decoder.columns().is_some()
                || decoder.id().is_some()
                || decoder.stability().is_some()
            {
                report("a `jsonl` decoder takes no options: every line is one record".to_owned());
            }
        }
        DecoderKind::Properties => {
            if decoder.field_separator().is_none() || decoder.record_separator().is_none() {
                report(
                    "a `properties` decoder needs `field_separator` (between key and value) and \
                     `record_separator` (between records)"
                        .to_owned(),
                );
            }
            if decoder.columns().is_some() || decoder.records().is_some() || decoder.id().is_some()
            {
                report("a `properties` decoder takes no `columns`, `records` or `id`".to_owned());
            }
        }
        DecoderKind::Lines => {
            if decoder.field_separator().is_none()
                || decoder.record_separator().is_none()
                || decoder.columns().is_none_or(<[String]>::is_empty)
            {
                report(
                    "a `lines` decoder needs `field_separator`, `record_separator` and `columns`"
                        .to_owned(),
                );
            }
            if decoder.records().is_some() || decoder.nested().is_some() || decoder.id().is_some() {
                report("a `lines` decoder takes no `records`, `nested` or `id`".to_owned());
            }
            if decoder.header_lines() > 0
                && decoder
                    .record_separator()
                    .is_some_and(|separator| separator != "\\n" && separator != "\n")
            {
                report("`header_lines` only makes sense for newline-separated records".to_owned());
            }
        }
        DecoderKind::Builtin => {
            match decoder.id() {
                None => report("a `builtin` decoder needs an `id`".to_owned()),
                Some(id) if !BUILTIN_DECODERS.contains(&id) => report(format!(
                    "builtin decoder `{id}` does not exist in this binary"
                )),
                Some(_) => {}
            }
            if decoder.stability().is_none() {
                report("a `builtin` decoder needs a `stability`".to_owned());
            }
        }
    }
    if adapter.tier() == StrategyTier::C && decoder.kind() != DecoderKind::Builtin {
        report(
            "a tier C adapter parses human output, which is code: its decoder must be `builtin` \
             (spec v0.3 §1.9)"
                .to_owned(),
        );
    }

    // Invocations and their plans (spec v0.3 §1.7, §1.15).
    if adapter.invocations().is_empty() {
        report("the adapter declares no invocations".to_owned());
    }
    let mut seen = BTreeSet::new();
    for invocation in adapter.invocations() {
        if !seen.insert(invocation.id()) {
            report(format!(
                "invocation id `{}` is declared twice",
                invocation.id()
            ));
        }
        if invocation.matcher().words().is_empty() {
            report(format!(
                "invocation `{}` has no `match.words`; use `[[]]` for the bare program",
                invocation.id()
            ));
        }
        match invocation.plan().argv().first() {
            None => report(format!(
                "invocation `{}` has an empty `plan.argv`",
                invocation.id()
            )),
            Some(program)
                if !adapter
                    .executable()
                    .names()
                    .iter()
                    .any(|name| basename(name) == basename(program)) =>
            {
                report(format!(
                    "invocation `{}` plans to run `{program}`, which is not one of the adapter's \
                     executables",
                    invocation.id()
                ));
            }
            Some(_) => {}
        }
    }

    // Fixtures (spec v0.3 §1.47).
    let directory = fixtures_root.join(adapter.fixtures());
    if !directory.is_dir() {
        report(format!(
            "fixture directory `{}` does not exist under docs/spec/adapters/fixtures/ (spec v0.3 §1.47)",
            adapter.fixtures()
        ));
        return;
    }
    let mut outputs = 0;
    let mut entries: Vec<_> = std::fs::read_dir(&directory)
        .map(|entries| entries.filter_map(Result::ok).collect())
        .unwrap_or_default();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "out") {
            outputs += 1;
            let sidecar = path.with_extension("yaml");
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            match std::fs::read_to_string(&sidecar)
                .map_err(|e| e.to_string())
                .and_then(|t| Fixture::parse(&t))
            {
                Err(error) => report(format!("fixture `{name}` has no readable sidecar: {error}")),
                Ok(fixture) => {
                    if fixture.expected_records().is_some() == fixture.expected_error().is_some() {
                        report(format!(
                            "fixture `{name}` must expect exactly one of `records` or `error`"
                        ));
                    }
                    if let Some(error) = fixture.expected_error()
                        && ono_core::ErrorCode::from_name(error)
                            .is_none_or(|c| !c.name().starts_with("adapter."))
                    {
                        report(format!(
                            "fixture `{name}` expects `{error}`, which is not an `adapter.*` error"
                        ));
                    }
                    match fixture.invocation().first() {
                        Some(program)
                            if adapter
                                .executable()
                                .names()
                                .iter()
                                .any(|n| basename(n) == basename(program)) => {}
                        _ => report(format!(
                            "fixture `{name}` was produced by something other than the adapter's \
                             executable"
                        )),
                    }
                }
            }
        }
    }
    if outputs == 0 {
        report(format!(
            "fixture directory `{}` holds no `.out` file (spec v0.3 §1.47)",
            adapter.fixtures()
        ));
    }
}

fn basename(program: &str) -> &str {
    program.rsplit('/').next().unwrap_or(program)
}

fn is_kebab(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}
