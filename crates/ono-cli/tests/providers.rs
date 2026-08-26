//! The provider conformance link of spec §35.3: `docs/spec/providers/*.yaml` declares what each
//! provider advertises, and this suite fails when the registry and the declarations drift.
//!
//! The declarations are the contract (spec §47, level 6); the registry is the implementation.
//! A capability the code advertises but no file declares is undocumented surface; one a file
//! declares but the code does not advertise is a promise nobody keeps. Both are drift, and both
//! fail here rather than waiting for a user to notice.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn spec_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec")
}

/// Every provider declaration, parsed.
fn declarations() -> Vec<serde_yaml_ng::Value> {
    let dir = spec_dir().join("providers");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("docs/spec/providers/ must exist: {error}"))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "yaml")
        })
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "docs/spec/providers/ declares at least one provider (spec §47)"
    );

    files
        .iter()
        .flat_map(|path| {
            let text = std::fs::read_to_string(path).expect("a readable declaration");
            let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).expect("valid YAML");
            parsed["providers"]
                .as_sequence()
                .unwrap_or_else(|| panic!("{} declares a `providers` list", path.display()))
                .clone()
        })
        .collect()
}

fn string_set(value: &serde_yaml_ng::Value, key: &str) -> BTreeSet<String> {
    value[key]
        .as_sequence()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The registry as the shell builds it, including the asynchronous providers.
fn built_registry() -> ono_provider_api::ProviderRegistry {
    let mut registry = ono_cli::providers::registry(std::iter::empty());
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(ono_cli::providers::register_async(&mut registry));
    registry
}

#[test]
fn should_advertise_exactly_what_the_declarations_promise() {
    let registry = built_registry();

    let mut declared: Vec<(String, BTreeSet<String>, BTreeSet<String>, BTreeSet<String>)> =
        declarations()
            .iter()
            .map(|provider| {
                (
                    provider["id"].as_str().expect("an id").to_owned(),
                    string_set(provider, "targets"),
                    string_set(provider, "capabilities"),
                    string_set(provider, "schemas"),
                )
            })
            .collect();
    declared.sort();

    let mut advertised: Vec<(String, BTreeSet<String>, BTreeSet<String>, BTreeSet<String>)> =
        registry
            .providers()
            .iter()
            .map(|provider| {
                (
                    provider.id().to_owned(),
                    provider.targets().iter().map(|t| (*t).to_owned()).collect(),
                    provider
                        .capabilities()
                        .iter()
                        .map(|capability| capability.id().to_owned())
                        .collect(),
                    provider
                        .schemas()
                        .iter()
                        .map(|schema| schema.id().to_string())
                        .collect(),
                )
            })
            .collect();
    advertised.sort();

    assert_eq!(
        declared, advertised,
        "docs/spec/providers/*.yaml and the built registry must agree exactly: an advertised \
         capability no file declares is undocumented surface, a declared one nothing advertises \
         is a promise nobody keeps"
    );
}

#[test]
fn should_declare_only_capabilities_and_schemas_the_registries_define() {
    let capabilities = std::fs::read_to_string(spec_dir().join("capabilities.yaml"))
        .expect("the capability registry");
    let schema_files: BTreeSet<String> = std::fs::read_dir(spec_dir().join("schemas"))
        .expect("the schema registry")
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    for provider in declarations() {
        let id = provider["id"].as_str().expect("an id");
        for capability in string_set(&provider, "capabilities") {
            assert!(
                capabilities.contains(&format!("id: {capability}")),
                "{id} declares `{capability}`, which docs/spec/capabilities.yaml does not define"
            );
        }
        for schema in string_set(&provider, "schemas") {
            let file = format!("{}.v1.yaml", schema.replace("ono.", "").replace('/', ".v"));
            let base = schema
                .split('/')
                .next()
                .unwrap_or(&schema)
                .replace("ono.", "");
            assert!(
                schema_files.iter().any(|name| name.starts_with(&base)),
                "{id} declares `{schema}`, and no file under docs/spec/schemas/ looks like \
                 `{file}`"
            );
        }
    }
}
