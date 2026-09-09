//! The messages the supervisor and a plugin exchange (`docs/contracts/kuang/protocol.v1.yaml`).
//!
//! Everything on the wire is one of three envelopes: the plugin's opening [`Hello`], a
//! [`Envelope::Request`] carrying a call id from the protocol contract, or a
//! [`Envelope::Response`] answering one. Values cross as the tagged JSON encoding of
//! `ono-value` — the lossless codec of `ono_value::to_json`/`from_json` — so a `ByteSize` does
//! not arrive as a bare number with its unit gone (protocol invariant `typed-units`; the binary
//! encoding of spec §31.61 is a later performance increment, ADR-0040).
//!
//! Flow control is pull-based in both directions (ADR-0022 §8): a plugin emits only against
//! credit the host granted, and the host grants credit only as its consumer takes values.

use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as Json};

use crate::{KuangError, KuangErrorCode, PluginContract, WireError};

/// The call ids of `docs/contracts/kuang/protocol.v1.yaml`, as they appear in `Request::method`.
pub mod method {
    /// Host → plugin: deliver the negotiated contract (spec §31.63).
    pub const LIFECYCLE_INIT: &str = "lifecycle.init";
    /// Host → plugin: drain and stop within the deadline.
    pub const LIFECYCLE_SHUTDOWN: &str = "lifecycle.shutdown";
    /// Host → plugin: run a contributed command (spec §31.22, §31.29).
    pub const COMMAND_INVOKE: &str = "command.invoke";
    /// Host → plugin: answer a query against a contributed target (spec §31.23; ADR-0040 —
    /// `protocol.v1.yaml` omitted the call, the provider conformance test forced it).
    pub const PROVIDER_QUERY: &str = "provider.query";
    /// Host → plugin: grant more credit on one of the plugin's output streams.
    pub const STREAM_DEMAND: &str = "stream.demand";
    /// Host → plugin: a stream was cancelled. Delivered, not inferred (spec §31.14).
    pub const STREAM_CANCEL: &str = "stream.cancel";
    /// Host → plugin: health check (spec §31.35).
    pub const HEALTH_PROBE: &str = "health.probe";
    /// Host → plugin: a view was mounted at a size (spec §31.28).
    pub const VIEW_MOUNT: &str = "view.mount";
    /// Host → plugin: a key, a resize, focus, blur or cancellation for a view (spec §31.28).
    pub const VIEW_EVENT: &str = "view.event";
    /// Host → plugin: a view was torn down (spec §31.28).
    pub const VIEW_UNMOUNT: &str = "view.unmount";

    /// Plugin → host: emit values against granted credit.
    pub const STREAMS_EMIT: &str = "streams.emit";
    /// Plugin → host: close an output stream, normally or with a terminal error.
    pub const STREAMS_CLOSE: &str = "streams.close";
    /// Plugin → host: check a grant without prompting (spec §31.61).
    pub const CAPABILITIES_CHECK: &str = "capabilities.check";
    /// Plugin → host: runtime capability request, against an explicit user action (spec §31.17).
    pub const CAPABILITIES_REQUEST: &str = "capabilities.request";
    /// Plugin → host: structured log record (spec §31.33).
    pub const AUDIT_LOG: &str = "audit.log";
    /// Plugin → host: add a security-relevant event to the audit trail (spec §31.37).
    pub const AUDIT_EVENT: &str = "audit.event";
    /// Plugin → host: read a value from the package's own store (spec §31.31).
    pub const STATE_GET: &str = "state.get";
    /// Plugin → host: write a value into the package's own store.
    pub const STATE_SET: &str = "state.set";
    /// Plugin → host: remove a key from the package's own store.
    pub const STATE_DELETE: &str = "state.delete";
    /// Plugin → host: wall-clock time. Virtual under the test host (spec §31.73).
    pub const CLOCK_NOW: &str = "clock.now";
    /// Plugin → host: read file bytes under the granted `paths` scope.
    pub const FILESYSTEM_READ: &str = "filesystem.read";
    /// Plugin → host: pull values from a stream the host produces (spec §31.15's credit).
    pub const STREAMS_NEXT: &str = "streams.next";
    /// Plugin → host: cancel a stream in either direction.
    pub const STREAMS_CANCEL: &str = "streams.cancel";
    /// Plugin → host: the context stack, and nothing beyond it (spec §31.12).
    pub const CONTEXT_GET: &str = "context.get";
    /// Plugin → host: one registered schema (spec §31.12, §31.64).
    pub const SCHEMAS_GET: &str = "schemas.get";
    /// Plugin → host: the registered schemas, as a stream (spec §31.12, §31.64).
    pub const SCHEMAS_LIST: &str = "schemas.list";
    /// Plugin → host: one object by identity (spec §31.13).
    pub const OBJECTS_GET: &str = "objects.get";
    /// Plugin → host: a finite stream of records for a query (spec §31.13).
    pub const OBJECTS_QUERY: &str = "objects.query";
    /// Plugin → host: the object references a selector matches (spec §31.13).
    pub const OBJECTS_RESOLVE: &str = "objects.resolve";
    /// Plugin → host: a bounded stream of snapshot events (spec §31.14).
    pub const OBJECTS_SNAPSHOT: &str = "objects.snapshot";
    /// Plugin → host: an unbounded stream of changes (spec §31.14).
    pub const OBJECTS_SUBSCRIBE: &str = "objects.subscribe";
    /// Plugin → host: a snapshot followed by changes (spec §31.14).
    pub const OBJECTS_WATCH: &str = "objects.watch";
    /// Plugin → host: the edges around an object (spec §31.26).
    pub const RELATIONS_QUERY: &str = "relations.query";
    /// Plugin → host: edges the package asserts, attributed to it (spec §31.26).
    pub const RELATIONS_CONTRIBUTE: &str = "relations.contribute";
    /// Plugin → host: bounded history, redacted (spec §31.12).
    pub const HISTORY_QUERY: &str = "history.query";
    /// Plugin → host: a history entry, attributed to the package (spec §31.12).
    pub const HISTORY_APPEND: &str = "history.append";
    /// Plugin → host: a signal to a process, within the granted list (spec §31.12).
    pub const PROCESS_SIGNAL: &str = "process.signal";
    /// Plugin → host: open a contributed view (spec §31.27).
    pub const VIEWS_OPEN: &str = "views.open";
    /// Plugin → host: submit a view tree (spec §31.27).
    pub const VIEWS_SUBMIT: &str = "views.submit";
    /// Plugin → host: close a view (spec §31.28).
    pub const VIEWS_CLOSE: &str = "views.close";
    /// Plugin → host: run a program within the granted `programs` scope (spec §31.12).
    pub const PROCESS_EXEC: &str = "process.exec";
    /// Plugin → host: a brokered outbound connection (spec §31.21).
    pub const NETWORK_CONNECT: &str = "network.connect";
    /// Plugin → host: a brokered request the host performs (spec §31.21).
    pub const NETWORK_REQUEST: &str = "network.request";
    /// Plugin → host: a brokered listener (spec §31.21).
    pub const NETWORK_LISTEN: &str = "network.listen";
    /// Plugin → host: close a brokered connection or listener.
    pub const NETWORK_CLOSE: &str = "network.close";
    /// Plugin → host: an opaque secret handle by name (spec §31.20).
    pub const SECRETS_REQUEST: &str = "secrets.request";
    /// Plugin → host: invalidate a secret handle (spec §31.20).
    pub const SECRETS_RELEASE: &str = "secrets.release";
    /// Plugin → host: the model providers this package may use (spec §31.43).
    pub const MODELS_LIST: &str = "models.list";
    /// Plugin → host: operator-approved inference through the model broker (spec §31.43).
    pub const MODELS_INFER: &str = "models.infer";

    /// Plugin → host: whether the session is historical, and at which instant (v0.5 §30.7, §4).
    pub const TEMPORAL_CONTEXT: &str = "temporal.context";
    /// Plugin → host: recorded events within the granted window (v0.5 §30.7, §11).
    ///
    /// A separate call from [`OBJECTS_QUERY`] on purpose: §30.7 is explicit that "a plugin with
    /// current object read permission does not automatically receive historical access", and two
    /// calls behind two capabilities is what makes that true rather than asserted.
    pub const TEMPORAL_QUERY: &str = "temporal.query";
    /// Plugin → host: the evidence and coverage behind a temporal claim (v0.5 §7.3, §30.7).
    pub const TEMPORAL_EVIDENCE: &str = "temporal.evidence";
    /// Plugin → host: canonical temporal events, attributed to the package by the host
    /// (v0.5 §37.3).
    pub const TEMPORAL_CONTRIBUTE_EVENTS: &str = "temporal.contribute.events";
    /// Plugin → host: causal links from a namespaced rule the package registered (v0.5 §37.4).
    pub const TEMPORAL_CONTRIBUTE_CAUSALITY: &str = "temporal.contribute.causality";
    /// Plugin → host: start or stop the persistent history recorder (v0.5 §10.3, §30.7).
    pub const TEMPORAL_RECORDER: &str = "temporal.recorder";

