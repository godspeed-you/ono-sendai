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
        /// name that `docs/spec/errors.yaml`, `try`/`catch` and predicates over error values
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
