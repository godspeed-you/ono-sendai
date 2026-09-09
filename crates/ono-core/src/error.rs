//! The stable error taxonomy of spec §43 and the error kinds of spec §16.1.
//!
//! Codes are payload-free so that every layer of the shell can name one without depending on
//! the value model. The full error record of spec §16.1 — target, cause chain, help, metadata —
//! is `ono_value::ErrorValue`, because spec §25 makes `Error` a variant of `Value`.
//!
//! The taxonomy is closed and additive (ADR-0006): a code is never renumbered, removed or
//! re-pointed at a different meaning.

use std::fmt;

/// The broad category an error belongs to, as spec §16.1 defines it.
///
/// Scripts branch on the kind; the [`ErrorCode`] carries the precise identity. ADR-0006 extends
/// the list of spec §16.1 with `Safety` and `Stream`, which spec §43 needs and §16.1 omits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorKind {
    /// A name could not be resolved to a command, target or object.
    Resolution,
    /// The operation is understood but not permitted for this user or context.
    Permission,
    /// The operating system refused or failed an I/O operation.
    Io,
    /// The input could not be parsed.
    Parse,
    /// A value, field or unit did not have the required type.
    Type,
    /// A provider could not answer, or answered outside its advertised schema.
    Provider,
    /// An external process failed or was signalled.
    External,
    /// The requested state conflicts with the state that already exists.
    Conflict,
    /// The operation did not complete within its budget.
    Timeout,
    /// The operation was cancelled before completing.
    Cancelled,
    /// A safety policy or confirmation requirement stopped the operation.
    Safety,
    /// The operation is not valid for a stream with these properties.
    Stream,
    /// A resource budget — items retained, bytes retained, values materialized — was reached.
    Resource,
}

impl ErrorKind {
    /// Every kind, in declaration order.
    pub const ALL: &'static [ErrorKind] = &[
        ErrorKind::Resolution,
        ErrorKind::Permission,
        ErrorKind::Io,
        ErrorKind::Parse,
        ErrorKind::Type,
        ErrorKind::Provider,
        ErrorKind::External,
        ErrorKind::Conflict,
        ErrorKind::Timeout,
        ErrorKind::Cancelled,
        ErrorKind::Safety,
        ErrorKind::Stream,
        ErrorKind::Resource,
    ];

    /// The kind's name, spelled as spec §16.1 spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorKind::Resolution => "resolution",
            ErrorKind::Permission => "permission",
            ErrorKind::Io => "io",
            ErrorKind::Parse => "parse",
            ErrorKind::Type => "type",
            ErrorKind::Provider => "provider",
            ErrorKind::External => "external",
            ErrorKind::Conflict => "conflict",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Safety => "safety",
            ErrorKind::Stream => "stream",
            ErrorKind::Resource => "resource",
        }
    }

    /// Resolves a kind from its name, or `None` if no kind has that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.as_str() == name)
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Builds [`ErrorCode`] together with its number, selector and kind, so the three can never
/// drift apart and the exhaustive lists stay derived rather than restated.
macro_rules! error_codes {
    ($( $variant:ident => $number:literal, $name:literal, $kind:ident, $doc:literal; )*) => {
        /// A stable, machine-readable error identity from spec §43.
        ///
        /// The rendered form is `Ono-Sendai-ENNNN`; the selector is the dotted `family.detail`
        /// name that `docs/contracts/errors.yaml`, `try`/`catch` and predicates over error values
        /// match on.
        ///
        /// ```
        /// use ono_core::{ErrorCode, ErrorKind};
        /// assert_eq!(ErrorCode::ParseSyntax.code(), "Ono-Sendai-E0001");
        /// assert_eq!(ErrorCode::ParseSyntax.name(), "parse.syntax");
        /// assert_eq!(ErrorCode::ParseSyntax.kind(), ErrorKind::Parse);
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum ErrorCode {
            $( #[doc = $doc] $variant, )*
        }

        impl ErrorCode {
            /// Every code of the taxonomy, in numeric order.
            pub const ALL: &'static [ErrorCode] = &[ $( ErrorCode::$variant, )* ];

            /// The rendered code, e.g. `Ono-Sendai-E0001`.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self { $( ErrorCode::$variant => $number, )* }
            }

            /// The dotted selector, e.g. `parse.syntax`.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self { $( ErrorCode::$variant => $name, )* }
            }

            /// The kind this code belongs to (ADR-0006).
            #[must_use]
            pub const fn kind(self) -> ErrorKind {
                match self { $( ErrorCode::$variant => ErrorKind::$kind, )* }
            }
        }
    };
}