    /// Plugin → host: read a change plan, its actions and its computed impact (v0.6 §48.3).
    ///
    /// §48.4 is the reason this is its own call behind its own capability: a package that can
    /// describe impact reads here and executes nowhere.
    pub const CHANGE_PLAN_READ: &str = "change.plan.read";
    /// Plugin → host: contribute actions, effects, impact edges and risk findings to a plan
    /// being resolved (v0.6 §48.2, §48.3).
    pub const CHANGE_PLAN_CONTRIBUTE: &str = "change.plan.contribute";
    /// Plugin → host: report the recovery candidates this package found (v0.6 §12.1, §48.5).
    pub const RECOVERY_DISCOVER: &str = "recovery.discover";
    /// Plugin → host: create the asset a protection action proposed (v0.6 §4.5, §12.1).
    pub const RECOVERY_PREPARE: &str = "recovery.prepare";
    /// Plugin → host: report what checking an asset against §11.4's list found.
    pub const RECOVERY_VALIDATE: &str = "recovery.validate";
    /// Plugin → host: put state back from an asset (v0.6 §12.1, §24).
    pub const RECOVERY_RESTORE: &str = "recovery.restore";
    /// Plugin → host: remove a recovery asset (v0.6 §37).
    pub const RECOVERY_CLEANUP: &str = "recovery.cleanup";
    /// Plugin → host: what an asset costs to create and to keep (v0.6 §38).
    pub const RECOVERY_ESTIMATE_COST: &str = "recovery.estimate_cost";
    /// Plugin → host: pause an application so a capture is application-consistent (v0.6 §39.3).
    pub const RECOVERY_QUIESCE: &str = "recovery.quiesce";
    /// Plugin → host: release a quiesce window (v0.6 §18.4, §39.3).
    ///
    /// The other half of [`RECOVERY_QUIESCE`] and behind the same capability: §18.4 requires the
    /// application to be resumed when creation fails, so a package that can pause can always
    /// resume, whatever else a policy decided afterwards.
    pub const RECOVERY_RESUME: &str = "recovery.resume";
    /// Plugin → host: the result of observing one verification contract (v0.6 §23, §25.1).
    pub const VERIFICATION_OBSERVE: &str = "verification.observe";
}

/// One frame's payload: the opening hello, a call, or an answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Envelope {
    /// The plugin's first frame, and only the plugin's (spec §31.63).
    Hello(Hello),
    /// A call in either direction.
    Request {
        /// The caller's sequence number, unique per direction.
        seq: u64,
        /// A call id from [`method`].
        method: String,
        /// The call's parameters, per the protocol contract.
        params: Json,
    },
    /// The answer to a request, carrying exactly one of `result` and `error`.
    Response {
        /// The sequence number of the request being answered.
        seq: u64,
        /// The successful result.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<Json>,
        /// The structured failure.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<WireError>,
    },
}

/// The plugin's opening frame: who it is and what it brings (spec §31.63).
///
/// The host has already read and validated the package manifest before spawning anything —
/// manifest before code, spec §31.89 rule 1 — so the hello carries the identity to cross-check
/// and the contribution documents to validate and register, not authority.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hello {
    /// The package format the plugin was built against. Must be `kuang-package/1`.
    pub format: String,
    /// The package id. Must match the manifest the host validated.
    pub package: String,
    /// The package version. Must match the manifest.
    pub version: String,
    /// The host API range the plugin speaks, e.g. `>=11.1 <12`.
    pub kuang_api: String,
    /// The contribution documents, validated before registration (spec §31.22).
    #[serde(default)]
    pub contributions: ContributionSet,
}

/// The contribution documents a plugin surfaces at handshake.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ContributionSet {
    /// Views and lenses (spec §31.27).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub views: Vec<ViewContribution>,
    /// Contributed commands (spec §31.22).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commands: Vec<CommandContribution>,
    /// Contributed targets (spec §31.23).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<TargetContribution>,
    /// Contributed schemas (spec §31.23).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<SchemaContribution>,
    /// Contributed temporal event sources and historical query providers (v0.5 §37.2, §37.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub temporal_sources: Vec<TemporalSourceContribution>,
    /// Contributed causal and correlation rules (v0.5 §37.2, §37.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_rules: Vec<CausalRuleContribution>,
    /// Contributed recovery providers (v0.6 §48.2's `RecoveryProvider`, §12.1, §48.5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recovery_providers: Vec<RecoveryProviderContribution>,
    /// Contributed impact providers (v0.6 §48.2's `ImpactProvider`, §9.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub impact_providers: Vec<ImpactProviderContribution>,
    /// Contributed verification providers (v0.6 §48.2's `VerificationProvider`, §25.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verification_providers: Vec<VerificationProviderContribution>,
    /// Contributed risk rules (v0.6 §48.2's `RiskRule`, §19.1, §19.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risk_rules: Vec<RiskRuleContribution>,
    /// Contributed plan views (v0.6 §48.2's `ChangeView`, §45).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub change_views: Vec<ChangeViewContribution>,
}

/// A contributed temporal source: an event source, or a provider of historical state (§37.5).
///
/// §37.5 lets a package expose the past of an external system — a metrics backend, a tracing
/// store, a container runtime archive, an audit log — and requires it to "map data into canonical
/// Ono objects/events and expose coverage/provenance". Both halves are declared here, before any
/// package code runs, because a source whose coverage is discovered by reading it is a source
/// that has already been believed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemporalSourceContribution {
    /// `<package.id>.temporal-source.<kebab-name>`.
    pub id: String,
    /// One line, for `get temporal-source` and `help`.
    pub summary: String,
    /// The schema of the objects the source maps its data into. A source that cannot name one
    /// has not mapped anything into canonical Ono objects (§37.5).
    pub schema: String,
    /// The canonical event kinds it produces, from the closed list of v0.5 §6.1.
    ///
    /// A package may refine a kind through an event's `subtype`; the top-level kind is always
    /// one Ono owns, because §37.1 keeps identity and evidence classes with the host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// Whether the answer ends by itself (§37.5, ADR-0588).
    ///
    /// A historical query provider is [`Answer::Bounded`]; a temporal event source is
    /// [`Answer::Unbounded`]. The host has to know which before the first record, because a
    /// bounded answer is collected and an unbounded one becomes a live stream.
    #[serde(default, skip_serializing_if = "Answer::is_default")]
    pub answer: Answer,
    /// What the source covers, in prose the package stands behind (§37.5, §8).
    pub coverage: String,
    /// How far back the external system keeps material, where the package states a bound.
    ///
    /// `None` means the package does not say, which is not the same as "forever" (§21.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_history: Option<String>,
}

/// A contributed causal or correlation rule (§37.4, §15.8).
///
/// §37.4: "Third-party causal rules MUST be namespaced and MUST identify their source." Both are
/// structural here — the id carries the publisher's namespace and `ono.*` is refused, and the
/// host attributes every link the rule emits to the package rather than taking the package's
/// word for where it came from. The declared `strength` is a ceiling the host lowers and never
/// raises: §37.4 caps a plugin's causal strength at `asserted` unless the host contract trusts
/// the package as authoritative for a domain.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CausalRuleContribution {
    /// The rule id, namespaced to the publisher — `dev.example.packet-eye.retransmit-to-drop`.
    pub rule_id: String,
    /// The relation class it emits, from the five of v0.5 §15.1.
    pub relation: String,
    /// The evidence strength its links carry, from the five of v0.5 §7.2.
    pub strength: String,
    /// One line, what the rule claims and why.
    pub summary: String,
    /// The canonical event kinds it reads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<String>,
    /// What must be *equal* for the rule to fire, in prose (§15.8).
    ///
    /// §15.2: "temporal proximity is insufficient". A rule whose only constraint is a time
    /// window is a correlation rule, and the host holds it to that.
    pub identity_constraints: String,
}

