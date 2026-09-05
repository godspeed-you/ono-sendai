//! The KUANG/11 error family of spec §31.79, integrated with the global error model.
//!
//! Every entry is an ordinary structured error: a stable rendered code, the dotted selector
//! `catch` and `where` match on, and one of the twelve kinds of ADR-0006. The rendered form is
//! `Ono-Sendai-K11NNN` (ADR-0022, `Spec deviation`): spec §31.79 writes `ONO-K11001`, but two
//! rendering conventions inside one error registry would be a seam a user meets, so the K-family
//! renders the way every other code in the product does. The dotted names are kept exactly as
//! §31.79 gives them.
//!
//! The codes live here rather than in `ono-core` because Phase I builds KUANG/11 as libraries
//! beside the shell; folding the K-family into `ono_core::ErrorCode` is the parent integration
//! step (ADR-0040).

use std::fmt;

use ono_core::ErrorKind;
use serde::{Deserialize, Serialize};

macro_rules! kuang_error_codes {
    ($( $variant:ident => $code:literal, $name:literal, $kind:ident, $doc:literal; )*) => {
        /// A stable error identity from the KUANG/11 taxonomy (spec §31.79).
        ///
        /// ```
        /// use ono_kuang_protocol::KuangErrorCode;
        /// use ono_core::ErrorKind;
        /// assert_eq!(KuangErrorCode::PackageInvalid.code(), "Ono-Sendai-K11001");
        /// assert_eq!(KuangErrorCode::PackageInvalid.name(), "package.invalid");
        /// assert_eq!(KuangErrorCode::PackageInvalid.kind(), ErrorKind::Parse);
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[non_exhaustive]
        pub enum KuangErrorCode {
            $( #[doc = $doc] $variant, )*
        }

        impl KuangErrorCode {
            /// Every code of the taxonomy, in numeric order.
            pub const ALL: &'static [KuangErrorCode] = &[ $( KuangErrorCode::$variant, )* ];

            /// The rendered code, e.g. `Ono-Sendai-K11001`.
            #[must_use]
            pub const fn code(self) -> &'static str {
                match self { $( KuangErrorCode::$variant => $code, )* }
            }

            /// The dotted selector, e.g. `package.invalid`, exactly as spec §31.79 spells it.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self { $( KuangErrorCode::$variant => $name, )* }
            }

            /// The kind this code belongs to (ADR-0006, mapped by ADR-0022 §13).
            #[must_use]
            pub const fn kind(self) -> ErrorKind {
                match self { $( KuangErrorCode::$variant => ErrorKind::$kind, )* }
            }
        }
    };
}

