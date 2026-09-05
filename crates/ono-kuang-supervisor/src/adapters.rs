//! Declarative adapter packages (spec v0.3 §1.22, §1.26, §1.44, §1.45, ADR-0065): a package
//! whose roles include `adapter` ships packs under `contributions.adapters`, and the host
//! checks them against the manifest before any of them may answer a negotiation.

use std::path::Path;

use ono_adapter::{AdapterPack, Tier};
use ono_core::ErrorCode;
use ono_kuang_protocol::{Capability, Manifest, Role};

/// Why an adapter package cannot be loaded, with the shell's error code for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterPackageError {
    /// The code the shell reports.
    pub code: ErrorCode,
    /// What is wrong, in one sentence.
    pub message: String,
}

impl std::fmt::Display for AdapterPackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// The executables the manifest's `process.exec` requests name, across required and
/// optional requests.
#[must_use]
pub fn declared_executables(manifest: &Manifest) -> Vec<String> {
    manifest
        .required_capabilities
        .iter()
        .chain(&manifest.optional_capabilities)
        .filter(|request| request.capability == Capability::ProcessExec)
        .filter_map(|request| request.scope.as_ref())
        .filter_map(|scope| scope.get("executables"))
        .filter_map(|value| value.as_array())
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect()
}

/// Reads and checks every adapter pack a package contributes.
///
/// The packs must parse, satisfy `docs/contracts/adapters/schema.yaml` with the package directory
/// as their fixture root, carry the manifest's own id, claim a third-party tier, and name no
/// executable outside the manifest's `process.exec` `executables` scope — the last is
/// `adapter.capability_denied`, because a pack that could run more than the user was shown
/// would be the bypass spec v0.3 §1.22 forbids.
///
/// # Errors
///
/// The first problem found, with the shell's code for it.
pub fn validate_package(
    directory: &Path,
    manifest: &Manifest,
) -> Result<Vec<AdapterPack>, AdapterPackageError> {
    let invalid = |message: String| AdapterPackageError {
        code: ErrorCode::ProviderSchemaViolation,
        message,
    };
    if !manifest.roles.contains(&Role::Adapter) {
        return Err(invalid(format!(
            "`{}` contributes adapter packs but does not declare the `adapter` role",
            manifest.package.id
        )));
    }
    let paths = manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.adapters.clone())
        .unwrap_or_default();
    if paths.is_empty() {
        return Err(invalid(format!(
            "`{}` declares the adapter role but contributes no packs",
            manifest.package.id
        )));
    }
    let granted = declared_executables(manifest);
    let mut packs = Vec::new();
    for relative in paths {
        let path = directory.join(&relative);
        let text = std::fs::read_to_string(&path)
            .map_err(|error| invalid(format!("{relative}: cannot be read: {error}")))?;
        let pack = AdapterPack::parse(&text)
            .map_err(|error| invalid(format!("{relative}: not an adapter pack: {error}")))?;
        if pack.id() != manifest.package.id {
            return Err(invalid(format!(
                "{relative}: the pack is `{}`, the package is `{}`",
                pack.id(),
                manifest.package.id
            )));
        }
        if !matches!(pack.tier(), Tier::Community | Tier::Experimental) {
            return Err(invalid(format!(
                "{relative}: a package cannot claim the first-party or recommended tier for \
                 itself (spec v0.3 §1.27)"
            )));
        }
        let problems = ono_adapter::validate(&pack, ono_value::builtin_schemas(), directory);
        if let Some(problem) = problems.first() {
            return Err(invalid(format!(
                "{relative}: {} — {}",
                problem.location, problem.detail
            )));
        }
        for adapter in pack.adapters() {
            for name in adapter.executable().names() {
                let base = name.rsplit('/').next().unwrap_or(name);
                let covered = granted
                    .iter()
                    .any(|allowed| allowed == name || allowed.rsplit('/').next() == Some(base));
                if !covered {
                    return Err(AdapterPackageError {
                        code: ErrorCode::AdapterCapabilityDenied,
                        message: format!(
                            "adapter `{}` would run `{name}`, which the package's process.exec \
                             `executables` scope does not name (spec v0.3 §1.22)",
                            adapter.full_id()
                        ),
                    });
                }
            }
        }
        packs.push(pack);
    }
    Ok(packs)
}