/// A contributed command, in the same metadata shape core commands use
/// (`docs/contracts/kuang/contributions.v1.yaml`). `provider` and origin are the host's to set at
/// registration; they are deliberately not wire fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommandContribution {
    /// `<package.id>.command.<kebab-name>`.
    pub id: String,
    /// An existing verb wherever the semantics allow it (spec §31.22).
    pub verb: String,
    /// A core target or one this package contributes.
    pub target: String,
    /// One line, for `help` and completion.
    pub summary: String,
    /// The input type, e.g. `stream<ono.socket/1>`. `None` for a command taking no input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    /// The output type, e.g. `stream<dev.example.echo.item/1>`. Validated on every value.
    pub output: String,
    /// The KUANG/11 capabilities the command needs, checked at invocation.
    #[serde(default)]
    pub capabilities: Vec<String>,
    /// The argument mode from ADR-0009's table.
    pub argument_mode: String,
    /// The positional arguments, as a core command declares them
    /// (`docs/contracts/kuang/contributions.v1.yaml` → `command.selectors`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selectors: Vec<ParameterContribution>,
    /// The named arguments, as a core command declares them
    /// (`docs/contracts/kuang/contributions.v1.yaml` → `command.options`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ParameterContribution>,
    /// The risk level, required for a mutating command (spec §31.75).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<String>,
    /// Documented examples. Each must parse and run under the test host (spec §31.22, §50).
    #[serde(default)]
    pub examples: Vec<String>,
    /// What this command does to the system a provider fronts, in the shape the generic
    /// provider contract asks an action to declare before the host runs any provider code
    /// (`docs/architecture/external-system-provider.md` §21.1; ADR-0595). Absent for a
    /// command that is not a provider action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ActionContribution>,
}

/// A provider action's safety and type contract (`docs/architecture/external-system-provider.md`
/// §21.1, §21.6, §22.2; ADR-0595).
///
/// The generic contract asks an action to specify its identity, accepted targets, parameters,
/// required capabilities, whether it mutates, its idempotency, its result, its verification and
/// its prospective effects. Identity, parameters and capabilities are the command's own fields;
/// this is the rest, declared where the host reads it before any package code runs and validated
/// at load: an action that says it mutates must carry a risk and a mutating capability.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ActionContribution {
    /// The schema ids of the objects this action accepts as its target, or `["*"]` for any
    /// object the package answers for. Each must resolve at load like a target's schema.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    /// Whether the action changes state in the external system. `true` requires `risk` to be
    /// `mutate` or `destructive` and at least one declared capability of that risk.
    #[serde(default)]
    pub mutates: bool,
    /// What repeating the action does.
    #[serde(default)]
    pub idempotency: Idempotency,
    /// The type of the result stream where it differs from `output`; validated like `output`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// How the outcome is verified after the system accepted the action, in prose the package
    /// stands behind — or `None`, which is §21.6's "explicit lack thereof" and is shown as such.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<String>,
    /// The classes of prospective effect the action may have (§22.2): what a plan of it should
    /// warn about — `restarts-workload`, `deletes-data`, `changes-routing`.
    ///
    /// A loose vocabulary, kept working unchanged: v0.6 §0.1 leaves earlier specifications
    /// authoritative for what they define, so a package written against the provider contract
    /// alone still says what it always said. What it cannot do is enter v0.6's coverage
    /// algorithm, which is what [`ActionContribution::effect_classes`] is for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    /// The same effects in the shape v0.6's coverage algorithm reads (§8.1, §8.2, Appendix A.1).
    ///
    /// Appendix A.5 computes protection per domain and §8.1 makes confidence a lattice with no
    /// operation that strengthens, so an effect that names neither cannot be placed in the
    /// coverage matrix at all — it can only be printed. Declaring the richer form is what lets a
    /// contributed action's consequences be counted rather than merely displayed.
    ///
    /// Optional, and empty by default: a package that declares only [`ActionContribution::effects`]
    /// loads exactly as it did before.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effect_classes: Vec<EffectClassContribution>,
}

/// One prospective effect of a contributed action, in v0.6's own vocabulary (§8.1, §8.2,
/// Appendix A.1).
///
/// §10.1 forbids a plan carrying a single `reversible: true/false` flag, and Appendix A.5 builds
/// the answer per domain instead. That is only possible when each effect says which domain it
/// acts in and how strongly Ono may assert it, which is what this shape is: the five facts the
/// coverage algorithm needs, declared before any package code runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectClassContribution {
    /// The mutation domain, from Appendix A.1 — `filesystem-persistent`, `process-runtime`,
    /// `external-side-effect`. Appendix A.7 caps a plan at partially protected while an
    /// `unknown` domain is present, so naming one honestly costs the package nothing it had.
    pub domain: String,
    /// What the effect does to its object, from §8.2 — `create`, `modify`, `remove`, `replace`,
    /// `interrupt`, `emit`, `unknown`.
    pub kind: String,
    /// How strongly Ono may assert the effect will occur, from §8.1 — `guaranteed`, `expected`,
    /// `possible`, `unknown`. §2.4 forbids promoting `unknown` silently, and there is no host
    /// operation that raises a declared confidence (§8.1).
    pub confidence: String,
    /// Why the package believes it, in one sentence a reader can weigh. §9.5 makes an impact
    /// claim that cannot be explained an impact claim that should not be shown.
    pub explanation: String,
    /// Whether nothing can undo it (§2.13, §35.2). An emitted outward call is irreversible on
    /// its own terms whatever a recovery asset holds.
    #[serde(default)]
    pub irreversible: bool,
    /// The inverse action that restores an acceptable semantic state, where one exists (§27.4).
    /// §27.4 forbids calling this rollback, and `None` means the package offers none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<String>,
}

/// What repeating a provider action does (`docs/architecture/external-system-provider.md` §19.4,
/// §20.3, §21.1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Idempotency {
    /// Repeating it leaves the system as the first run left it.
    Idempotent,
    /// Repeating it is safe only under a precondition the action carries — a resource version,
    /// a generation, a uid.
    ConditionallyIdempotent,
    /// Repeating it may duplicate the effect. Never retried on the package's behalf.
    NotIdempotent,
    /// The package did not say, which is not the same as saying it is safe (spec §10.5).
    #[default]
    Unknown,
}

/// One declared argument of a contribution, in the vocabulary `docs/contracts/commands/*.yaml`
/// uses for a core command's `selectors` and `options` (ADR-0012 §7, ADR-0587).
///
/// A package that does not declare its arguments is not refused — its words still reach it, as
/// they did before there was anywhere to declare them. What a declaration buys is everything the
/// host can only do when it knows the argument exists: a help line, a completion candidate, a
/// declared type, and a default the host applies when the user says nothing. For a package that
/// changes an external system, the last of those is the difference between a safe default the
/// shell guarantees and one every handler has to remember.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParameterContribution {
    /// The name, without the `--` an option is written with.
    pub name: String,
    /// The declared type, e.g. `int`, `bool`, `string`, `duration`, `list<string>`.
    #[serde(rename = "type")]
    pub declared_type: String,
    /// One line, what the argument is for. Shown by `help` and beside a completion candidate.
    pub doc: String,
    /// Whether it may be written more than once.
    #[serde(default)]
    pub repeatable: bool,
    /// Whether the option may be written without its value (ADR-0144).
    #[serde(default)]
    pub optional_value: bool,
    /// The value the host supplies when the argument is absent, written as the registry writes
    /// it. `None` means the argument simply does not arrive, which is different from arriving
    /// as zero or false (spec §10.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Json>,
}

impl ParameterContribution {
    /// The declared default as the text the registry vocabulary coerces, or `None`.
    #[must_use]
    pub fn default_text(&self) -> Option<String> {
        match self.default.as_ref()? {
            Json::String(text) => Some(text.clone()),
            Json::Null => None,
            other => Some(other.to_string()),
        }
    }
}

/// The document a `contributions.commands` path names (spec §31.22, §31.68).
///
/// A package declares its commands twice over, in the same shape: once in a document beside its
/// manifest, so the host can register a registry placeholder without starting anything, and once
/// across the handshake, when the instance actually loads. One shape means the two cannot
/// disagree about what the package contributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandDocument {
    /// The commands the document declares.
    pub commands: Vec<CommandContribution>,
}

impl CommandDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        serde_yaml_ng::from_str(text).map_err(|error| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("a contributed command document does not read: {error}"),
            )
            .with_help(
                "the document is a `commands:` list of the contribution shape of \
                 `docs/contracts/kuang/contributions.v1.yaml`",
            )
        })
    }
}

