//! The half of the spatial registry drift check that only `ono-cli` can make: the settings.
//!
//! Spec v0.4 §41.3 makes `docs/contracts/spatial/` the source of help, completion, tests and SDK
//! bindings, and §26.3 requires the landmark thresholds to be "inspectable and configurable" —
//! which means the registry's `settings` block and the typed catalogue of ADR-0094 are two
//! spellings of one thing. `cargo run -p xtask -- spec-check` holds the rest of the registry
//! against `ono-spatial-core`; it cannot reach the catalogue, because `xtask` does not depend on
//! this crate. This suite is that missing direction, modelled on `providers.rs`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ono_value::Value;
use serde_yaml_ng::Value as Yaml;

fn registry() -> Yaml {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/spatial/spatial.yaml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} must be readable: {error}", path.display()));
    serde_yaml_ng::from_str(&text).expect("the spatial registry is valid YAML")
}

/// Every `spatial.*` setting the registry declares, with its type and default as text.
fn declared() -> BTreeMap<String, (String, String)> {
    registry()["settings"]
        .as_sequence()
        .expect("the registry declares its settings")
        .iter()
        .filter_map(|setting| {
            let key = setting["key"].as_str()?.to_owned();
            let ty = setting["type"].as_str()?.to_owned();
            let default = serde_yaml_ng::to_string(&setting["default"])
                .ok()?
                .trim()
                .trim_matches('\'')
                .to_owned();
            Some((key, (ty, default)))
        })
        .collect()
}

/// Every `spatial.*` setting the shell declares, with its type and default as text.
fn implemented() -> BTreeMap<String, (String, String)> {
    ono_cli::settings::CATALOGUE
        .iter()
        .filter(|setting| setting.key.starts_with("spatial."))
        .map(|setting| {
            let rendered = match setting.default_value() {
                Value::Bool(flag) => flag.to_string(),
                Value::Int(number) => number.to_string(),
                Value::String(text) => text.to_string(),
                other => format!("{other:?}"),
            };
            (
                setting.key.to_owned(),
                (setting.ty.name().to_owned(), rendered),
            )
        })
        .collect()
}

#[test]
fn should_declare_exactly_the_spatial_settings_the_shell_implements() {
    // Drift in both directions, as `providers.rs` checks the provider registry: a declared
    // setting nothing implements is a promise nobody keeps, and an implemented one no file
    // declares is undocumented surface.
    let declared: BTreeSet<String> = declared().keys().cloned().collect();
    let implemented: BTreeSet<String> = implemented().keys().cloned().collect();
    assert_eq!(
        declared, implemented,
        "docs/contracts/spatial/spatial.yaml and `ono_cli::settings::CATALOGUE` must agree exactly"
    );
}

#[test]
fn should_give_every_spatial_setting_the_type_and_default_the_registry_declares() {
    // §47 spells out each default and §26.3 makes the landmark thresholds configurable; two
    // defaults for one key would mean the documentation and the shell disagree about when a
    // landmark fires or how large a map may get.
    let declared = declared();
    let implemented = implemented();
    for (key, (ty, default)) in &declared {
        let Some((served_type, served_default)) = implemented.get(key) else {
            continue;
        };
        assert_eq!(
            ty, served_type,
            "`{key}` is declared and implemented with one type"
        );
        assert_eq!(
            default, served_default,
            "`{key}` is declared and implemented with one default"
        );
    }
}

#[test]
fn should_configure_every_landmark_threshold_the_landmark_registry_names() {
    // §26.3: "Thresholds MUST be inspectable and configurable." A threshold whose setting does
    // not exist is inspectable and not configurable, which is half of a requirement.
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/spatial/landmarks.yaml");
    let text = std::fs::read_to_string(&path).expect("the landmark registry is readable");
    let landmarks: Yaml = serde_yaml_ng::from_str(&text).expect("valid YAML");
    let implemented = implemented();

    for landmark in landmarks["landmarks"]
        .as_sequence()
        .expect("the registry declares its landmarks")
    {
        let reason = landmark["reason"].as_str().unwrap_or_default();
        let Some(key) = landmark["threshold"]["setting"].as_str() else {
            continue;
        };
        assert!(
            implemented.contains_key(key),
            "§26.3: the threshold of `{reason}` is configured by `{key}`, which the shell does \
             not declare as a setting"
        );
    }
}
