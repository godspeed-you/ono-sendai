//! Transcodes the operation registry this crate embeds from YAML to JSON at build time.
//!
//! The document stays YAML in `docs/contracts/change/`, where people read and edit it. Parsing
//! YAML is the single largest cost of a cold start, and §52.1 budgets plan creation at under
//! 150 ms; the same document as JSON deserialises in a fraction of the time. The transcoding is
//! exact — one YAML value model in, the same value model out — and a unit test in the crate
//! compares what was embedded with what is on disk, exactly as `ono-command` and `ono-value` do
//! (ADR-0571).

use std::path::{Path, PathBuf};

fn main() {
    let manifest = env_path("CARGO_MANIFEST_DIR");
    let out = env_path("OUT_DIR");
    transcode(
        &manifest.join("../../docs/contracts/change/actions.yaml"),
        &out.join("actions.json"),
    );
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
