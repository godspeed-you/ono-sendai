//! Transcodes the contract documents this crate embeds from YAML to JSON at build time.
//!
//! The documents stay YAML in `docs/spec/`, where people read and edit them. Parsing YAML is the
//! single largest cost of a cold start, and it was paid on every start for text that never
//! changes between builds; the same documents as JSON deserialize in a fraction of the time. The
//! transcoding is exact — one YAML value model in, the same value model out — and a unit test in
//! the crate compares what was embedded with what is on disk (ADR-0571).

use std::path::{Path, PathBuf};

fn main() {
    let manifest = env_path("CARGO_MANIFEST_DIR");
    let out = env_path("OUT_DIR");
    let spec = manifest.join("../../docs/spec");
    transcode_directory(&spec.join("commands"), &out.join("commands"));
    for name in ["verbs", "targets", "capabilities"] {
        transcode(
            &spec.join(format!("{name}.yaml")),
            &out.join(format!("{name}.json")),
        );
    }
}

/// Transcodes every `*.yaml` in `from` into `<to>/<stem>.json`.
fn transcode_directory(from: &Path, to: &Path) {
    // The directory itself, so a document added or removed there rebuilds the crate.
    println!("cargo:rerun-if-changed={}", from.display());
    let Ok(entries) = std::fs::read_dir(from) else {
        fail(&format!("cannot read {}", from.display()));
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "yaml")
        {
            transcode(
                &path,
                &to.join(path.with_extension("json").file_name().unwrap_or_default()),
            );
        }
    }
}

/// Transcodes one YAML document into one JSON document.
fn transcode(from: &Path, to: &Path) {
    println!("cargo:rerun-if-changed={}", from.display());
    let Ok(yaml) = std::fs::read_to_string(from) else {
        fail(&format!("cannot read {}", from.display()));
    };
    let document: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&yaml) {
        Ok(document) => document,
        Err(error) => fail(&format!("{} is not valid YAML: {error}", from.display())),
    };
    let json = match serde_json::to_string(&document) {
        Ok(json) => json,
        Err(error) => fail(&format!(
            "{} does not transcode to JSON: {error}",
            from.display()
        )),
    };
    if let Some(parent) = to.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        fail(&format!("cannot create {}: {error}", parent.display()));
    }
    if let Err(error) = std::fs::write(to, json) {
        fail(&format!("cannot write {}: {error}", to.display()));
    }
}

fn env_path(name: &str) -> PathBuf {
    match std::env::var_os(name) {
        Some(value) => PathBuf::from(value),
        None => fail(&format!("cargo did not set {name}")),
    }
}

/// A build defect: said once, on stderr, and the build stops.
fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(1)
}
