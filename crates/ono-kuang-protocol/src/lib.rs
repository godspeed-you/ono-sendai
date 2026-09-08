//! The wire types of the KUANG/11 extension runtime (spec §31).
//!
//! KUANG/11 is the software that can be loaded into the deck: Ono's extension runtime, and
//! nothing else (spec §0, AGENTS.md §3). This crate holds everything both sides of that
//! boundary must agree on, and nothing that runs on either side:
//!
//! - **The manifest** ([`Manifest`]): parsing and fail-closed validation of a
//!   `kuang-package/1` document, with every identity rule of spec §31.5 enforced — reverse-DNS
//!   ids, publisher namespacing, and the `ono.*` reservation no third party may claim.
//! - **The messages** ([`Envelope`], [`Hello`], the typed parameter shapes): the calls of
//!   `docs/contracts/kuang/protocol.v1.yaml`, over length-declared frames ([`read_frame`]/[`write_frame`]) whose bounds
//!   are checked before allocation.
//! - **The lifecycle** ([`Lifecycle`], [`PluginState`]): spec §31.8's six states with legal
//!   transitions enforced in the type.
//! - **The capability model** ([`Capability`], [`Lease`], [`Decision`]): the thirty
//!   families with their scope shapes and enforcement levels (spec §31.16–§31.19, §31.49).
//! - **The negotiated contract** ([`PluginContract`]): what a load produces and
//!   `lifecycle.init` delivers (spec §31.63).
//! - **The audit record** ([`AuditEvent`]): spec §31.37's `PluginAction`, in the shape of
//!   `ono.plugin-audit-event/1`.
//! - **The error taxonomy** ([`KuangErrorCode`], [`KuangError`]): spec §31.79's twenty-seven
//!   codes, integrated with the global error model (ADR-0022 §13).
//!
//! The supervisor side lives in `ono-kuang-supervisor`; the plugin side in `ono-kuang-sdk`.
//! Values cross the boundary in the tagged, lossless JSON encoding of `ono-value`
//! (`ono_value::to_json` / `from_json`), so nothing typed is flattened on the way (ADR-0040).

mod artifact;
mod audit;
mod capability;
mod catalog;
mod confinement;
mod contract;
mod error;
mod frame;
mod lifecycle;
mod manifest;
mod message;
mod permission;
mod signature;
mod version;

pub use artifact::{BUNDLE_FILE, artifact_files};
pub use audit::{AuditEvent, AuditResult};
pub use capability::{
    Capability, Decision, DeclarationClass, Elevation, Enforcement, GrantDuration, Lease, Risk,
    ScopeKey, ScopeKind,
};
pub use catalog::{
    Artifact, CATALOG_FORMAT, Catalog, CatalogEntry, CatalogRelease, CatalogVerification,
    PluginRef, SYSTEM_ORIGIN_FORMAT, SourceKind, SystemOrigin, compare_versions, looks_like_id,
};
pub use confinement::{Control, ExecutionTier, FailureBehaviour, Requirement};
pub use contract::{
    DeniedCapability, EffectiveLimits, GrantedCapability, OverflowPolicy, PluginContract,
};
pub use error::{KuangError, KuangErrorCode, WireError};
pub use frame::{FrameError, FrameLimits, decode_payload, encode_frame, read_frame, write_frame};
pub use lifecycle::{Lifecycle, PluginState, TransitionError};
pub use manifest::{
    CapabilityRequest, Compatibility, ContributionPaths, CpuBudget, Dependencies, Manifest,
    NetworkDeclaration, Outbound, PackageInfo, Persistence, Role, Runtime, RuntimeKind, Startup,
    StateDeclaration, validate_contributed_id,
};
pub use message::{
    ActionContribution, Answer, AuditLogParams, CancelParams, CancelReason, CheckAnswer,
    CheckParams, ClockNowResult, CloseParams, CommandContribution, CommandDocument,
    ContributionSet, DemandParams, EmitParams, EmitResult, Envelope, FilesystemReadParams,
    FilesystemReadResult, HealthState, Hello, Idempotency, InitParams, InitResult, InvokeParams,
    InvokeResult, InvokeStatus, NextParams, NextResult, ParameterContribution, ProbeResult,
    QueryParams, RequestOnceParams, SchemaContribution, SchemaFieldContribution, SchemaGetParams,
    SchemaListParams, ShutdownParams, ShutdownReason, StateGetResult, StateKeyParams,
    StateSetParams, StreamHandleParams, TargetContribution, TargetDocument, VIEW_COMPONENTS,
    ViewContribution, ViewEvent, ViewEventParams, ViewHandleParams, ViewMountParams,
    ViewOpenParams, ViewOpenResult, ViewSize, ViewSubmitParams, method, parse_type_name,
};
pub use permission::{
    AccessProfile, ConsentClass, DeltaEntry, DeltaKind, GrantTemplate, MINIMAL, PURPOSE_LIMIT,
    PermissionDescriptor, PermissionKind, PermissionPhase, PermissionRisk, PermissionSet,
    RECOMMENDED, RawGrantTemplate, RawPermissionRequest, RawPermissions, RawProfile, ScopeTemplate,
    TITLE_LIMIT, consent_class, default_kind, delta, derived_id, describe_scope, family_title,
    is_permission_id, minimum_risk, sanitize, validate_declared,
};
pub use signature::{
    FileDigest, PackageSignature, PublicKey, SIGNATURE_ALGORITHM, SIGNATURE_FILE, SIGNATURE_FORMAT,
    SecretKey, SignedPackage, content_digest,
};
pub use version::{
    ApiVersion, HOST_API, PACKAGE_FORMAT, PACKAGE_FORMAT_2, PACKAGE_FORMATS, VALUE_PROTOCOL,
    VersionRange,
};

/// Whether `word` is a semantic role word: kebab-case ASCII, starting with a letter (ADR-0596).
///
/// The vocabulary is open on purpose — the exact registry is reserved by the external-system
/// provider contract §42.3 — and the spelling is closed, so that `find place --role workload`
/// and a package's `roles: [workload]` are one word and not two.
#[must_use]
pub fn is_role_word(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && word
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !word.ends_with('-')
        && !word.contains("--")
}