/// The document a `contributions.targets` path names (spec §31.23, §31.68).
///
/// The target half of what a package declares twice over. A command document says what a package
/// can be *asked to do*; this says what nouns it *answers for*, and the difference decides how an
/// invocation is routed — a contributed command is invoked, a contributed target is queried
/// through the provider path, so that its records carry the declared schema and the host's
/// provenance rather than whatever a command chose to emit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetDocument {
    /// The targets the document declares.
    pub targets: Vec<TargetContribution>,
}

impl TargetDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        serde_yaml_ng::from_str(text).map_err(|error| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("a contributed target document does not read: {error}"),
            )
            .with_help(
                "the document is a `targets:` list of the contribution shape of \
                 `docs/contracts/kuang/contributions.v1.yaml`",
            )
        })
    }
}

/// The document a `contributions.temporal_sources` path names (v0.5 §37.2, §37.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemporalSourceDocument {
    /// The temporal sources the document declares.
    pub temporal_sources: Vec<TemporalSourceContribution>,
}

impl TemporalSourceDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        serde_yaml_ng::from_str(text).map_err(|error| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("a contributed temporal source document does not read: {error}"),
            )
            .with_help(
                "the document is a `temporal_sources:` list of the `temporal_source` shape of \
                 `docs/contracts/kuang/contributions.v1.yaml`",
            )
        })
    }
}

/// The document a `contributions.causal_rules` path names (v0.5 §37.2, §37.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CausalRuleDocument {
    /// The causal and correlation rules the document declares.
    pub causal_rules: Vec<CausalRuleContribution>,
}

impl CausalRuleDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        serde_yaml_ng::from_str(text).map_err(|error| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("a contributed causal rule document does not read: {error}"),
            )
            .with_help(
                "the document is a `causal_rules:` list of the `causal_rule` shape of \
                 `docs/contracts/kuang/contributions.v1.yaml`",
            )
        })
    }
}

/// A contributed recovery provider (v0.6 §48.2, §12.1, §48.5).
///
/// §48.5's PostgreSQL example is the shape this is written around. A database package
/// contributes restart semantics as impact, a checkpoint or quiesce action, an
/// application-consistency claim, recovery verification and a transaction-local rollback — and
/// every one of those is a claim about state a person may later need back. §12.2 fixes the
/// capabilities that carry each claim, and §48.4 makes them the boundary: the declaration is
/// read at load, before any package code runs, so a provider that says it can restore and holds
/// nothing destructive is refused rather than believed.
///
/// §39.2 is the sentence the `consistency` field exists to enforce: a filesystem snapshot of
/// PostgreSQL's files may be crash-consistent, and Ono MUST NOT label it `APPLICATION_CONSISTENT`
/// unless a PostgreSQL-aware provider asserts that guarantee. A package that claims it and
/// cannot quiesce cannot own the claim, and is refused at load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecoveryProviderContribution {
    /// `<package.id>.recovery-provider.<kebab-name>`.
    pub id: String,
    /// One line, for `get recovery-provider` and `help`.
    pub summary: String,
    /// The persistence domain kinds it covers, as `RecoveryScope` names them — `zfs-dataset`,
    /// `postgres-database`. Appendix B.1 requires a path to be mapped to a domain before
    /// protection is claimed, and this is the set of domains this provider claims to map.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domain_kinds: Vec<String>,
    /// The asset type it creates, from `docs/contracts/recovery/assets.yaml` — `zfs-snapshot`,
    /// `file-archive`. §11.1 makes the asset the thing a person inspects, so it is named before
    /// one exists.
    pub asset_type: String,
    /// The strongest consistency it can claim, from §11.3. `application-consistent` requires
    /// `recovery.quiesce`: §39.2 says the provider must own the claim, and a provider that
    /// cannot pause the application cannot own it.
    pub consistency: String,
    /// The restore methods it offers, from Appendix C.1. A provider that offers none cannot
    /// restore, and §62.1 calls a candidate nobody can use snapshot theatre.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restore_methods: Vec<String>,
    /// Whether its assets sit in the same failure domain as the state they protect (§11.5).
    ///
    /// A ZFS snapshot lives in the pool it protects; a file archive on another disk does not.
    /// §11.5 requires the answer to be visible beside the protection, because a snapshot that
    /// dies with its pool is not a backup, and a package that stays silent about it would leave
    /// the reader to assume the safer answer.
    #[serde(default)]
    pub shares_failure_domain: bool,
    /// The `recovery.*` capabilities the provider declares (§12.2, §48.3). Each is checked
    /// against the registry at load, and the ones that authorise a mutation are checked against
    /// the risk the package actually holds (§48.4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    /// The atomicity the provider states over its own resource scope (§27.1), or `None`.
    ///
    /// §27.3 makes generic distributed two-phase commit an explicit non-goal, so a declaration
    /// reaching beyond what this provider covers is refused at load rather than honoured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionContribution>,
}

/// The atomicity one provider states over its own resource scope (v0.6 §27.1, §27.3).
///
/// §27.1 lets a provider expose `begin`, `prepare`, `commit` and `rollback` and declare atomicity
/// **over its own resource scope**. §27.2 forbids the word `transaction` once a second boundary
/// is involved, and §27.3 makes generic distributed two-phase commit a non-goal: a plugin may
/// implement a domain-specific distributed transaction and then the plugin owns the guarantee,
/// which is not something a manifest can claim on Ono's behalf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransactionContribution {
    /// The domain kinds the atomicity covers. Every one must be a domain kind this provider
    /// itself declares; a boundary it does not own is `package.invalid` at load (§27.2, §27.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<String>,
    /// What the guarantee is, in prose the package stands behind. §27.1 puts the guarantee with
    /// the provider, so this is the sentence a reader holds it to.
    pub guarantee: String,
}

/// A contributed impact provider (v0.6 §48.2, §9.4).
///
/// §9.4 lets a package relate object types Ono's own graph does not reach — a database to the
/// service that fronts it, a config file to the workload that reads it. What it may add is
/// edges. What it may not do is raise a confidence: §49.3 and §8.1 both put that beyond anything
/// that is not a provider proving a fact, and the confidence lattice has no operation that
/// strengthens (`EffectConfidence::weakest_of` is the only combiner v0.6 defines).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImpactProviderContribution {
    /// `<package.id>.impact-provider.<kebab-name>`.
    pub id: String,
    /// One line, for `get impact-provider` and `help`.
    pub summary: String,
    /// The schema ids of the object types it can relate. Each resolves at load like a target's
    /// schema, so an impact provider that names a type nothing carries is refused before it can
    /// draw an edge to it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_types: Vec<String>,
    /// The relation labels it contributes, e.g. `reads-configuration-from`. §15.8's rule for
    /// causal language applies to impact too: a label a reader meets is one they can look up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<String>,
    /// The highest confidence any edge it contributes may carry, from §8.1. The host takes the
    /// weaker of this and whatever the edge claims and never the stronger, so a declaration here
    /// is a ceiling the package accepts rather than an authority it gains (§49.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_ceiling: Option<String>,
}

/// A contributed verification provider (v0.6 §48.2, §25.1).
///
/// §25.3 forbids the sentence "rollback successful" without a scope, and §25.1 names the scopes:
/// persistent state, runtime state, external side effects. A verification provider therefore
/// declares which equivalence domain each of its check kinds speaks to, because a check that
/// proves the bytes came back proves nothing about what left the machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationProviderContribution {
    /// `<package.id>.verification-provider.<kebab-name>`.
    pub id: String,
    /// One line, for `get verification-provider` and `help`.
    pub summary: String,
    /// The check kinds it can observe, paired with the equivalence domain each speaks to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<VerificationCheckContribution>,
}

/// One check kind a verification provider offers, and what it is evidence about (v0.6 §25.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerificationCheckContribution {
    /// The check kind, e.g. `postgres-accepts-connections`.
    pub kind: String,
    /// The equivalence domain of §25.1 it is evidence about — `persistent-state`,
    /// `runtime-state` or `external-side-effect`.
    pub equivalence: String,
    /// One line, what the check observes and what it does not.
    pub summary: String,
}