kuang_error_codes! {
    PackageInvalid => "Ono-Sendai-K11001", "package.invalid", Parse,
        "The package manifest is not a valid `kuang-package/1` document.";
    PackageIncompatible => "Ono-Sendai-K11002", "package.incompatible", Conflict,
        "The package requires a host, platform or API version this system does not provide.";
    PackageIntegrityFailed => "Ono-Sendai-K11003", "package.integrity_failed", Safety,
        "The package's bytes do not match the hash the reference named.";
    PackageSignatureInvalid => "Ono-Sendai-K11004", "package.signature_invalid", Safety,
        "The package carries a signature and it does not verify.";
    PublisherUntrusted => "Ono-Sendai-K11005", "publisher.untrusted", Safety,
        "The signing key or publisher is not one this system trusts.";
    LoadCapabilityDenied => "Ono-Sendai-K11101", "load.capability_denied", Permission,
        "A capability the package declares as required was not granted.";
    LoadDependencyMissing => "Ono-Sendai-K11102", "load.dependency_missing", Resolution,
        "A declared package or schema dependency could not be resolved.";
    LoadDependencyCycle => "Ono-Sendai-K11103", "load.dependency_cycle", Conflict,
        "The package's dependencies form a cycle.";
    LoadRuntimeUnavailable => "Ono-Sendai-K11104", "load.runtime_unavailable", Provider,
        "The isolation tier the package declares is not available on this host.";
    RuntimeTrap => "Ono-Sendai-K11201", "runtime.trap", External,
        "The plugin instance trapped or crashed.";
    RuntimeTimeout => "Ono-Sendai-K11202", "runtime.timeout", Timeout,
        "A host call or invocation exceeded its deadline.";
    RuntimeMemoryLimit => "Ono-Sendai-K11203", "runtime.memory_limit", External,
        "The plugin instance exceeded its memory ceiling and was terminated.";
    RuntimeProtocolViolation => "Ono-Sendai-K11204", "runtime.protocol_violation", Provider,
        "The plugin sent a message that is not valid under the negotiated host API.";
    RuntimeSchemaViolation => "Ono-Sendai-K11205", "runtime.schema_violation", Provider,
        "The plugin emitted a value outside the schema its contribution advertises.";
    RuntimeBackpressureFailure => "Ono-Sendai-K11206", "runtime.backpressure_failure", Stream,
        "A stream could not keep up and its policy was to fail rather than lose data.";
    CapabilityDenied => "Ono-Sendai-K11301", "capability.denied", Permission,
        "The plugin asked for something it has not been granted.";
    CapabilityScopeViolation => "Ono-Sendai-K11302", "capability.scope_violation", Permission,
        "The plugin holds the capability but the call fell outside its granted scope.";
    CapabilityLeaseExpired => "Ono-Sendai-K11303", "capability.lease_expired", Permission,
        "The lease backing this call has expired, been used up, or its condition no longer holds.";
    StateQuotaExceeded => "Ono-Sendai-K11401", "state.quota_exceeded", Safety,
        "The plugin's persistent state would exceed its quota.";
    StateMigrationFailed => "Ono-Sendai-K11402", "state.migration_failed", External,
        "A plugin state migration did not complete.";
    ViewProtocolError => "Ono-Sendai-K11501", "view.protocol_error", Provider,
        "The plugin submitted a view tree the host cannot lay out.";
    ModelProviderUnavailable => "Ono-Sendai-K11601", "model.provider_unavailable", Provider,
        "No configured model provider satisfies the request.";
    ModelPolicyDenied => "Ono-Sendai-K11602", "model.policy_denied", Safety,
        "The request carries a data class this provider is not allowed to receive.";
    AssistantToolInvalid => "Ono-Sendai-K11603", "assistant.tool_invalid", Type,
        "A tool intent named an unexposed tool, or arguments that do not fit its descriptor.";
    AssistantContextDenied => "Ono-Sendai-K11604", "assistant.context_denied", Permission,
        "The assistant requested a context source it has not been granted.";
    RemoteExtensionUnavailable => "Ono-Sendai-K11701", "remote.extension_unavailable", Provider,
        "The remote agent cannot run the requested extension component.";
    RemotePolicyDenied => "Ono-Sendai-K11702", "remote.policy_denied", Safety,
        "The remote host's policy denies the capability, whatever the local grant says.";
    PluginConfinementFailed => "Ono-Sendai-K11801", "plugin.confinement_failed", Safety,
        "A mandatory confinement control could not be installed, so the plugin was not started.";
    PluginResourceLimitFailed => "Ono-Sendai-K11802", "plugin.resource_limit_failed", Safety,
        "A mandatory resource limit could not be installed, so the plugin was not started.";
    PluginNoNewPrivsFailed => "Ono-Sendai-K11803", "plugin.no_new_privs_failed", Safety,
        "`PR_SET_NO_NEW_PRIVS` could not be installed, so the plugin was not started.";
}

impl KuangErrorCode {
    /// Resolves a code from its dotted selector, or `None` if no code has that name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|code| code.name() == name)
    }
}

impl fmt::Display for KuangErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.code(), self.name())
    }
}

/// A structured KUANG/11 error: a stable code, a message and machine-readable detail.
///
/// This is the boundary error both sides of the protocol raise and read. It carries the same
/// shape `ono.error/1` promises — code, message, help, metadata — so the parent integration can
/// surface it as an ordinary error value without translation loss (spec §31.79, §43).
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub struct KuangError {
    code: KuangErrorCode,
    message: String,
    help: Option<String>,
    // Boxed so the whole error stays small enough to travel in a `Result` without a lint about
    // it: the metadata is empty on almost every error, and a rarely-filled map does not belong
    // inline in every `Err` the protocol returns.
    metadata: Box<serde_json::Map<String, serde_json::Value>>,
}

impl KuangError {
    /// Creates an error with a stable code and a human message.
    #[must_use]
    pub fn new(code: KuangErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            help: None,
            metadata: Box::default(),
        }
    }

    /// Adds the second line a user sees: what to do about it.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Attaches a machine-readable detail, such as the attempted value beside the granted scope.
    #[must_use]
    pub fn with_metadata(mut self, key: &str, value: serde_json::Value) -> Self {
        self.metadata.insert(key.to_owned(), value);
        self
    }

    /// The stable code.
    #[must_use]
    pub const fn code(&self) -> KuangErrorCode {
        self.code
    }

    /// The kind the code belongs to (ADR-0006). Scripts branch on this.
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        self.code.kind()
    }

    /// The human message, without help.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// The suggestion shown under the message.
    #[must_use]
    pub fn help(&self) -> Option<&str> {
        self.help.as_deref()
    }

    /// The machine-readable details.
    #[must_use]
    pub fn metadata(&self) -> &serde_json::Map<String, serde_json::Value> {
        &self.metadata
    }
}

impl fmt::Display for KuangError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.name(), self.message)?;
        if let Some(help) = &self.help {
            write!(f, "\n{help}")?;
        }
        Ok(())
    }
}