error_codes! {
    ParseSyntax => "Ono-Sendai-E0001", "parse.syntax", Parse,
        "The input is not valid Ono syntax.";
    ParseIncomplete => "Ono-Sendai-E0002", "parse.incomplete", Parse,
        "The input ended in the middle of a construct; more text would complete it.";
    ResolveCommandNotFound => "Ono-Sendai-E0101", "resolve.command_not_found", Resolution,
        "No keyword, function, native command or executable answers to this name.";
    ResolveTargetNotFound => "Ono-Sendai-E0102", "resolve.target_not_found", Resolution,
        "The verb is known but has no such target.";
    ResolveAmbiguous => "Ono-Sendai-E0103", "resolve.ambiguous", Resolution,
        "The name matches more than one candidate and no namespace was given.";
    TypeMismatch => "Ono-Sendai-E0201", "type.mismatch", Type,
        "A value did not have the type the operation requires.";
    TypeUnknownField => "Ono-Sendai-E0202", "type.unknown_field", Type,
        "A record schema has no such field.";
    TypeInvalidUnit => "Ono-Sendai-E0203", "type.invalid_unit", Type,
        "A unit was unknown, or two incompatible dimensions were compared.";
    IoNotFound => "Ono-Sendai-E0301", "io.not_found", Io,
        "The path or resource does not exist.";
    IoPermissionDenied => "Ono-Sendai-E0302", "io.permission_denied", Permission,
        "The operating system refused access to the resource.";
    IoAlreadyExists => "Ono-Sendai-E0303", "io.already_exists", Io,
        "The resource already exists and would have been overwritten.";
    IoNotDirectory => "Ono-Sendai-E0304", "io.not_directory", Io,
        "A path component is not a directory.";
    ProviderUnavailable => "Ono-Sendai-E0401", "provider.unavailable", Provider,
        "The provider cannot answer on this system right now.";
    ProviderUnsupported => "Ono-Sendai-E0402", "provider.unsupported", Provider,
        "The provider does not implement this capability.";
    ProviderSchemaViolation => "Ono-Sendai-E0403", "provider.schema_violation", Provider,
        "A provider emitted a value outside the schema it advertises.";
    ProviderInconclusive => "Ono-Sendai-E0404", "provider.inconclusive", Provider,
        "The answer is empty or incomplete, and that establishes nothing about the system.";
    ProviderAuthenticationFailed => "Ono-Sendai-E0405", "provider.authentication_failed", Permission,
        "The external system did not accept the credential the provider presented.";
    ProviderAuthorizationDenied => "Ono-Sendai-E0406", "provider.authorization_denied", Permission,
        "The external system refused the operation to the identity the provider presented.";
    ProviderRateLimited => "Ono-Sendai-E0407", "provider.rate_limited", Timeout,
        "The external system asked the provider to slow down.";
    ExternalExitNonzero => "Ono-Sendai-E0501", "external.exit_nonzero", External,
        "An external process exited with a non-zero status.";
    ExternalSignal => "Ono-Sendai-E0502", "external.signal", External,
        "An external process was terminated by a signal.";
    RemoteUnreachable => "Ono-Sendai-E0601", "remote.unreachable", Provider,
        "The remote link could not be established or was lost.";
    RemoteProtocolMismatch => "Ono-Sendai-E0602", "remote.protocol_mismatch", Provider,
        "The remote peer speaks an incompatible protocol version.";
    RemoteHostKeyChanged => "Ono-Sendai-E0603", "remote.host_key_changed", Safety,
        "The remote host presented a different key than the one recorded for it.";
    RemoteIdentityPermissions => "Ono-Sendai-E0604", "remote.identity_permissions", Safety,
        "The peer identity file is readable or writable by someone other than its owner.";
    RemotePeerUnauthenticated => "Ono-Sendai-E0605", "remote.peer_unauthenticated", Safety,
        "The remote peer did not prove possession of a peer key.";
    SafetyConfirmationRequired => "Ono-Sendai-E0701", "safety.confirmation_required", Safety,
        "The operation needs explicit confirmation that this context cannot ask for.";
    SafetyPolicyDenied => "Ono-Sendai-E0702", "safety.policy_denied", Safety,
        "A configured safety policy forbids the operation.";
    StreamUnboundedOperation => "Ono-Sendai-E0801", "stream.unbounded_operation", Stream,
        "The operation requires bounded input but the stream is unbounded.";
    StreamCancelled => "Ono-Sendai-E0802", "stream.cancelled", Cancelled,
        "The stream was cancelled before it completed.";
    StreamBackpressureTimeout => "Ono-Sendai-E0803", "stream.backpressure_timeout", Timeout,
        "A consumer did not accept values within the configured budget.";
    AdapterNotAvailable => "Ono-Sendai-E0901", "adapter.not_available", Resolution,
        "No adapter answers to this invocation, and structured output was required.";
    AdapterDisabled => "Ono-Sendai-E0902", "adapter.disabled", Permission,
        "An adapter exists for this invocation but is switched off in this context.";
    AdapterUnsupportedInvocation => "Ono-Sendai-E0903", "adapter.unsupported_invocation", Provider,
        "The adapter knows the executable but not this combination of options.";
    AdapterVersionIncompatible => "Ono-Sendai-E0904", "adapter.version_incompatible", Provider,
        "The executable's version is outside the range the adapter was tested with.";
    AdapterExecutableMismatch => "Ono-Sendai-E0905", "adapter.executable_mismatch", Resolution,
        "The executable that resolved is not the one the adapter's contract names.";
    AdapterRewriteFailed => "Ono-Sendai-E0906", "adapter.rewrite_failed", Provider,
        "The adapter could not turn the invocation into its machine-oriented form.";
    AdapterDecodeFailed => "Ono-Sendai-E0907", "adapter.decode_failed", Provider,
        "The executable's output was not what the adapter's decoder expects.";
    AdapterSchemaViolation => "Ono-Sendai-E0908", "adapter.schema_violation", Provider,
        "The adapter decoded a value outside the schema it advertises.";
    AdapterCapabilityDenied => "Ono-Sendai-E0909", "adapter.capability_denied", Permission,
        "The adapter's package is not allowed to run this executable.";
    AdapterConflict => "Ono-Sendai-E0910", "adapter.conflict", Conflict,
        "More than one adapter claims the invocation and the resolution rules cannot separate them.";
    AdapterRequiredForStructuredPipeline => "Ono-Sendai-E0911", "adapter.required_for_structured_pipeline", Type,
        "A consumer demanded objects and no adapter can provide them for this invocation.";

    // --- spatial, spec v0.4 §40: the fourteen conditions of the spatial interface, allocated
    // as the E10xx family in the order §40 lists them (ADR-0125, ADR-0127).
    SpatialNotFound => "Ono-Sendai-E1001", "spatial.not_found", Resolution,
        "No place, object or alias answers to this spatial selector.";
    SpatialAmbiguousSelector => "Ono-Sendai-E1002", "spatial.ambiguous_selector", Resolution,
        "More than one place answers to this selector and none of them is the obvious one.";
    SpatialNotEnterable => "Ono-Sendai-E1003", "spatial.not_enterable", Type,
        "This object exists but is not a place you can stand in.";
    SpatialNoRelation => "Ono-Sendai-E1004", "spatial.no_relation", Resolution,
        "No relation of that name leaves this place.";
    SpatialNoParent => "Ono-Sendai-E1005", "spatial.no_parent", Resolution,
        "This place has no canonical parent, so `up` has nowhere to go.";
    SpatialHistoryEmpty => "Ono-Sendai-E1006", "spatial.history_empty", Conflict,
        "The navigation trail holds no earlier place to return to.";
    SpatialDestinationGone => "Ono-Sendai-E1007", "spatial.destination_gone", Resolution,
        "The place the trail or pin points at no longer exists.";
    SpatialPermissionDenied => "Ono-Sendai-E1008", "spatial.permission_denied", Permission,
        "This user may not see the objects or relations behind this place.";
    SpatialUnsupported => "Ono-Sendai-E1009", "spatial.unsupported", Provider,
        "No installed provider can answer for this place, relation or domain.";
    SpatialStale => "Ono-Sendai-E1010", "spatial.stale", Provider,
        "The index's answer is older than this operation is allowed to accept.";
    SpatialRemoteUnavailable => "Ono-Sendai-E1011", "spatial.remote_unavailable", Provider,
        "The remote host this place belongs to cannot be reached.";
    SpatialScopeViolation => "Ono-Sendai-E1012", "spatial.scope_violation", Permission,
        "The movement would cross a scope boundary this session may not cross.";
    SpatialMapTooLarge => "Ono-Sendai-E1013", "spatial.map_too_large", Stream,
        "The requested map exceeds its node budget and cannot be drawn honestly.";
    SpatialCostRefused => "Ono-Sendai-E1401", "spatial.cost_refused", Resource,
        "The planner estimated this query beyond the interactive budget and refused it.";
    SpatialIdentityConflict => "Ono-Sendai-E1014", "spatial.identity_conflict", Conflict,
        "Two observations claim to be the same object and their identities disagree.";

    // --- resource budgets, v0.4.1 §21.4: the three refusals a byte-bounded shell owes automation
    // (ADR-0453). The block after spatial, which ADR-0125 left free for the next family.
    ResourceItemLimit => "Ono-Sendai-E1101", "resource.item_limit", Resource,
        "An operation reached the number of values it is allowed to retain.";
    ResourceByteLimit => "Ono-Sendai-E1102", "resource.byte_limit", Resource,
        "An operation reached the number of bytes it is allowed to retain.";
    ResourceMaterializationLimit => "Ono-Sendai-E1103", "resource.materialization_limit", Resource,
        "An operation that must see its whole input may not materialize this input.";

    // --- server-side client authorization, v0.4.1 §9 and §10.4: the three refusals §53.1 names
    // as one family, plus the store that would not load (ADR-0466). E12xx, after the resource
    // block ADR-0453 opened.
    RemoteUnauthenticated => "Ono-Sendai-E1201", "remote.unauthenticated", Safety,
        "The connecting client proved possession of no key, so there is no identity to authorize.";
    RemoteUnauthorized => "Ono-Sendai-E1202", "remote.unauthorized", Safety,
        "The client is authenticated but is not listed in the agent's authorization store.";
    RemoteCapabilityDenied => "Ono-Sendai-E1203", "remote.capability_denied", Safety,
        "The client is authorized, but not for the capability this request needs.";
    RemoteAuthorizationStoreInvalid => "Ono-Sendai-E1204", "remote.authorization_store_invalid", Safety,
        "The authorization store could not be loaded, so nothing is authorized.";

    // --- listening-agent resource limits, v0.4.1 §12: the ceilings an exposed agent enforces on
    // everyone who reaches it (ADR-0502). E15xx, the block H3 reserved.
    RemoteConnectionLimit => "Ono-Sendai-E1501", "remote.connection_limit", Resource,
        "The listening agent is already holding as many connections as it may.";
    RemoteHandshakeTimeout => "Ono-Sendai-E1502", "remote.handshake_timeout", Timeout,
        "The peer did not complete TLS and Ono negotiation within the time a handshake is given.";

    // --- plugin references, K11P §10, §11, §24.1 (ADR-0601, ADR-0605) -----------------------
    PluginNotFound => "Ono-Sendai-E1601", "plugin.not_found", Resolution,
        "No installed package and no catalog entry answers to the plugin reference.";
    PluginReferenceAmbiguous => "Ono-Sendai-E1602", "plugin.reference_ambiguous", Resolution,
        "The short name resolves to more than one canonical package.";
    PluginCatalogUnavailable => "Ono-Sendai-E1603", "plugin.catalog_unavailable", Resolution,
        "The catalog names the release and its artifact is not available to this build.";
    PluginReleaseNotCompatible => "Ono-Sendai-E1604", "plugin.release_not_compatible", Conflict,
        "No release of the package the catalog offers can run on this host.";
    PluginSourceConflict => "Ono-Sendai-E1605", "plugin.source_conflict", Conflict,
        "Two acquisition sources offer the same package version with different content.";
    PluginTransportRefused => "Ono-Sendai-E1606", "plugin.transport_refused", Safety,
        "A plugin artifact would travel over a transport policy does not allow.";
    PluginSourceRootRejected => "Ono-Sendai-E1607", "plugin.source_root_rejected", Safety,
        "A system source root is not read-only to ordinary users and was not searched.";

    // --- temporal, v0.5 §34: the family a shell owes a user who is standing in the past, and
    // the one that says why a history cannot be read. v0.5 §34 numbers them E1101–E1114, which
    // v0.4.1 §21.4 had already spent on the resource family; ADR-0610 keeps every name §34 fixes
    // and opens E13, the lowest free block, so a reader adds two hundred to the section's table.
    // Unknown cause is deliberately not a code: §16.7 makes it a successful explanation.
    TemporalInvalidTime => "Ono-Sendai-E1301", "temporal.invalid_time", Parse,
        "Time selector cannot resolve unambiguously.";
    TemporalNotRecorded => "Ono-Sendai-E1302", "temporal.not_recorded", Resolution,
        "No evidence source covers the requested time or scope.";
    TemporalOutOfRetention => "Ono-Sendai-E1303", "temporal.out_of_retention", Resolution,
        "Requested history is known to have expired.";
    TemporalReadOnly => "Ono-Sendai-E1304", "temporal.read_only", Safety,
        "Mutation attempted while the session observes historical state.";
    TemporalPresentOnly => "Ono-Sendai-E1305", "temporal.present_only", Safety,
        "An operation bound to the present was attempted in historical context.";
    TemporalAmbiguousEvent => "Ono-Sendai-E1306", "temporal.ambiguous_event", Resolution,
        "A query resolves to more than one equally valid event.";
    TemporalStoreUnavailable => "Ono-Sendai-E1307", "temporal.store_unavailable", Io,
        "The persistent temporal store cannot be accessed.";
    TemporalStoreCorrupt => "Ono-Sendai-E1308", "temporal.store_corrupt", Io,
        "Ledger integrity failure detected.";
    TemporalPermissionDenied => "Ono-Sendai-E1309", "temporal.permission_denied", Permission,
        "History or evidence exists and is not accessible to this session.";
    TemporalUnsupportedSource => "Ono-Sendai-E1310", "temporal.unsupported_source", Provider,
        "The source cannot provide the requested temporal capability.";
    TemporalCoverageGap => "Ono-Sendai-E1311", "temporal.coverage_gap", Provider,
        "The operation requires an interval with a material unsupported gap.";
    TemporalClockUncertain => "Ono-Sendai-E1312", "temporal.clock_uncertain", Provider,
        "The requested strict ordering cannot be established from the evidence.";
    TemporalRecorderNotRunning => "Ono-Sendai-E1313", "temporal.recorder_not_running", Conflict,
        "The operation requires a running recorder.";
    TemporalRecorderAlreadyRunning => "Ono-Sendai-E1314", "temporal.recorder_already_running", Conflict,
        "A start was requested while the recorder is already active.";

    // --- prospective change, v0.6 §45: the family a shell owes a user who asked what a change
    // would do before it did it, and the one that says why a way back is not available. §45
    // fixes the names and leaves the numbers open; E17, E18 and E19 are the lowest free blocks
    // (ADR-0801). Three groups, and the split is the specification's: `change.*` is about the
    // plan, `recovery.*` about the way back, `transaction.*` about a guarantee nobody has.
    ChangePlanNotSealed => "Ono-Sendai-E1701", "change.plan_not_sealed", Conflict,
        "A plan that is not sealed was given to a command that acts on one.";
    ChangePlanSealed => "Ono-Sendai-E1702", "change.plan_sealed", Conflict,
        "A sealed plan cannot be edited.";
    ChangePlanExpired => "Ono-Sendai-E1703", "change.plan_expired", Conflict,
        "The sealed plan's validity window has closed.";
    ChangePlanAlreadyApplying => "Ono-Sendai-E1704", "change.plan_already_applying", Conflict,
        "Another session is applying this plan.";
    ChangePlanDriftDetected => "Ono-Sendai-E1705", "change.plan_drift_detected", Conflict,
        "A precondition the plan froze no longer holds.";
    ChangePlanNotFound => "Ono-Sendai-E1706", "change.plan_not_found", Resolution,
        "No plan matches the reference.";
    ChangePlanReferenceAmbiguous => "Ono-Sendai-E1707", "change.plan_reference_ambiguous", Resolution,
        "The reference matches more than one plan.";
    ChangePlanStateInvalid => "Ono-Sendai-E1708", "change.plan_state_invalid", Conflict,
        "The lifecycle transition is not one v0.6 §4.1 draws.";
    ChangePlanStoreUnavailable => "Ono-Sendai-E1709", "change.plan_store_unavailable", Io,
        "The plan store cannot be accessed.";
    ChangePlanStoreCorrupt => "Ono-Sendai-E1710", "change.plan_store_corrupt", Io,
        "The plan store failed its integrity check.";
    ChangeTargetUnresolved => "Ono-Sendai-E1711", "change.target_unresolved", Resolution,
        "A selector did not resolve to a concrete object.";
    ChangeTargetChanged => "Ono-Sendai-E1712", "change.target_changed", Conflict,
        "A frozen target is no longer the object the plan resolved.";
    ChangeActionNotPlannable => "Ono-Sendai-E1713", "change.action_not_plannable", Provider,
        "The operation has no provider contract v0.6 can plan from.";
    ChangeOpaqueActionForbidden => "Ono-Sendai-E1714", "change.opaque_action_forbidden", Safety,
        "An opaque external command cannot be planned by default.";
    ChangeHistoricalContextReadOnly => "Ono-Sendai-E1715", "change.historical_context_read_only", Safety,
        "A plan for application cannot be resolved against historical state.";
    ChangePreconditionFailed => "Ono-Sendai-E1716", "change.precondition_failed", Conflict,
        "A declared precondition of an action did not hold.";
    ChangePrepareFailed => "Ono-Sendai-E1717", "change.prepare_failed", Provider,
        "Preparation failed, and no mutating action ran.";
    ChangeApplyFailed => "Ono-Sendai-E1718", "change.apply_failed", Provider,
        "A mutating action failed.";
    ChangeVerificationFailed => "Ono-Sendai-E1719", "change.verification_failed", Provider,
        "A required postcondition did not hold after the change.";
    ChangeVerificationUnknown => "Ono-Sendai-E1720", "change.verification_unknown", Provider,
        "A verification check could not be answered.";
    ChangeVerificationMissing => "Ono-Sendai-E1721", "change.verification_missing", Conflict,
        "A plan containing a mutating action carries no verification contract.";
    ChangeIrreversibleNotAccepted => "Ono-Sendai-E1722", "change.irreversible_not_accepted", Safety,
        "The plan contains an irreversible action that has not been acknowledged.";
    ChangeRiskNotAccepted => "Ono-Sendai-E1723", "change.risk_not_accepted", Safety,
        "The plan's risk class requires an acknowledgement that has not been given.";
    ChangeBulkGuardFailed => "Ono-Sendai-E1724", "change.bulk_guard_failed", Safety,
        "A bulk plan reaches further than its configured guard permits.";
    ChangeRemoteStateUnknown => "Ono-Sendai-E1725", "change.remote_state_unknown", Provider,
        "The state of a remote action could not be established.";
    ChangeAutoRecoveryRejected => "Ono-Sendai-E1726", "change.auto_recovery_rejected", Safety,
        "The plan declares automatic recovery under conditions v0.6 does not permit.";
    ChangePrivilegeRequired => "Ono-Sendai-E1727", "change.privilege_required", Permission,
        "An action or its recovery needs privilege this session does not hold.";
    ChangeCapabilityMissing => "Ono-Sendai-E1728", "change.capability_missing", Permission,
        "The session does not hold the capability an action or its recovery needs.";
    ChangeResumeRefused => "Ono-Sendai-E1729", "change.resume_refused", Safety,
        "An interrupted plan cannot be resumed in the state it was left in.";

    // --- recovery, v0.6 §45: what a recovery provider, an asset or a restore could not do.
    RecoveryProviderUnavailable => "Ono-Sendai-E1801", "recovery.provider_unavailable", Provider,
        "A recovery provider the plan needs cannot run here.";
    RecoveryAssetCreateFailed => "Ono-Sendai-E1802", "recovery.asset_create_failed", Provider,
        "A recovery asset could not be created.";
    RecoveryAssetInvalid => "Ono-Sendai-E1803", "recovery.asset_invalid", Provider,
        "A recovery asset exists and cannot satisfy protection.";
    RecoveryAssetExpired => "Ono-Sendai-E1804", "recovery.asset_expired", Resolution,
        "The recovery asset's retention has passed.";
    RecoveryAssetNotFound => "Ono-Sendai-E1805", "recovery.asset_not_found", Resolution,
        "No recovery asset matches the reference.";
    RecoveryScopeMismatch => "Ono-Sendai-E1806", "recovery.scope_mismatch", Conflict,
        "A recovery asset does not cover the object it was expected to.";
    RecoveryCoverageInsufficient => "Ono-Sendai-E1807", "recovery.coverage_insufficient", Safety,
        "The plan cannot reach the protection its policy requires.";
    RecoveryConsistencyUnknown => "Ono-Sendai-E1808", "recovery.consistency_unknown", Provider,
        "The consistency of a captured state could not be established.";
    RecoveryNewerStateConflict => "Ono-Sendai-E1809", "recovery.newer_state_conflict", Conflict,
        "Recovery would discard state written after the recovery point.";
    RecoveryDestructiveHistoryNotAccepted => "Ono-Sendai-E1810", "recovery.destructive_history_not_accepted", Safety,
        "Recovery would destroy newer snapshots, bookmarks or clones, and that was not accepted.";
    RecoveryRequiresOffline => "Ono-Sendai-E1811", "recovery.requires_offline", Conflict,
        "Recovery needs the filesystem unmounted or the system offline.";
    RecoveryRequiresReboot => "Ono-Sendai-E1812", "recovery.requires_reboot", Conflict,
        "Recovery needs a reboot to take effect.";
    RecoveryApplyFailed => "Ono-Sendai-E1813", "recovery.apply_failed", Provider,
        "A recovery action failed.";
    RecoveryVerificationFailed => "Ono-Sendai-E1814", "recovery.verification_failed", Provider,
        "Recovery verification did not hold.";
    RecoveryCleanupBlocked => "Ono-Sendai-E1815", "recovery.cleanup_blocked", Safety,
        "Removing this asset would take away recovery a retained plan still offers.";
    RecoveryStoragePressure => "Ono-Sendai-E1816", "recovery.storage_pressure", Resource,
        "Creating this asset would push storage below the configured floor.";
    RecoveryAssetStale => "Ono-Sendai-E1817", "recovery.asset_stale", Conflict,
        "The recovery asset no longer reflects the state that is about to change.";
    RecoveryQuiesceFailed => "Ono-Sendai-E1818", "recovery.quiesce_failed", Provider,
        "An application could not be quiesced for an application-consistent asset.";
    RecoveryResumeFailed => "Ono-Sendai-E1819", "recovery.resume_failed", Provider,
        "An application was quiesced and could not be resumed.";
    RecoveryPlanIncomplete => "Ono-Sendai-E1820", "recovery.plan_incomplete", Safety,
        "A recovery fact required before destructive recovery could not be established.";

    // --- transaction, v0.6 §45: the two refusals that keep §27's word honest.
    TransactionAtomicityUnavailable => "Ono-Sendai-E1901", "transaction.atomicity_unavailable", Provider,
        "A transaction was requested from a provider that does not offer one.";
    TransactionCrossProviderNotAtomic => "Ono-Sendai-E1902", "transaction.cross_provider_not_atomic", Provider,
        "A change spanning several providers cannot be atomic.";
    // --- KUANG/11, spec §31.79: the K11 family of docs/contracts/kuang/errors.v1.yaml, folded into
    // the global model (ADR-0108). Numbering follows §31.79's families.
    KuangPackageInvalid => "Ono-Sendai-K11001", "package.invalid", Parse,
        "The package manifest is not a valid `kuang-package/1` document.";
    KuangPackageIncompatible => "Ono-Sendai-K11002", "package.incompatible", Conflict,
        "The package requires a host, platform or API version this system does not provide.";
    KuangPackageIntegrityFailed => "Ono-Sendai-K11003", "package.integrity_failed", Safety,
        "The package's bytes do not match the hash the reference named.";
    KuangPackageSignatureInvalid => "Ono-Sendai-K11004", "package.signature_invalid", Safety,
        "The package carries a signature and it does not verify.";
    KuangPublisherUntrusted => "Ono-Sendai-K11005", "publisher.untrusted", Safety,
        "The signing key or publisher is not one this system trusts.";
    KuangLoadCapabilityDenied => "Ono-Sendai-K11101", "load.capability_denied", Permission,
        "A capability the package declares as required was not granted.";
    KuangLoadDependencyMissing => "Ono-Sendai-K11102", "load.dependency_missing", Resolution,
        "A declared package or schema dependency could not be resolved.";
    KuangLoadDependencyCycle => "Ono-Sendai-K11103", "load.dependency_cycle", Conflict,
        "The package's dependencies form a cycle.";
    KuangLoadRuntimeUnavailable => "Ono-Sendai-K11104", "load.runtime_unavailable", Provider,
        "The isolation tier the package declares is not available on this host.";
    KuangRuntimeTrap => "Ono-Sendai-K11201", "runtime.trap", External,
        "The plugin instance trapped or crashed.";
    KuangRuntimeTimeout => "Ono-Sendai-K11202", "runtime.timeout", Timeout,
        "A host call or invocation exceeded its deadline.";
    KuangRuntimeMemoryLimit => "Ono-Sendai-K11203", "runtime.memory_limit", External,
        "The plugin instance exceeded its memory ceiling and was terminated.";
    KuangRuntimeProtocolViolation => "Ono-Sendai-K11204", "runtime.protocol_violation", Provider,
        "The plugin sent a message that is not valid under the negotiated host API.";
    KuangRuntimeSchemaViolation => "Ono-Sendai-K11205", "runtime.schema_violation", Provider,
        "The plugin emitted a value outside the schema its contribution advertises.";
    KuangRuntimeBackpressureFailure => "Ono-Sendai-K11206", "runtime.backpressure_failure", Stream,
        "A stream could not keep up and its policy was to fail rather than lose data.";
    KuangRuntimeConcurrencyLimit => "Ono-Sendai-K11207", "runtime.concurrency_limit", Safety,
        "The plugin already has as many invocations open as its contract allows.";
    KuangCapabilityDenied => "Ono-Sendai-K11301", "capability.denied", Permission,
        "The plugin asked for something it has not been granted.";
    KuangCapabilityScopeViolation => "Ono-Sendai-K11302", "capability.scope_violation", Permission,
        "The plugin holds the capability but the call fell outside its granted scope.";
    KuangCapabilityLeaseExpired => "Ono-Sendai-K11303", "capability.lease_expired", Permission,
        "The lease backing this call has expired, been used up, or its condition no longer holds.";
    KuangPermissionRequired => "Ono-Sendai-K11304", "permission.required", Permission,
        "The operation needs a permission nobody has decided, and this context cannot ask.";
    KuangPermissionDenied => "Ono-Sendai-K11305", "permission.denied", Permission,
        "The operation needs a permission the user denied.";
    KuangPermissionInvalidProfile => "Ono-Sendai-K11306", "permission.invalid_profile", Resolution,
        "The named access profile does not exist for this package.";
    KuangPermissionInvalidMapping => "Ono-Sendai-K11307", "permission.invalid_mapping", Parse,
        "The package's permission declarations do not map validly onto its capabilities.";
    KuangPermissionScopeUnavailable => "Ono-Sendai-K11308", "permission.scope_unavailable", Safety,
        "The concrete scope a permission needs could not be derived or enforced.";
    KuangPermissionEscalationRequiresConfirmation => "Ono-Sendai-K11309", "permission.escalation_requires_confirmation", Safety,
        "The permission widens authority and needs a deliberate confirmation this context cannot give.";
    KuangStateQuotaExceeded => "Ono-Sendai-K11401", "state.quota_exceeded", Safety,
        "The plugin's persistent state would exceed its quota.";
    KuangStateMigrationFailed => "Ono-Sendai-K11402", "state.migration_failed", External,
        "A plugin state migration did not complete.";
    KuangViewProtocolError => "Ono-Sendai-K11501", "view.protocol_error", Provider,
        "The plugin submitted a view tree the host cannot lay out.";
    KuangModelProviderUnavailable => "Ono-Sendai-K11601", "model.provider_unavailable", Provider,
        "No configured model provider satisfies the request.";
    KuangModelPolicyDenied => "Ono-Sendai-K11602", "model.policy_denied", Safety,
        "The request carries a data class this provider is not allowed to receive.";
    KuangAssistantToolInvalid => "Ono-Sendai-K11603", "assistant.tool_invalid", Type,
        "A tool intent named an unexposed tool, or arguments that do not fit its descriptor.";
    KuangAssistantContextDenied => "Ono-Sendai-K11604", "assistant.context_denied", Permission,
        "The assistant requested a context source it has not been granted.";
    KuangRemoteExtensionUnavailable => "Ono-Sendai-K11701", "remote.extension_unavailable", Provider,
        "The remote agent cannot run the requested extension component.";
    KuangRemotePolicyDenied => "Ono-Sendai-K11702", "remote.policy_denied", Safety,
        "The remote host's policy denies the capability, whatever the local grant says.";
    KuangPluginConfinementFailed => "Ono-Sendai-K11801", "plugin.confinement_failed", Safety,
        "A mandatory confinement control could not be installed, so the plugin was not started.";
    KuangPluginResourceLimitFailed => "Ono-Sendai-K11802", "plugin.resource_limit_failed", Safety,
        "A mandatory resource limit could not be installed, so the plugin was not started.";
    KuangPluginNoNewPrivsFailed => "Ono-Sendai-K11803", "plugin.no_new_privs_failed", Safety,
        "`PR_SET_NO_NEW_PRIVS` could not be installed, so the plugin was not started.";
    KuangContributionRefused => "Ono-Sendai-K11901", "contribution.refused", Safety,
        "The contribution declined to act under a safety rule of its own.";
}

impl ErrorCode {
    /// Resolves a code from its dotted selector, or `None` if no code has that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|code| code.name() == name)
    }

    /// Resolves a code from its rendered form, e.g. `Ono-Sendai-E0001`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|c| c.code() == code)
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code(), self.name())
    }
}