/// A contributed risk rule (v0.6 §48.2, §19.1, §19.2).
///
/// §19.2 makes risk rule-based rather than AI-generated, and a contributed rule is still a rule:
/// it is registered under a namespaced id, it is inspectable exactly as a built-in one is, and
/// the class it emits is folded into the plan's class by the one operation §19.2 defines —
/// `RiskAssessment::classify`, a maximum. There is no minimum, so a contributed rule can raise
/// a plan's class and can never reduce it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RiskRuleContribution {
    /// The rule id, namespaced to the package — `dev.example.postgres.risk.replica-lag`.
    pub rule_id: String,
    /// The §19.1 dimension it emits into — `downtime`, `irreversibility`, `bulk-count`.
    pub dimension: String,
    /// The highest §19.2 class it may emit — `low`, `moderate`, `high`, `critical`, `unknown`.
    /// A finding above it is refused; a finding below it composes as a maximum like any other.
    pub emits: String,
    /// One line, what the rule finds and why it matters. §40.2 shows this instead of "Are you
    /// sure?", so a rule that cannot say why it is gating teaches the flag rather than the risk.
    pub summary: String,
}

/// A contributed plan view (v0.6 §48.2, §45).
///
/// The `ChangeView` of §48.2, in the shape [`ViewContribution`] already uses for every other
/// contributed view — a package that renders a plan renders it through the same view lifecycle
/// as everything else (spec §31.27), and the only extra fact is which plan states it is for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeViewContribution {
    /// `<package.id>.change-view.<kebab-name>`.
    pub id: String,
    /// One line, for `help` and the view picker.
    pub summary: String,
    /// `interactive` or `static`, as [`ViewContribution::mode`] spells it.
    pub mode: String,
    /// The plan states it can render, from `docs/contracts/change/plans.yaml` — `sealed`,
    /// `recovery-planned`. A view offered for a state a plan cannot be in is a view nobody
    /// reaches, and the host settles the words at load.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plan_states: Vec<String>,
    /// The deterministic non-interactive output for a redirected stdout (spec §31.28, §50).
    pub fallback: String,
}

/// The document a `contributions.recovery_providers` path names (v0.6 §48.2, §31.68).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryProviderDocument {
    /// The recovery providers the document declares.
    pub recovery_providers: Vec<RecoveryProviderContribution>,
}

impl RecoveryProviderDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        parse_document(
            text,
            "recovery provider",
            "recovery_providers",
            "recovery_provider",
        )
    }
}

/// The document a `contributions.impact_providers` path names (v0.6 §48.2, §9.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactProviderDocument {
    /// The impact providers the document declares.
    pub impact_providers: Vec<ImpactProviderContribution>,
}

impl ImpactProviderDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        parse_document(
            text,
            "impact provider",
            "impact_providers",
            "impact_provider",
        )
    }
}

/// The document a `contributions.verification_providers` path names (v0.6 §48.2, §25.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationProviderDocument {
    /// The verification providers the document declares.
    pub verification_providers: Vec<VerificationProviderContribution>,
}

impl VerificationProviderDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        parse_document(
            text,
            "verification provider",
            "verification_providers",
            "verification_provider",
        )
    }
}

/// The document a `contributions.risk_rules` path names (v0.6 §48.2, §19.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskRuleDocument {
    /// The risk rules the document declares.
    pub risk_rules: Vec<RiskRuleContribution>,
}

impl RiskRuleDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        parse_document(text, "risk rule", "risk_rules", "risk_rule")
    }
}

/// The document a `contributions.change_views` path names (v0.6 §48.2, §45).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangeViewDocument {
    /// The plan views the document declares.
    pub change_views: Vec<ChangeViewContribution>,
}

impl ChangeViewDocument {
    /// Reads a declaration document.
    ///
    /// # Errors
    ///
    /// `package.invalid` when the document is not the shape
    /// `docs/contracts/kuang/contributions.v1.yaml` describes.
    pub fn parse(text: &str) -> Result<Self, KuangError> {
        parse_document(text, "change view", "change_views", "change_view")
    }
}

/// Reads one on-disk contribution document, refusing anything that is not the declared shape.
///
/// Spec §31.68 wants a package's contributions readable **without running it**, so every one of
/// these documents is parsed by the host before the runtime exists. `deny_unknown_fields` on each
/// document is what makes a typo a refusal rather than a silently dropped declaration.
fn parse_document<T: serde::de::DeserializeOwned>(
    text: &str,
    what: &str,
    key: &str,
    shape: &str,
) -> Result<T, KuangError> {
    serde_yaml_ng::from_str(text).map_err(|error| {
        KuangError::new(
            KuangErrorCode::PackageInvalid,
            format!("a contributed {what} document does not read: {error}"),
        )
        .with_help(format!(
            "the document is a `{key}:` list of the `{shape}` shape of \
             `docs/contracts/kuang/contributions.v1.yaml`"
        ))
    })
}

/// A contributed target (spec §31.23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetContribution {
    /// The target word, e.g. `echo-item`.
    pub name: String,
    /// The schema id objects of this target carry.
    pub schema: String,
    /// One line, for `help` and completion.
    pub summary: String,
    /// What makes two observations the same object, in prose.
    pub identity_doc: String,
    /// The options a query against this target is narrowed by — a context, a namespace, a kind.
    /// Declared so that `get <target> --<option>` has help and completion, exactly as a core
    /// target's does (`docs/contracts/kuang/contributions.v1.yaml` → `target.options`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ParameterContribution>,
    /// Whether the answer ends by itself.
    #[serde(default, skip_serializing_if = "Answer::is_default")]
    pub answer: Answer,
    /// The semantic roles objects of this target carry — `workload`, `storage`, `identity` — in
    /// the small cross-provider vocabulary of `docs/architecture/external-system-provider.md`
    /// §25 (ADR-0596). A role is additional semantics beside the native type, never a
    /// replacement for it: the schema stays the schema, and `find place --role workload` is what
    /// the role buys.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// The kind of place that is this kind's canonical spatial parent — the id of a schema one
    /// of this package's own targets declares — so that `up` from a place of this kind lands on
    /// one of that kind (spec v0.4 §11.3, §36.4; ADR-0597). The edge itself is contributed like
    /// any other: the manifest declares the shape `<schema>-><parent>` and the package answers
    /// the edge under `relation.write`. Spatial containment, never ownership.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// Whether a contributed target's answer ends by itself (spec §31.23, ADR-0588).
///
/// The host has to know this before it reads the first record, and it cannot find out by reading:
/// a package that has not sent a record yet and a package that will never stop sending them look
/// the same from the outside. So the package says which it is, in the document the host reads
/// before anything runs.
///
/// The consequence is the whole point. A bounded answer is collected and shown as a table; an
/// unbounded one becomes a live stream, which is what the shell's live view is fed by
/// (`ono_cli::live`, shell specification §18.2, §18.3). Without the declaration the host had one
/// choice for both, and it chose to collect — so a package whose answer never ends never returned
/// to the prompt.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    /// The answer is a finite collection: the host reads it to the end and shows it.
    ///
    /// The default, and deliberately so. A package that says nothing about its answer gets the
    /// behaviour every package had before there was anywhere to say it, and a package whose
    /// answer *does* end is the ordinary case.
    #[default]
    Bounded,
    /// The answer continues until the operator ends it: a watch, a followed log, a subscription.
    ///
    /// The host must not collect one. It becomes an unbounded stream, cancellable by the
    /// operator, and the invocation is cancelled when the stream is dropped (spec §31.14:
    /// cancellation is delivered, not inferred).
    Unbounded,
}

impl Answer {
    /// Whether this is the default, so that a document and a handshake that say nothing carry
    /// nothing.
    #[must_use]
    pub fn is_default(&self) -> bool {
        matches!(self, Self::Bounded)
    }

    /// Whether the answer ends by itself.
    #[must_use]
    pub fn is_bounded(self) -> bool {
        matches!(self, Self::Bounded)
    }
}

/// A contributed schema, in the field vocabulary of `docs/contracts/schemas/*.v1.yaml`
/// (spec §31.23: contributed schemas are written in the same language core schemas are).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaContribution {
    /// `<package.id>.<kebab-name>/<major>`.
    pub id: String,
    /// The schema's display name.
    pub name: String,
    /// One line, what an object of this schema is.
    pub summary: String,
    /// The identity fields (spec §27.3).
    pub identity: Vec<String>,
    /// The fields, in declaration order.
    pub fields: Vec<SchemaFieldContribution>,
}

