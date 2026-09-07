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

use ono_command::{
    CommandContract, CommandRegistry, ContributedCommand, ContributedParameter, Origin,
};
use ono_core::ErrorCode;
use ono_kuang_protocol::{
    CommandDocument, Manifest, ParameterContribution, TargetContribution, TargetDocument,
};
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

/// The infix that marks a `get` the shell must answer through the provider path.
///
/// A contributed command's id is `<package.id>.command.<kebab>`; a contributed target's synthetic
/// entry is `<package.id>.target.<kebab>`. The two are routed differently — invoked versus
/// queried — and the id is where that decision is readable, in the registry, in `explain` and in
/// `get command`, rather than being inferred from the verb.
pub(crate) const TARGET_INFIX: &str = ".target.";

/// The command id the shell registers for a target a package contributes.
pub(crate) fn target_command_id(package_id: &str, target: &str) -> String {
    format!("{package_id}{TARGET_INFIX}{target}")
}

/// What one package declares, as registry entries attributed to it (spec §31.64).
pub(crate) fn declarations(package: &Installed) -> (Vec<CommandContract>, Vec<ErrorValue>) {
    let mut commands = Vec::new();
    let mut problems = Vec::new();
    let origin = origin_of(&package.manifest);
    let (targets, refused) = target_declarations(package, &origin);
    commands.extend(targets);
    problems.extend(refused);
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
                selectors: parameters(contribution.selectors),
                options: parameters(contribution.options),
                risk: contribution.risk,
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

/// The targets one package contributes, as `get <target>` entries attributed to it.
///
/// A target becomes a registry placeholder exactly as a command does (spec §31.68), so that
/// `get pod`, its help page, its completion and `explain` all answer before the package has run.
/// The entry it becomes is a `get` whose output is a stream of the schema the target declared —
/// which is what lets the pipeline type-check the stage without loading anything.
fn target_declarations(
    package: &Installed,
    origin: &Origin,
) -> (Vec<CommandContract>, Vec<ErrorValue>) {
    let mut commands = Vec::new();
    let (targets, mut problems) = declared_targets(package);
    for target in targets {
        let declared = ContributedCommand {
            id: target_command_id(&package.manifest.package.id, &target.name),
            verb: "get".to_owned(),
            target: target.name.clone(),
            summary: target.summary.clone(),
            input: None,
            output: format!("stream<{}>", target.schema),
            capabilities: Vec::new(),
            argument_mode: "expression".to_owned(),
            // A target declares no positional arguments: the one word after the verb is the
            // target itself, and the registry resolved the entry by it.
            selectors: Vec::new(),
            options: parameters(target.options.clone()),
            // A target is read by construction: `get <target>` answers, and §21.2 of
            // `docs/architecture/external-system-provider.md` forbids a getter from mutating.
            risk: Some("read".to_owned()),
            examples: vec![format!("get {}", target.name)],
            origin: origin.clone(),
        };
        match declared.into_contract() {
            Ok(contract) => commands.push(contract),
            Err(error) => problems.push(error),
        }
    }
    (commands, problems)
}

/// The schema ids the targets a package declares on disk answer with (spec §31.23, §31.68).
///
/// Read without starting anything, which is what makes it usable *before* the package runs: a
/// relation shape naming one of these names a kind of place the package will contribute, and the
/// two halves of that statement are then both on disk at the moment the shell reads either
/// (ADR-0585).
pub(crate) fn declared_target_schemas(package: &Installed) -> Vec<String> {
    let (targets, _) = declared_targets(package);
    let mut schemas: Vec<String> = targets.into_iter().map(|target| target.schema).collect();
    schemas.sort();
    schemas.dedup();
    schemas
}

/// The `(schema, parent)` pairs of every on-disk target that declares a spatial parent, for the
/// load-time check of ADR-0597.
pub(crate) fn declared_target_parents(package: &Installed) -> Vec<(String, String, String)> {
    let (targets, _) = declared_targets(package);
    targets
        .into_iter()
        .filter_map(|target| {
            target
                .parent
                .map(|parent| (target.name, target.schema, parent))
        })
        .collect()
}

/// The target declarations of one package's `contributions.targets` documents, and what did not
/// read.
fn declared_targets(package: &Installed) -> (Vec<TargetContribution>, Vec<ErrorValue>) {
    let mut targets = Vec::new();
    let mut problems = Vec::new();
    let paths = package
        .manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.targets.clone())
        .unwrap_or_default();
    for path in paths {
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
        match TargetDocument::parse(&text) {
            Ok(document) => targets.extend(document.targets),
            Err(error) => problems.push(
                ErrorValue::new(
                    ErrorCode::KuangPackageInvalid,
                    format!("{}: {}", file.display(), error.message()),
                )
                .with_help(error.help().unwrap_or_default()),
            ),
        }
    }
    (targets, problems)
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

/// The declared arguments of a contribution, as the command registry's vocabulary spells them.
///
/// A straight rename across a crate boundary: `ono-kuang-protocol` owns what crosses the wire and
/// `ono-command` owns what the registry holds, and neither depends on the other.
fn parameters(declared: Vec<ParameterContribution>) -> Vec<ContributedParameter> {
    declared
        .into_iter()
        .map(|parameter| ContributedParameter {
            default: parameter.default_text(),
            name: parameter.name,
            declared_type: parameter.declared_type,
            doc: parameter.doc,
            repeatable: parameter.repeatable,
            optional_value: parameter.optional_value,
        })
        .collect()
}
