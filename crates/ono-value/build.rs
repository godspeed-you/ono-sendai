//! Transcodes the contract documents this crate embeds from YAML to JSON at build time.
//!
//! The documents stay YAML in `docs/contracts/`, where people read and edit them. Parsing YAML is the
//! single largest cost of a cold start, and it was paid on every start for text that never
//! changes between builds; the same documents as JSON deserialize in a fraction of the time. The
//! transcoding is exact — one YAML value model in, the same value model out — and a unit test in
//! the crate compares what was embedded with what is on disk (ADR-0571).

use std::path::{Path, PathBuf};

fn main() {
    let manifest = env_path("CARGO_MANIFEST_DIR");
    let out = env_path("OUT_DIR");
    let spec = manifest.join("../../docs/contracts");
    let schemas = transcode_directory(&spec.join("schemas"), &out.join("schemas"));
    write_inventory(&schemas, &out.join("schema_contracts.rs"));
}

/// Writes the list `builtin.rs` embeds: one `include_str!` per schema document, in name order.
///
/// Generated from the directory rather than typed beside it, so a schema document added under
/// `docs/contracts/schemas/` is embedded by the build that sees it (issue #159, ADR-0930).
fn write_inventory(schemas: &[PathBuf], to: &Path) {
    let mut source = String::from("&[\n");
    for json in schemas {
        source.push_str(&format!(
            "    include_str!({:?}),\n",
            json.display().to_string()
        ));
    }
    source.push_str("]\n");
    if let Err(error) = std::fs::write(to, source) {
        fail(&format!("cannot write {}: {error}", to.display()));
    }
}

/// A schema document is `<stem>.v<N>.yaml`; anything else in the directory (the `deferred.yaml`
/// register) is a contract about schemas, not one.
fn is_schema_document(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".yaml"))
        .and_then(|stem| stem.rsplit_once(".v"))
        .is_some_and(|(_, version)| {
            !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
        })
}

/// Transcodes every `*.yaml` in `from` into `<to>/<stem>.json`, and returns the JSON paths of the
/// schema documents among them, sorted.
fn transcode_directory(from: &Path, to: &Path) -> Vec<PathBuf> {
    // The directory itself, so a document added or removed there rebuilds the crate.
    println!("cargo:rerun-if-changed={}", from.display());
    let Ok(entries) = std::fs::read_dir(from) else {
        fail(&format!("cannot read {}", from.display()));
    };
    let mut schemas = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "yaml")
        {
            let json = to.join(path.with_extension("json").file_name().unwrap_or_default());
            transcode(&path, &json);
            if is_schema_document(&path) {
                schemas.push(json);
            }
        }
    }
    schemas.sort();
    schemas
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
#[allow(
    clippy::disallowed_macros,
    reason = "a build script reports to cargo, which always reads its stderr (issue #163)"
)]
fn fail(message: &str) -> ! {
    eprintln!("error: {message}");
    std::process::exit(1)
}