/// One field of a contributed schema. Exactly one of `required` and `nullable` is true
/// (ADR-0012 §8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaFieldContribution {
    /// The field name.
    pub name: String,
    /// The type name as the registries spell it, e.g. `int`, `string`, `list<string>`.
    #[serde(rename = "type")]
    pub field_type: String,
    /// The manifest is invalid without this field.
    #[serde(default)]
    pub required: bool,
    /// May be absent or null; absent means unknown, never a default (spec §10.5).
    #[serde(default)]
    pub nullable: bool,
}

// --- typed parameters and results, per protocol.v1.yaml ---------------------------------------

/// Parameters of [`method::LIFECYCLE_INIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitParams {
    /// The negotiated contract of spec §31.63.
    pub contract: PluginContract,
}

/// The plugin's answer to [`method::LIFECYCLE_INIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InitResult {
    /// Whether the plugin is ready to serve.
    pub ready: bool,
    /// The features the plugin switched off because of denied optional capabilities —
    /// its own account of what it gave up (spec §31.63).
    #[serde(default)]
    pub disabled_features: Vec<String>,
    /// Why the plugin cannot serve, when `ready` is false.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Why the host is shutting the instance down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShutdownReason {
    /// `unload plugin`.
    Unload,
    /// A new version is taking over (spec §31.35).
    Upgrade,
    /// A capability the plugin holds was withdrawn.
    Revocation,
    /// Policy ended the instance.
    Policy,
    /// The instance was idle past its budget.
    Idle,
    /// The host itself is exiting.
    HostExit,
}

/// Parameters of [`method::LIFECYCLE_SHUTDOWN`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShutdownParams {
    /// Why.
    pub reason: ShutdownReason,
    /// How long the plugin has to drain, in milliseconds. After it, the instance is terminated.
    pub deadline_ms: u64,
}

/// Parameters of [`method::COMMAND_INVOKE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeParams {
    /// The contributed command id to run.
    pub command: String,
    /// Selectors and options by name, already bound and typed by the host's command layer.
    pub arguments: JsonMap<String, Json>,
    /// The output stream handle the plugin emits into.
    pub output: u64,
    /// The invocation every handle the plugin opens will belong to.
    pub invocation: u64,
    /// The initial emission credit on `output` (the pull protocol's opening window).
    pub credit: u32,
}

/// Parameters of [`method::PROVIDER_QUERY`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryParams {
    /// The contributed target being queried.
    pub target: String,
    /// Provider options by name.
    #[serde(default)]
    pub options: JsonMap<String, Json>,
    /// The output stream handle the plugin answers into.
    pub output: u64,
    /// The owning invocation.
    pub invocation: u64,
    /// The initial emission credit on `output`.
    pub credit: u32,
}

/// How an invocation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvokeStatus {
    /// The invocation ran to completion.
    Completed,
    /// The invocation failed; `error` says how.
    Failed,
    /// The invocation observed cancellation and stopped.
    Cancelled,
}

/// The plugin's answer to [`method::COMMAND_INVOKE`] and [`method::PROVIDER_QUERY`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InvokeResult {
    /// How it ended.
    pub status: InvokeStatus,
    /// The failure, for `failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Parameters of [`method::STREAM_DEMAND`]: credit is cumulative, never a rate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DemandParams {
    /// The plugin's output stream.
    pub handle: u64,
    /// How many more values the host will accept.
    pub credit: u32,
}

/// Why a stream was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CancelReason {
    /// The consumer went away.
    ConsumerGone,
    /// The invocation's deadline passed.
    Deadline,
    /// Policy ended the stream.
    Policy,
    /// A capability the stream depended on was revoked.
    Revocation,
    /// The operator cancelled.
    Operator,
}

/// Parameters of [`method::STREAM_CANCEL`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelParams {
    /// The stream being cancelled.
    pub handle: u64,
    /// Why.
    pub reason: CancelReason,
}

/// The plugin's answer to [`method::HEALTH_PROBE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
    /// The instance's own judgement of itself.
    pub state: HealthState,
    /// Optional detail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A health answer (spec §31.35).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HealthState {
    /// Ready to serve.
    Ready,
    /// Serving, but at capacity.
    Busy,
    /// Running with disabled features.
    Degraded,
}

/// Parameters of [`method::STREAMS_EMIT`]: at most the credit the host last granted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmitParams {
    /// The output stream.
    pub handle: u64,
    /// The values, in the tagged JSON encoding of `ono-value`.
    pub values: Vec<Json>,
}

/// The host's answer to [`method::STREAMS_EMIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmitResult {
    /// How many more values the host will now accept on this stream.
    pub credit: u32,
}

/// Parameters of [`method::STREAMS_CLOSE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloseParams {
    /// The output stream.
    pub handle: u64,
    /// The terminal error, when the producer failed. `None` is a normal completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Parameters of [`method::CAPABILITIES_CHECK`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckParams {
    /// The capability id.
    pub capability: String,
    /// The scope the plugin would use it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<JsonMap<String, Json>>,
}

/// The host's answer to [`method::CAPABILITIES_CHECK`]. Asking never prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CheckAnswer {
    /// The call would proceed.
    Granted,
    /// The call would be refused.
    Denied,
    /// A request would prompt the operator. Not a grant.
    Ask,
    /// The host cannot say.
    Unknown,
}

/// Parameters of [`method::CAPABILITIES_REQUEST`] (spec §31.17's runtime-requested class).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestOnceParams {
    /// The capability id.
    pub capability: String,
    /// The requested scope — no broader than the declaration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<JsonMap<String, Json>>,
    /// Why, in the package's words. Sanitised before display.
    pub purpose: String,
    /// The explicit user action the request answers. A request with none is denied without
    /// prompting.
    pub action_context: String,
}

/// Parameters of [`method::AUDIT_LOG`] (spec §31.33: structured records, never stderr).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuditLogParams {
    /// The severity, e.g. `info`, `warn`, `error`.
    pub level: String,
    /// The message.
    pub message: String,
    /// Structured fields.
    #[serde(default)]
    pub fields: JsonMap<String, Json>,
}

/// Parameters of [`method::STATE_GET`] / [`method::STATE_DELETE`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateKeyParams {
    /// A key in the package's own store.
    pub key: String,
    /// Which store (spec §31.31).
    pub class: String,
}

/// The host's answer to [`method::STATE_GET`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateGetResult {
    /// The stored value in tagged encoding, or `None` when the key is unset — which is not an
    /// empty value (spec §10.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<Json>,
}

/// Parameters of [`method::STATE_SET`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateSetParams {
    /// A key in the package's own store.
    pub key: String,
    /// Which store.
    pub class: String,
    /// The value in tagged encoding.
    pub value: Json,
}

/// Parameters of [`method::FILESYSTEM_READ`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemReadParams {
    /// The path, which must resolve inside the granted `paths` scope.
    pub path: String,
    /// Byte offset. `None` starts at the beginning.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Bytes to read. `None` reads to the host's per-call ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u64>,
}

/// The host's answer to [`method::CLOCK_NOW`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockNowResult {
    /// The instant, in tagged encoding (`{"$timestamp": …}`). Virtual under the test host.
    pub now: Json,
}

/// The host's answer to [`method::FILESYSTEM_READ`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilesystemReadResult {
    /// The bytes read, in tagged encoding (`{"$bytes": …}`). Never interpreted.
    pub content: Json,
}

impl SchemaContribution {
    /// Converts the contribution into the `ono-value` schema both sides then build and
    /// validate records against (spec §31.23: contributed schemas are written in the same
    /// language core schemas are).
    ///
    /// # Errors
    ///
    /// Returns `package.invalid` for a field that is neither required nor nullable
    /// (ADR-0012 §8), an unknown type name, or a shape `ono-value` refuses.
    pub fn to_schema(&self) -> Result<ono_value::Schema, crate::KuangError> {
        use crate::{KuangError, KuangErrorCode};
        let invalid = |detail: String| KuangError::new(KuangErrorCode::PackageInvalid, detail);
        let id: ono_value::SchemaId = self
            .id
            .parse()
            .map_err(|_| invalid(format!("`{}` is not a schema id", self.id)))?;
        let mut builder = ono_value::Schema::builder(id, &self.name).doc(&self.summary);
        for field in &self.fields {
            if field.required == field.nullable {
                return Err(invalid(format!(
                    "field `{}` must be exactly one of required and nullable (ADR-0012 §8)",
                    field.name
                )));
            }
            let field_type = parse_type_name(&field.field_type).ok_or_else(|| {
                invalid(format!(
                    "field `{}` has unknown type `{}`",
                    field.name, field.field_type
                ))
            })?;
            let mut def = ono_value::FieldDef::new(&field.name, field_type);
            def = if field.required {
                def.required()
            } else {
                def.nullable()
            };
            builder = builder.field(def);
        }
        builder = builder.identity(self.identity.iter().map(String::as_str));
        builder
            .build()
            .map_err(|error| invalid(format!("schema `{}` is invalid: {error}", self.id)))
    }
}

