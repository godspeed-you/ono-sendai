//! Registry placeholders for what an installed package declares (spec §31.64, §31.68).
//!
//! Spec §31.68: `installed manifest -> registry placeholders -> first invocation -> runtime
//! load`. A package's `contributions.commands` documents say what it contributes; reading them
//! costs a few file reads and starts nothing, so `get command`, `help`, completion and `explain`
//! can answer for a contributed command before any of the package's code has run.
//!
//! The placeholders are built once per process, from the environment the shell started in
//! (ADR-0282). A package installed *during* a session therefore reaches the registry in the next
//! one — the same boundary spec §31.8 already draws between installing and having.

use std::path::PathBuf;
use std::sync::OnceLock;

use ono_command::{CommandContract, CommandRegistry, ContributedCommand, Origin};
use ono_core::ErrorCode;
use ono_kuang_protocol::{CommandDocument, Manifest};
use ono_value::ErrorValue;

use crate::kuang_host::{Installed, packages_under};

/// The command registry this process answers from: the embedded contracts, plus a placeholder
/// for every command an installed, enabled package declares.
///
/// # Errors
///
/// The registry's own error when an embedded contract does not parse — a build defect. A package
/// whose declaration does not read is *not* an error here: it is refused, and the refusal is
/// available through [`refusals`].
pub fn registry() -> Result<&'static CommandRegistry, ErrorValue> {
    if let Some((registry, _)) = EXTENDED.get() {
        return Ok(registry);
    }
    let embedded = CommandRegistry::embedded()?;
    let (declared, mut problems) = declared_commands(&plugin_path());
    let (extended, refused) = embedded.extended(declared);
    problems.extend(refused);
    let (registry, _) = EXTENDED.get_or_init(|| (extended, problems));
    Ok(registry)
}

/// Every contributed declaration this process refused, and why (spec §31.65).
///
/// `get plugin` reports these beside the packages themselves: a declaration the shell would not
/// register must not be silently missing from `get command`.
#[must_use]
pub fn refusals() -> Vec<ErrorValue> {
    let _ = registry();
    EXTENDED
        .get()
        .map(|(_, problems)| problems.clone())
        .unwrap_or_default()
}

static EXTENDED: OnceLock<(CommandRegistry, Vec<ErrorValue>)> = OnceLock::new();

/// The directories `ONO_PLUGIN_PATH` names, or the user's plugin directory.
///
/// The process environment rather than the session's, because the registry is built before the
/// first pipeline runs. `crate::plugins::plugin_path` reads the same two places from the session.
fn plugin_path() -> Vec<PathBuf> {
    if let Some(path) = std::env::var_os("ONO_PLUGIN_PATH") {
        return std::env::split_paths(&path).collect();
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config/ono/plugins"))
        .into_iter()
        .collect()
}

/// The commands every installed package under `plugin_path` declares, and the declarations that
/// did not read.
fn declared_commands(plugin_path: &[PathBuf]) -> (Vec<CommandContract>, Vec<ErrorValue>) {
    let mut commands = Vec::new();
    let mut problems = Vec::new();
    for directory in plugin_path {
        let (packages, failures) = packages_under(directory);
        problems.extend(failures);
        for package in packages {
            let (declared, refused) = declarations(&package);
            commands.extend(declared);
            problems.extend(refused);
        }
    }
    (commands, problems)
}

/// What one package declares, as registry entries attributed to it (spec §31.64).
pub(crate) fn declarations(package: &Installed) -> (Vec<CommandContract>, Vec<ErrorValue>) {
    let mut commands = Vec::new();
    let mut problems = Vec::new();
    let origin = origin_of(&package.manifest);
    for path in declared_paths(&package.manifest) {
        let file = package.directory.join(&path);
        let text = match std::fs::read_to_string(&file) {
            Ok(text) => text,
            Err(error) => {
                problems.push(
                    ErrorValue::new(
                        ErrorCode::KuangPackageInvalid,
                        format!(
                            "`{}` declares `{path}`, which cannot be read: {error}",
                            package.manifest.package.id
                        ),
                    )
                    .with_help("a declared contribution document is part of the package"),
                );
                continue;
            }
        };
        let document = match CommandDocument::parse(&text) {
            Ok(document) => document,
            Err(error) => {
                problems.push(
                    ErrorValue::new(
                        ErrorCode::KuangPackageInvalid,
                        format!("{}: {}", file.display(), error.message()),
                    )
                    .with_help(error.help().unwrap_or_default()),
                );
                continue;
            }
        };
        for contribution in document.commands {
            let declared = ContributedCommand {
                id: contribution.id,
                verb: contribution.verb,
                target: contribution.target,
                summary: contribution.summary,
                input: contribution.input,
                output: contribution.output,
                capabilities: contribution.capabilities,
                argument_mode: contribution.argument_mode,
                examples: contribution.examples,
                origin: origin.clone(),
            };
            match declared.into_contract() {
                Ok(contract) => commands.push(contract),
                Err(error) => problems.push(error),
            }
        }
    }
    (commands, problems)
}

/// The `contributions.commands` paths of a manifest.
fn declared_paths(manifest: &Manifest) -> Vec<String> {
    manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.commands.clone())
        .unwrap_or_default()
}

/// The origin the host attributes a package's contributions to (spec §31.64).
pub(crate) fn origin_of(manifest: &Manifest) -> Origin {
    Origin::plugin(&manifest.package.id, &manifest.package.version)
}