/// An error as it crosses the wire: codes as strings, so a plugin built against a newer
/// taxonomy can still name a code this host does not know (spec §31.62).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    /// The rendered code, e.g. `Ono-Sendai-K11301`.
    pub code: String,
    /// The dotted selector, e.g. `capability.denied` — what `catch` matches on (ADR-0006).
    pub name: String,
    /// The human message.
    pub message: String,
    /// What to do about it, when the raiser knows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// Machine-readable details.
    ///
    /// Boxed so the whole struct stays small enough to travel in a `Result` without a lint about
    /// it: the map is empty on almost every error, and `preserve_order` makes it an index map
    /// (ADR-0228). The wire form is unchanged — a `Box` serialises as what it holds.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub metadata: Box<serde_json::Map<String, serde_json::Value>>,
}

impl From<&KuangError> for WireError {
    fn from(error: &KuangError) -> Self {
        Self {
            code: error.code().code().to_owned(),
            name: error.code().name().to_owned(),
            message: error.message().to_owned(),
            help: error.help().map(str::to_owned),
            metadata: Box::new(error.metadata().clone()),
        }
    }
}

impl From<KuangError> for WireError {
    fn from(error: KuangError) -> Self {
        Self::from(&error)
    }
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.message)?;
        if let Some(help) = &self.help {
            write!(f, "\n{help}")?;
        }
        Ok(())
    }
}

impl std::error::Error for WireError {}

impl WireError {
    /// A wire error carrying a code from the global taxonomy of spec §43 — the host calls that
    /// answer with `io.not_found` and friends use exactly the codes `docs/contracts/errors.yaml`
    /// already defines, because there is no second error model for extensions (spec §31.79).
    #[must_use]
    pub fn from_core(code: ono_core::ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code: code.code().to_owned(),
            name: code.name().to_owned(),
            message: message.into(),
            help: None,
            metadata: Box::default(),
        }
    }
}

impl WireError {
    /// Reads the wire error back as a [`KuangError`] where the code is one this build knows.
    ///
    /// An unknown selector maps to [`KuangErrorCode::RuntimeProtocolViolation`] with the
    /// original code preserved in the metadata, because an error must never be silently dropped
    /// for being new (spec §31.62).
    #[must_use]
    pub fn to_kuang_error(&self) -> KuangError {
        let mut error = match KuangErrorCode::from_name(&self.name) {
            Some(code) => KuangError::new(code, self.message.clone()),
            None => KuangError::new(
                KuangErrorCode::RuntimeProtocolViolation,
                self.message.clone(),
            )
            .with_metadata("unknown_code", serde_json::Value::String(self.name.clone())),
        };
        if let Some(help) = &self.help {
            error = error.with_help(help.clone());
        }
        for (key, value) in self.metadata.iter() {
            error = error.with_metadata(key, value.clone());
        }
        error
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_render_the_k_family_like_every_other_code_when_displayed() {
        // ADR-0022 `Spec deviation`: `Ono-Sendai-K11001`, never `ONO-K11001`.
        for code in KuangErrorCode::ALL {
            assert!(
                code.code().starts_with("Ono-Sendai-K11"),
                "{} must render in the product's convention",
                code.name()
            );
        }
    }

    #[test]
    fn should_expose_all_27_codes_of_spec_31_79_when_enumerated() {
        // §31.79's families are closed: nothing here is renumbered, removed or re-pointed.
        let inherited = KuangErrorCode::ALL
            .iter()
            .filter(|code| !code.name().starts_with("plugin."))
            .count();
        assert_eq!(inherited, 27);
        // v0.4.1 §16.3 adds a family §31.79 does not have, and names all three of its codes
        // verbatim: a confinement control that could not be installed (ADR-0444).
        let confinement = KuangErrorCode::ALL
            .iter()
            .filter(|code| code.name().starts_with("plugin."))
            .count();
        assert_eq!(confinement, 3);
    }

    #[test]
    fn should_resolve_a_code_from_its_dotted_selector_when_looked_up() {
        assert_eq!(
            KuangErrorCode::from_name("capability.scope_violation"),
            Some(KuangErrorCode::CapabilityScopeViolation)
        );
        assert_eq!(KuangErrorCode::from_name("no.such_code"), None);
    }

    #[test]
    fn should_keep_an_unknown_wire_code_visible_when_reading_it_back() {
        let wire = WireError {
            code: "Ono-Sendai-K11999".to_owned(),
            name: "future.code".to_owned(),
            message: "from a newer taxonomy".to_owned(),
            help: None,
            metadata: Box::default(),
        };
        let error = wire.to_kuang_error();
        assert_eq!(error.code(), KuangErrorCode::RuntimeProtocolViolation);
        assert_eq!(
            error.metadata().get("unknown_code"),
            Some(&serde_json::Value::String("future.code".to_owned()))
        );
    }
}