/// Parses a registry type name (`int`, `list<string>`, `enum<a|b>`, `record<x/1>`, …) into the
/// `ono-value` type it names, or `None` for a name the vocabulary does not carry.
#[must_use]
pub fn parse_type_name(name: &str) -> Option<ono_value::FieldType> {
    use ono_value::FieldType;
    let name = name.trim();
    if let Some(inner) = name
        .strip_prefix("list<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Some(FieldType::list(parse_type_name(inner)?));
    }
    if let Some(inner) = name
        .strip_prefix("enum<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        let variants: Vec<&str> = inner.split('|').collect();
        return Some(FieldType::enumeration(&variants));
    }
    if let Some(inner) = name
        .strip_prefix("record<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Some(FieldType::Record(inner.parse().ok()?));
    }
    if let Some(inner) = name
        .strip_prefix("ref<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Some(FieldType::Ref(inner.parse().ok()?));
    }
    Some(match name {
        "any" => FieldType::Any,
        "bool" => FieldType::Bool,
        "int" => FieldType::Int,
        "float" => FieldType::Float,
        "decimal" => FieldType::Decimal,
        "string" => FieldType::String,
        "bytes" => FieldType::Bytes,
        "path" => FieldType::Path,
        "timestamp" => FieldType::Timestamp,
        "duration" => FieldType::Duration,
        "bytesize" => FieldType::ByteSize,
        "percent" => FieldType::Percent,
        "regex" => FieldType::Regex,
        "uuid" => FieldType::Uuid,
        "ip" => FieldType::Ip,
        "ipnetwork" => FieldType::IpNetwork,
        "port" => FieldType::Port,
        "map" => FieldType::Map,
        "error" => FieldType::Error,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_round_trip_an_envelope_when_serialised() {
        let envelope = Envelope::Request {
            seq: 7,
            method: method::STREAMS_EMIT.to_owned(),
            params: serde_json::json!({"handle": 1, "values": [3]}),
        };
        let text = serde_json::to_string(&envelope).expect("serialises");
        let back: Envelope = serde_json::from_str(&text).expect("parses");
        assert_eq!(back, envelope);
    }

    #[test]
    fn should_keep_result_and_error_distinct_when_a_response_crosses() {
        let text = r#"{"kind":"response","seq":3,"error":{"code":"Ono-Sendai-K11301","name":"capability.denied","message":"no grant"}}"#;
        let envelope: Envelope = serde_json::from_str(text).expect("parses");
        let Envelope::Response { result, error, .. } = envelope else {
            panic!("expected a response");
        };
        assert!(result.is_none());
        assert_eq!(error.expect("error").name, "capability.denied");
    }
}

/// Parameters of [`method::STREAMS_NEXT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextParams {
    /// The stream to read.
    pub handle: u64,
    /// How many values the plugin is ready for: the credit. The host sends no more than this.
    pub max: u64,
    /// How long to wait for the first value. Absent uses the invocation's deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline: Option<serde_json::Value>,
}

/// Parameters of [`method::STREAMS_CANCEL`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamHandleParams {
    /// Any stream in either direction.
    pub handle: u64,
}

/// The answer to [`method::STREAMS_NEXT`]: `complete` with no error is a normal end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NextResult {
    /// The values, at most `max` of them, in order.
    pub values: Vec<serde_json::Value>,
    /// Whether the stream has nothing more to give.
    pub complete: bool,
    /// The terminal failure, when the stream ended in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// Parameters of [`method::SCHEMAS_GET`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaGetParams {
    /// A schema id, e.g. `ono.process/1`.
    pub id: String,
}

/// Parameters of [`method::SCHEMAS_LIST`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SchemaListParams {
    /// Restrict to ids under a namespace. Absent lists every registered schema.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

// --- the change and recovery domain of v0.6 §48 ------------------------------------------------
//
// The wire shapes mirror `ono-change-core`'s domain types field for field, and this crate depends
// on that crate for none of them. §31.61's boundary is JSON, the protocol crate stays free of the
// domain, and the testhost holds the two together with a round-trip test — so a field renamed on
// one side is a failing test rather than a value that silently stops arriving.

/// `change.plan.read`: which plan to read (v0.6 §48.3, §5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanReadParams {
    /// The plan id, as `ono.change-plan/1` carries it.
    pub plan: String,
}

/// `change.plan.contribute`: what one package adds to a plan being resolved (v0.6 §48.2, §48.3).
///
/// `deny_unknown_fields` is load-bearing rather than tidy. §19.2 makes the plan's risk class the
/// fold of its findings and nothing else, so there is deliberately no field here by which a
/// package could state the class directly; a package that invents one is a protocol violation
/// rather than a package whose extra key is ignored.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanContributeParams {
    /// The plan being contributed to.
    pub plan: String,
    /// Plan actions the package resolved, as `ono.plan-action/1` records.
    #[serde(default)]
    pub actions: Vec<Json>,
    /// Prospective effects, in the shape Appendix A.5's coverage algorithm reads.
    #[serde(default)]
    pub effects: Vec<EffectClassContribution>,
    /// Impact edges, as `ono.graph-edge/1` records. §49.3 and §9.4: edges, never a confidence
    /// the host did not already have a provider for.
    #[serde(default)]
    pub impact: Vec<Json>,
    /// Risk findings from rules the package declared (§19.2).
    #[serde(default)]
    pub risk_findings: Vec<RiskFindingContribution>,
}

/// One risk finding a contributed rule produced (v0.6 §19.2).
///
/// It carries a class, and carrying a class is not deciding one: the host folds every finding
/// with `RiskAssessment::classify`, which is a maximum. §19.2 defines no minimum, so there is no
/// path from a finding to a lower plan class however low the finding is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskFindingContribution {
    /// The §19.1 dimension, mirroring `RiskFinding::dimension`.
    pub dimension: String,
    /// The §19.2 class, mirroring `RiskFinding::class`.
    pub class: String,
    /// The rule that found it, mirroring `RiskFinding::rule`. It must be a rule this package
    /// declared in its `risk_rules` contribution.
    pub rule: String,
    /// The sentence §40.2 shows instead of "Are you sure?", mirroring `RiskFinding::reason`.
    pub reason: String,
}

/// The scope one recovery asset covers, mirroring `RecoveryScope` (v0.6 §11.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryScopeWire {
    /// The resolved persistence object, e.g. `tank/var`.
    pub domain: String,
    /// What kind of object that is, e.g. `zfs-dataset`. The value the broker checks a
    /// `domain_kinds` scope against.
    pub domain_kind: String,
    /// The objects the scope actually covers. §13.4 and §14.3 turn on this being the truth
    /// rather than the paths a person hoped were included.
    #[serde(default)]
    pub covers: Vec<String>,
    /// The host the domain is on.
    pub host: String,
}

/// What a recovery asset costs, mirroring `RecoveryCost` (v0.6 §38).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCostWire {
    /// The space it takes at creation, as a typed `ByteSize` value.
    #[serde(default)]
    pub initial_bytes: Option<Json>,
    /// The space it takes now.
    #[serde(default)]
    pub retained_bytes: Option<Json>,
    /// Whether the figures are estimates (§37.5). Absent means estimated, which is the honest
    /// default for a figure nobody measured.
    #[serde(default = "estimated_by_default")]
    pub estimated: bool,
    /// How long creation takes, as a typed `Duration` value.
    #[serde(default)]
    pub creation_latency: Option<Json>,
    /// How long the application is paused for (§18.4).
    #[serde(default)]
    pub quiesce: Option<Json>,
    /// Whether using it needs a reboot (§13.7, §14.6).
    #[serde(default)]
    pub requires_reboot: bool,
    /// Whether using it needs the filesystem offline.
    #[serde(default)]
    pub requires_offline: bool,
}

const fn estimated_by_default() -> bool {
    true
}

/// Something an asset does not cover, mirroring `RecoveryExclusion` (v0.6 §11.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryExclusionWire {
    /// What is outside the asset.
    pub subject: String,
    /// Why it is outside it.
    pub reason: String,
}

