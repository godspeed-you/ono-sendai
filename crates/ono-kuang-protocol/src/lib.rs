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
//! - **The capability model** ([`Capability`], [`Lease`], [`Decision`]): the twenty-nine
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
mod confinement;
mod contract;
mod error;
mod frame;
mod lifecycle;
mod manifest;
mod message;
mod signature;
mod version;

pub use artifact::artifact_files;
pub use audit::{AuditEvent, AuditResult};
pub use capability::{
    Capability, Decision, DeclarationClass, Elevation, Enforcement, GrantDuration, Lease, Risk,
    ScopeKey, ScopeKind,
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
    AuditLogParams, CancelParams, CancelReason, CheckAnswer, CheckParams, ClockNowResult,
    CloseParams, CommandContribution, CommandDocument, ContributionSet, DemandParams, EmitParams,
    EmitResult, Envelope, FilesystemReadParams, FilesystemReadResult, HealthState, Hello,
    InitParams, InitResult, InvokeParams, InvokeResult, InvokeStatus, NextParams, NextResult,
    ProbeResult, QueryParams, RequestOnceParams, SchemaContribution, SchemaFieldContribution,
    SchemaGetParams, SchemaListParams, ShutdownParams, ShutdownReason, StateGetResult,
    StateKeyParams, StateSetParams, StreamHandleParams, TargetContribution, TargetDocument,
    VIEW_COMPONENTS, ViewContribution, ViewEvent, ViewEventParams, ViewHandleParams,
    ViewMountParams, ViewOpenParams, ViewOpenResult, ViewSize, ViewSubmitParams, method,
    parse_type_name,
};
pub use signature::{
    FileDigest, PackageSignature, PublicKey, SIGNATURE_ALGORITHM, SIGNATURE_FILE, SIGNATURE_FORMAT,
    SecretKey, SignedPackage,
};
pub use version::{ApiVersion, HOST_API, PACKAGE_FORMAT, VALUE_PROTOCOL, VersionRange};
