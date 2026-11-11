//! Loading the installed package a spatial command needs, before the command runs (#129,
//! spec §31.68, v0.4 §6.4, §6.5, §6.8, §20.4; ADR-1005, ADR-1008).
//!
//! A kind of place a KUANG/11 package contributes is learned from its handshake (ADR-0584), and
//! a package is lazy: §31.68's `installed manifest -> registry placeholders -> first invocation
//! -> runtime load`. `get pod` is a first invocation and loads the package; `jump @alpha`,
//! `jump pod/checkout`, `find place --type Pod` and `find place --role workload` are not, because
//! they are the shell's own commands and never reach the package's registry entry. So the words
//! they carry are read here, before the command runs, and where one names a kind of place, a pin
//! or a role this session does not know yet, the installed packages that can answer for it are
//! loaded the way a contributed command would load its own.
//!
//! The declaration on disk decides. A role is declared on a target in `contributions.targets`,
//! so a role names exactly the packages that declare it. A kind of place is named by its schema's
//! display name or id, which a package's `contributions.schemas` documents state on disk. A
//! package that declares its schema names only in its handshake is not started for a word it
//! might answer to: it is found once something has loaded it (ADR-1021).

use crate::session::Session;

/// Loads the installed, not yet loaded packages the spatial commands of `list` need.
///
/// Nothing here refuses: a package that will not load is left as it was, and the command then
/// answers exactly as it would have — with the refusal for the word nobody answers for.
pub fn load_contributors(session: &mut Session, list: &ono_parser::StageList, source: &str) {
    let mut roles: Vec<String> = Vec::new();
    let mut kinds: Vec<String> = Vec::new();
    // Schema ids a contributed relation `follow` names is waiting for (ADR-1008).
    let mut awaited: Vec<String> = Vec::new();
    for stage in &list.stages {
        let span = stage.span;
        let Some(text) = source.get(span.start() as usize..span.end() as usize) else {
            continue;
        };
        let words: Vec<&str> = text
            .split_whitespace()
            .map(|word| word.trim_matches(|c| c == '"' || c == '\''))
            .collect();
        match words.as_slice() {
            ["jump", selector, ..] => kinds.extend(unknown_kind_in(session, selector)),
            ["follow", word, ..] => awaited.extend(ono_spatial_core::relation::awaited_by(word)),
            ["find", "place", rest @ ..] => {
                let mut rest = rest.iter();
                while let Some(&word) = rest.next() {
                    if !word.starts_with("--") {
                        continue;
                    }
                    let (option, inline): (&str, Option<&str>) = match word.split_once('=') {
                        Some((option, value)) => (option, Some(value)),
                        None => (word, None),
                    };
                    let value = inline.or_else(|| rest.next().copied());
                    match (option, value) {
                        ("--type", Some(kind)) if unknown_kind(kind) => {
                            kinds.push(kind.to_owned());
                        }
                        ("--role", Some(role))
                            if !ono_spatial_core::types::known_roles().contains(&role) =>
                        {
                            roles.push(role.to_owned());
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    if kinds.is_empty() && roles.is_empty() && awaited.is_empty() {
        return;
    }
    let (installed, _) = crate::plugins::installed(session);
    for package in installed {
        let id = package.manifest.package.id.clone();
        if crate::plugins::loaded_package(session, &id).is_some() {
            continue;
        }
        let declared_roles = crate::plugin_registry::declared_target_roles(&package);
        let declared_schemas = crate::plugin_registry::declared_target_schemas(&package);
        let answers = roles.iter().any(|role| declared_roles.contains(role))
            || awaited
                .iter()
                .any(|schema| declared_schemas.contains(schema))
            || (!kinds.is_empty() && may_contribute(&package, &kinds));
        if answers {
            let _ = crate::plugins::load_plugin_with(
                session,
                &id,
                &crate::plugins::LoadOptions {
                    silent: true,
                    ..crate::plugins::LoadOptions::default()
                },
            );
        }
    }
}

/// The kind of place a `jump` selector names that this session does not know: the kind a pin
/// recorded, or the `<kind>` of a `<kind>/<key>` selector (v0.4 §11.2, §20.4).
fn unknown_kind_in(session: &Session, selector: &str) -> Option<String> {
    if let Some(name) = selector.strip_prefix('@') {
        return crate::spatial::PinStore::of(session)
            .and_then(|store| store.unplaced().ok())?
            .into_iter()
            .find(|(pinned, _)| pinned == name)
            .map(|(_, kind)| kind);
    }
    let (kind, key) = selector.split_once('/')?;
    let word = !kind.is_empty()
        && kind
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (word && !key.is_empty() && unknown_kind(kind)).then(|| kind.to_owned())
}

/// Whether `kind` is a word no declared or loaded kind of place answers to.
fn unknown_kind(kind: &str) -> bool {
    !ono_spatial_core::types::known()
        .iter()
        .any(|known| known.as_str().eq_ignore_ascii_case(kind))
}

/// Whether an installed package contributes one of `kinds`, as its declaration on disk says
/// (§31.68; ADR-1021).
///
/// A kind is a schema's display name or its id, and both are known on disk only where the
/// package's `contributions.schemas` documents state them for a schema one of its targets
/// declares. A package that states none is not started for a word it might answer to: a package
/// is loaded on its first invocation, never on a word nobody declared (K11P).
fn may_contribute(package: &crate::kuang_host::Installed, kinds: &[String]) -> bool {
    let targets = crate::plugin_registry::declared_target_schemas(package);
    if targets.is_empty() {
        return false;
    }
    let schemas = declared_schemas(package);
    kinds.iter().any(|kind| {
        schemas.iter().any(|(id, name)| {
            targets.contains(id) && (name.eq_ignore_ascii_case(kind) || id == kind)
        })
    })
}

/// The `(id, name)` of every schema a package's `contributions.schemas` documents declare.
///
/// Only the two fields this module needs are read, so a document written for a later version of
/// the schema contract still answers what it names.
fn declared_schemas(package: &crate::kuang_host::Installed) -> Vec<(String, String)> {
    schema_documents(package)
        .into_iter()
        .map(|declared| (declared.id, declared.name))
        .collect()
}

/// One schema of a `contributions.schemas` document, as far as the spatial layer reads it.
#[derive(serde::Deserialize)]
struct Declared {
    id: String,
    name: String,
    /// The fields naming the provider-native scope of an object, outermost first (ADR-1009).
    #[serde(default)]
    scope: Vec<String>,
}

/// Records the scope dimensions the schemas of the package `id`, loaded from `directory`, declare
/// on disk, so its places are placed in the scope they belong to (external-system-provider §9.2;
/// ADR-1009).
///
/// Called when the package is loaded, with the directory the instance was loaded from — the
/// session's plugin path, not the one the process started with. A schema outside the package's
/// own namespace (`<package-id>.`) is another package's to describe (§31.5), and its scope is
/// ignored.
pub fn declare_scopes(id: &str, directory: &std::path::Path) {
    let Ok(Some(package)) = crate::kuang_host::read_package(directory) else {
        return;
    };
    let namespace = format!("{id}.");
    for declared in schema_documents(&package) {
        if !declared.id.starts_with(&namespace) {
            continue;
        }
        let dimensions: Vec<&str> = declared.scope.iter().map(String::as_str).collect();
        ono_spatial_core::types::declare_scope(&declared.id, &dimensions);
    }
}

/// The schemas a package's `contributions.schemas` documents declare, read tolerantly: only the
/// fields this module needs, so a document written for a later version still answers.
fn schema_documents(package: &crate::kuang_host::Installed) -> Vec<Declared> {
    #[derive(serde::Deserialize)]
    struct Document {
        #[serde(default)]
        schemas: Vec<Declared>,
    }
    let paths = package
        .manifest
        .contributions
        .as_ref()
        .and_then(|contributions| contributions.schemas.clone())
        .unwrap_or_default();
    paths
        .iter()
        .filter_map(|path| std::fs::read_to_string(package.directory.join(path)).ok())
        .filter_map(|text| serde_yaml_ng::from_str::<Document>(&text).ok())
        .flat_map(|document| document.schemas)
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    /// A package on disk whose schema document declares a scope for `schema`.
    fn package_declaring_a_scope_for(schema: &str) -> ono_testkit::Scratch {
        let scratch = ono_testkit::scratch();
        let package = scratch.path().join("dev.example.claimant");
        std::fs::create_dir_all(package.join("contributions")).expect("the package directory");
        std::fs::write(
            package.join("manifest.yaml"),
            r#"format: kuang-package/1
package:
  id: dev.example.claimant
  name: claimant
  version: 0.1.0
  description: Declares scopes.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: ">=11.1 <12"
  ono_language: ">=0.2"
  platforms: [linux-amd64, linux-arm64]
runtime:
  kind: native-process
  entry: runtime/claimant
  memory_max: 64MiB
  cpu_budget: interactive
  startup: lazy
roles: [provider]
network:
  outbound: none
contributions:
  schemas: [contributions/schemas.yaml]
"#,
        )
        .expect("the manifest");
        std::fs::write(
            package.join("contributions/schemas.yaml"),
            format!("schemas:\n  - id: {schema}\n    name: Claimed\n    scope: [tenant]\n"),
        )
        .expect("the schema document");
        scratch
    }

    #[test]
    fn should_ignore_a_scope_a_package_declares_for_a_schema_outside_its_namespace() {
        // §31.5: `<publisher>.<package>` is its owner's. A package's schema document may describe
        // the package's own schemas; a scope it declares for another package's schema would
        // re-scope that package's places (last load wins), so it is ignored.
        let foreign = ono_spatial_core::types::contribute(
            "dev.example.victim.thing/1",
            "VictimThing",
            "victim-thing",
            &["uid"],
            "dev.example.victim",
            &[],
            None,
        );
        let home = package_declaring_a_scope_for("dev.example.victim.thing/1");
        super::declare_scopes(
            "dev.example.claimant",
            &home.path().join("dev.example.claimant"),
        );
        assert!(
            ono_spatial_core::types::scope_dimensions(foreign).is_empty(),
            "another package's kind keeps the scope its own package declared"
        );
    }

    #[test]
    fn should_declare_the_scope_a_package_states_for_its_own_schema() {
        let own = ono_spatial_core::types::contribute(
            "dev.example.claimant.thing/1",
            "ClaimantThing",
            "claimant-thing",
            &["uid"],
            "dev.example.claimant",
            &[],
            None,
        );
        let home = package_declaring_a_scope_for("dev.example.claimant.thing/1");
        super::declare_scopes(
            "dev.example.claimant",
            &home.path().join("dev.example.claimant"),
        );
        assert_eq!(
            ono_spatial_core::types::scope_dimensions(own),
            &["tenant"],
            "the package's own kind sits in the scope it declares"
        );
    }
}