/// A protection opportunity, mirroring `RecoveryCandidate` (v0.6 Appendix A.3).
///
/// §2.1 keeps a candidate a description: nothing exists yet, and creating it is a PREPARE action
/// the plan shows first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCandidateWire {
    /// The provider that offered it.
    pub provider: String,
    /// What it would protect.
    pub scope: RecoveryScopeWire,
    /// The Appendix A.1 domain it covers.
    pub domain: String,
    /// The Appendix A.2 objective it would satisfy.
    pub objective: String,
    /// The §11.3 consistency it would achieve.
    pub consistency: String,
    /// The Appendix C.1 method it would be restored by.
    pub restore_method: String,
    /// What creating it costs.
    #[serde(default)]
    pub cost: RecoveryCostWire,
    /// What it would not cover.
    #[serde(default)]
    pub exclusions: Vec<RecoveryExclusionWire>,
    /// What creating it needs — a capability, free space, a quiesce window.
    #[serde(default)]
    pub creation_requirements: Vec<String>,
    /// What restoring from it needs — a reboot, an unmount, a stronger privilege (§43.4).
    #[serde(default)]
    pub restore_requirements: Vec<String>,
    /// The sentence the plan view shows.
    pub detail: String,
}

/// `recovery.discover`: the candidates a provider found for one domain (v0.6 §12.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryDiscoverParams {
    /// The persistence domain kind the candidates are about. The broker checks it against the
    /// granted `domain_kinds` before the host is asked to record anything.
    pub domain_kind: String,
    /// The candidates. An empty list means "nothing here", which §55.6 case 29 keeps distinct
    /// from "this provider could not answer".
    #[serde(default)]
    pub candidates: Vec<RecoveryCandidateWire>,
}

/// `recovery.prepare`: the asset a protection action produced (v0.6 §4.5, §12.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPrepareParams {
    /// The scope the asset covers.
    pub scope: RecoveryScopeWire,
    /// The asset, as an `ono.recovery-asset/1` record.
    pub asset: Json,
}

/// `recovery.validate`: what checking an asset against §11.4's list found.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryValidateParams {
    /// The scope the asset covers.
    pub scope: RecoveryScopeWire,
    /// The asset id being checked.
    pub asset: String,
    /// The findings, as `ono.recovery-asset/1`'s validation record shapes them. An asset that
    /// was not checked is not a valid asset (§11.4), so an empty list is a validation that found
    /// nothing rather than one that did not run.
    #[serde(default)]
    pub findings: Vec<Json>,
}

/// `recovery.restore`: one restore step and what it did (v0.6 §12.1, §24).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryRestoreParams {
    /// The scope being restored.
    pub scope: RecoveryScopeWire,
    /// The asset being restored from.
    pub asset: String,
    /// The Appendix C.1 method used. §13.6 and §14.4 make the method the fact a person needs
    /// most: a dataset rollback and a selective file restore lose entirely different things.
    pub method: String,
    /// What the restore could not put back, as `ono.recovery-plan/1` records the unrecoverable
    /// effects (§25.3 forbids a scopeless claim of success).
    #[serde(default)]
    pub unrecoverable: Vec<Json>,
}

/// `recovery.cleanup`: an asset the provider removed (v0.6 §37).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryCleanupParams {
    /// The scope the asset covered.
    pub scope: RecoveryScopeWire,
    /// The asset id.
    pub asset: String,
}

/// `recovery.estimate_cost`: what an asset costs now (v0.6 §38).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryEstimateCostParams {
    /// The persistence domain kind the asset belongs to.
    pub domain_kind: String,
    /// The asset id.
    pub asset: String,
    /// The measured or estimated cost.
    pub cost: RecoveryCostWire,
}

/// `recovery.quiesce` and `recovery.resume`: one step of §39.3's five (v0.6 §16.4, §18.4).
///
/// §39.3 names `prepare_quiesce`, `verify_quiesced`, `create_storage_asset`, `resume` and
/// `verify_resumed`, and requires compensation rules for a failure at each step. The step is on
/// the wire so a failure is attributable to one of them rather than to "quiescing".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryQuiesceParams {
    /// The application the package paused or released. The host cannot resolve this name, so
    /// the `applications` scope over it is advisory and is labelled as such wherever it shows.
    pub application: String,
    /// The §39.3 step this reports — `prepare_quiesce`, `verify_quiesced`,
    /// `create_storage_asset`, `resume` or `verify_resumed`.
    pub step: String,
    /// What the package will do if the step fails (§39.3's compensation rules), or `None` when
    /// the step is the compensation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<String>,
}

/// `verification.observe`: the result of observing one verification contract (v0.6 §23, §25.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationObserveParams {
    /// The check id, as `ono.change-verification/1` carries it.
    pub check: String,
    /// The §23.3 status — `passed`, `failed`, `unknown` or `skipped`. §23.5 forbids treating
    /// `unknown` as success, so there is no default and a package must say which it means.
    pub status: String,
    /// The §25.1 equivalence domain the observation is evidence about, where it is a recovery
    /// verification. §25.3 forbids the scopeless claim, so a recovery check names its scope.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equivalence: Option<String>,
    /// What was observed, in one line.
    pub detail: String,
}

/// A contributed view (spec §31.27; `contributions.v1.yaml` → `view.declaration`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewContribution {
    /// `<package.id>.view.<kebab-name>`.
    pub id: String,
    /// The input type, e.g. `stream<packet-eye.flow/2>`.
    pub accepts: String,
    /// `interactive` or `static`: whether the view takes key input.
    pub mode: String,
    /// Key bindings to view actions, e.g. `{enter: inspect-selected, q: close}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<serde_json::Map<String, serde_json::Value>>,
    /// The deterministic output when stdout is redirected (spec §31.28).
    pub fallback: String,
    /// One line, for `help`.
    pub summary: String,
}

/// The components a view tree may be built of (spec §31.27), complete.
pub const VIEW_COMPONENTS: [&str; 14] = [
    "Text",
    "Table",
    "Tree",
    "Graph",
    "KeyValue",
    "LogStream",
    "Sparkline",
    "Gauge",
    "Tabs",
    "Split",
    "CommandPalette",
    "ObjectPicker",
    "StatusLine",
    // v0.5 §37.6: a package may contribute an alternate view over the canonical temporal
    // schemas. Without a component for it, "consume canonical temporal schemas" would mean
    // redrawing a timeline out of `Table` rows, and the constraint that a view "cannot create
    // causality that is absent from its input" would have nowhere to live.
    "Timeline",
];

/// Parameters of [`method::VIEWS_OPEN`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewOpenParams {
    /// A contributed view id.
    pub view: String,
    /// The stream the view renders, when it renders one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
}

/// The answer to [`method::VIEWS_OPEN`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewOpenResult {
    /// The view's handle.
    pub handle: u64,
    /// Whether a terminal took it. False when output is redirected: the package emits its
    /// declared fallback instead (spec §31.28).
    pub mounted: bool,
    /// The terminal's size when mounted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<ViewSize>,
}

/// A terminal size, as `view.mount` and a `resize` event carry it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSize {
    /// Rows.
    pub rows: u16,
    /// Columns.
    pub columns: u16,
}

/// Parameters of [`method::VIEWS_SUBMIT`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewSubmitParams {
    /// The view to update.
    pub view: u64,
    /// A view tree of the components in `contributions.v1.yaml`.
    pub tree: serde_json::Value,
}

/// Parameters of [`method::VIEWS_CLOSE`], [`method::VIEW_UNMOUNT`] and the like.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewHandleParams {
    /// The view.
    pub view: u64,
}

/// Parameters of [`method::VIEW_MOUNT`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewMountParams {
    /// The view being mounted.
    pub view: u64,
    /// The terminal's size.
    pub size: ViewSize,
    /// The stream to render, when the view was opened over one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
}

/// Parameters of [`method::VIEW_EVENT`]: `{kind, key?, size?}` with `kind` one of `key`,
/// `resize`, `focus`, `blur`, `cancel`, `close`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewEventParams {
    /// The view the event is for.
    pub view: u64,
    /// The event.
    pub event: ViewEvent,
}

/// One event a view receives (spec §31.28).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewEvent {
    /// `key`, `resize`, `focus`, `blur`, `cancel` or `close`.
    pub kind: String,
    /// The key, for `key`: a character, or `enter`, `esc`, `tab`, `backtab`, `backspace`,
    /// `delete`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, or
    /// `ctrl-<char>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The new size, for `resize`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<ViewSize>,
}
