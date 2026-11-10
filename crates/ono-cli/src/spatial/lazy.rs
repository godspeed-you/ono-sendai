//! Loading the installed package a spatial command needs, before the command runs (#129,
//! spec §31.68, v0.4 §6.5, §6.8, §20.4; ADR-1005).
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
//! The declaration on disk narrows the set where it can. A role is declared on a target in
//! `contributions.targets`, so a role names exactly the packages that declare it. A kind of place
//! is named by its schema's display name, which a package's `contributions.schemas` documents
//! state on disk; a package that declares its schemas only in its handshake cannot be ruled out
//! without running it, so it is loaded too when it declares a target at all — once per session,
//! and only for a word that would otherwise be refused.

use crate::session::Session;

/// Loads the installed, not yet loaded packages the spatial commands of `list` need.
///
/// Nothing here refuses: a package that will not load is left as it was, and the command then
/// answers exactly as it would have — with the refusal for the word nobody answers for.
pub fn load_contributors(session: &mut Session, list: &ono_parser::StageList, source: &str) {
    let mut roles: Vec<String> = Vec::new();
    let mut kinds: Vec<String> = Vec::new();
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
    if kinds.is_empty() && roles.is_empty() {
        return;
    }
    let (installed, _) = crate::plugins::installed(session);
    for package in installed {
        let id = package.manifest.package.id.clone();
        if crate::plugins::loaded_package(session, &id).is_some() {
            continue;
        }
        let declared_roles = crate::plugin_registry::declared_target_roles(&package);
        let answers = roles.iter().any(|role| declared_roles.contains(role))
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

/// Whether an installed package may contribute one of `kinds`, as far as its declaration on disk
/// says (§31.68).
///
/// A kind is a schema's display name or its id. Where the package's `contributions.schemas`
/// documents name its schemas, they settle it; where they do not, a package that declares a
/// target may still answer with one, and only loading it can tell.
fn may_contribute(package: &crate::kuang_host::Installed, kinds: &[String]) -> bool {
    let targets = crate::plugin_registry::declared_target_schemas(package);
    if targets.is_empty() {
        return false;
    }
    let schemas = declared_schemas(package);
    if schemas.is_empty() {
        return true;
    }
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
    #[derive(serde::Deserialize)]
    struct Document {
        #[serde(default)]
        schemas: Vec<Declared>,
    }
    #[derive(serde::Deserialize)]
    struct Declared {
        id: String,
        name: String,
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
        .map(|declared| (declared.id, declared.name))
        .collect()
}
